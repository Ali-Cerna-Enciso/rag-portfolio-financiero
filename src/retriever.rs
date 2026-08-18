use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{doc, Index, IndexReader, ReloadPolicy, TantivyDocument};

use crate::indicators::expand_financial_queries;
use crate::ingest::{ChildChunk, Corpus, ParentChunk};

static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[a-záéíóúüñ0-9.%/]+").unwrap()
});

pub const SPANISH_STOPWORDS: &[&str] = &[
    "de", "la", "que", "el", "en", "y", "a", "los", "del", "se", "las", "por", "un", "para",
    "con", "no", "una", "su", "al", "lo", "como", "mas", "pero", "sus", "le", "ya", "o",
    "este", "si", "porque", "esta", "entre", "cuando", "muy", "sin", "sobre", "tambien",
    "me", "hasta", "hay", "donde", "quien", "desde", "todo", "nos", "durante", "todos",
    "uno", "les", "ni", "contra", "otros", "ese", "eso", "ante", "ellos", "e", "esto",
    "mi", "antes", "algunos", "que", "unos", "yo", "otro", "otras", "otra", "el", "tanto",
    "esa", "estos", "mucho", "quienes", "nada", "muchos", "cual", "poco", "ella", "estar",
    "estas", "algunas", "algo", "nosotros", "mi", "mis", "tus", "cual", "cuales", "como",
];

/// Normaliza un texto para búsqueda: minúsculas + sin diacríticos del español
/// (á→a, é→e, í→i, ó→o, ú→u, ü→u). "DÓLARES" / "dólares" / "dolares" → "dolares".
/// Aplica a: índice Tantivy (campo content_idx), queries y TF-IDF (VectorSearcher).
pub fn normalize_text(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'á' | 'Á' => 'a',
            'é' | 'É' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'Ó' => 'o',
            'ú' | 'Ú' | 'ü' | 'Ü' => 'u',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

pub fn tokenize_text(text: &str) -> Vec<String> {
    let stopwords: HashSet<&str> = SPANISH_STOPWORDS.iter().cloned().collect();
    let mut tokens = Vec::new();
    let norm = normalize_text(text);
    for mat in TOKEN_RE.find_iter(&norm) {
        let token = mat.as_str();
        if token.len() >= 2 && !stopwords.contains(token) {
            tokens.push(token.to_string());
        }
    }
    tokens
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }
}

pub fn reciprocal_rank_fusion(
    ranked_lists: &[Vec<String>],
    rrf_k: f64,
    top_n: usize,
) -> Vec<(String, f64)> {
    let mut score_map: HashMap<String, f64> = HashMap::new();

    for list in ranked_lists {
        for (rank_idx, doc_id) in list.iter().enumerate() {
            let rank = (rank_idx + 1) as f64;
            let weight = 1.0 / (rrf_k + rank);
            *score_map.entry(doc_id.clone()).or_default() += weight;
        }
    }

    let mut scored_vec: Vec<(String, f64)> = score_map.into_iter().collect();
    scored_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored_vec.truncate(top_n);
    scored_vec
}

pub fn normalize_issuer_name(s: &str) -> String {
    let low = s.trim().to_lowercase().replace(['_', '-', ' '], "");
    if low.contains("worldbank") || low.contains("bancomundial") || low.contains("world") || low.contains("bank") {
        "worldbank".to_string()
    } else if low.contains("inei") || low.contains("enapres") || low.contains("fichatecnica") {
        "inei".to_string()
    } else if low.contains("financiera") || low.contains("efectiva") {
        "financieraefectiva".to_string()
    } else if low.contains("ferreycorp") || low.contains("ferreyros") {
        "ferreycorp".to_string()
    } else {
        low
    }
}

pub fn matches_issuer_filter(doc_issuer: &str, filter: &str) -> bool {
    let f = normalize_issuer_name(filter);
    if f.is_empty() {
        return true;
    }
    let d = normalize_issuer_name(doc_issuer);
    d.contains(&f) || f.contains(&d)
}

/// Propuesta C — segmenta el query compuesto en sub-frases, una por emisor.
/// Devuelve (emisor canónico, sub-frase que lo menciona). Si un emisor no
/// tiene segmento propio, se usa el query completo (fallback).
pub fn segment_query_by_issuer(question: &str, issuers: &[String]) -> Vec<(String, String)> {
    let q = question.trim().to_string();
    // Cláusulas: separadas por comas, punto, ¿?, y la conjunción " y "
    // ("...ventas de Ferreycorp en dolares en 2025 y que patrimonio supero
    // Financiera Efectiva..." → dos cláusulas, una por emisor).
    let segments: Vec<String> = q
        .split([',', ';', '\n', '¿', '?'])
        .flat_map(|s| s.split(" y "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut out: Vec<(String, String)> = Vec::new();
    for iss in issuers {
        let needle: &str = match iss.as_str() {
            "ferreycorp" => "ferreycorp",
            "financieraefectiva" => "financiera",
            "worldbank" => "banco mundial",
            "inei" => "inei",
            other => other,
        };
        let mut seg = segments
            .iter()
            .find(|s| normalize_text(s).contains(needle))
            .cloned()
            .unwrap_or_else(|| q.clone());
        // Quitar prefijos conversacionales que no aportan tokens discriminantes.
        for prefix in [
            "comparando los reportes del corpus:",
            "comparando los reportes:",
            "segun los reportes del corpus:",
            "segun los documentos:",
        ] {
            let low = normalize_text(&seg);
            if low.starts_with(prefix) {
                let prefix_chars = prefix.chars().count();
                let cut = seg
                    .char_indices()
                    .nth(prefix_chars)
                    .map(|(i, _)| i)
                    .unwrap_or(seg.len());
                seg = seg[cut..].trim().to_string();
            }
        }
        out.push((iss.clone(), seg));
    }
    out
}

/// Propuesta C — detecta los emisores conocidos mencionados en el query.
/// Devuelve nombres canónicos normalizados, deduplicados, en orden de aparición.
/// Con ≥2 emisores el retriever ejecuta una sub-consulta por emisor (multi-query),
/// garantizando que cada tema del query tenga representación en el top-k.
pub fn detect_issuers_in_query(question: &str) -> Vec<String> {
    let q = question.to_lowercase();
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (needle, canon) in [
        ("ferreycorp", "ferreycorp"),
        ("ferreyros", "ferreycorp"),
        ("financiera", "financieraefectiva"),
        ("efectiva", "financieraefectiva"),
        ("banco mundial", "worldbank"),
        ("world bank", "worldbank"),
        ("bancomundial", "worldbank"),
        ("worldbank", "worldbank"),
        ("inei", "inei"),
        ("enapres", "inei"),
        ("ficha tecnica", "inei"),
        ("microsoft", "microsoft"),
    ] {
        if let Some(pos) = q.find(needle) {
            if !seen.contains(canon) {
                seen.insert(canon.to_string());
                found.push((pos, canon.to_string()));
            }
        }
    }
    found.sort_by_key(|(pos, _)| *pos);
    found.into_iter().map(|(_, c)| c).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedParent {
    pub parent: ParentChunk,
    pub score: f64,
    pub matched_child_ids: Vec<String>,
    pub citations: Vec<String>,
}

pub struct TantivyIndexWrapper {
    pub index: Index,
    pub reader: IndexReader,
    pub f_id: Field,
    pub f_parent_id: Field,
    pub f_document: Field,
    pub f_issuer: Field,
    pub f_doc_type: Field,
    pub f_year: Field,
    pub f_content: Field,
    pub f_content_idx: Field,
    pub f_is_table: Field,
    pub f_indicator: Field,
}

impl TantivyIndexWrapper {
    pub fn create_in_dir<P: AsRef<Path>>(dir_path: P, children: &[ChildChunk]) -> Result<Self, String> {
        let path = dir_path.as_ref();
        if path.exists() {
            let _ = fs::remove_dir_all(path);
        }
        fs::create_dir_all(path).map_err(|e| e.to_string())?;

        let mut schema_builder = Schema::builder();
        // content original: stored, sin indexar (citas con acentos intactos).
        // content_idx: indexado, normalizado (sin acentos) → "dólares"≈"dolares".
        let content_idx_options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default().set_tokenizer("default")
        );

        let f_id = schema_builder.add_text_field("id", STRING | STORED);
        let f_parent_id = schema_builder.add_text_field("parent_id", STRING | STORED);
        let f_document = schema_builder.add_text_field("document", STRING | STORED);
        let f_issuer = schema_builder.add_text_field("issuer", STRING | STORED);
        let f_doc_type = schema_builder.add_text_field("doc_type", STRING | STORED);
        let f_year = schema_builder.add_text_field("year", STRING | STORED);
        let f_content = schema_builder.add_text_field("content", TextOptions::default().set_stored());
        let f_content_idx = schema_builder.add_text_field("content_idx", content_idx_options);
        let f_is_table = schema_builder.add_u64_field("is_table", STORED);
        let f_indicator = schema_builder.add_text_field("indicator", STRING | STORED);

        let schema = schema_builder.build();
        let index = Index::create_in_dir(path, schema.clone())
            .map_err(|e| format!("Error creando índice Tantivy: {}", e))?;

        let mut index_writer = index.writer(50_000_000)
            .map_err(|e| format!("Error creando Tantivy writer: {}", e))?;

        for child in children {
            let mut doc = doc!(
                f_id => child.id.clone(),
                f_parent_id => child.parent_id.clone(),
                f_document => child.document.clone(),
                f_issuer => child.issuer.clone(),
                f_doc_type => child.doc_type.clone(),
                f_year => child.year.clone(),
                f_content => child.content.clone(),
                f_content_idx => normalize_text(&child.content),
                f_is_table => if child.is_table { 1u64 } else { 0u64 },
            );
            if let Some(ind) = &child.indicator {
                doc.add_text(f_indicator, ind);
            }
            index_writer.add_document(doc)
                .map_err(|e| format!("Error indexando child: {}", e))?;
        }

        index_writer.commit()
            .map_err(|e| format!("Error commiteando Tantivy writer: {}", e))?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| format!("Error creando Tantivy reader: {}", e))?;

        Ok(Self {
            index,
            reader,
            f_id,
            f_parent_id,
            f_document,
            f_issuer,
            f_doc_type,
            f_year,
            f_content,
            f_content_idx,
            f_is_table,
            f_indicator,
        })
    }

    pub fn create_in_ram(children: &[ChildChunk]) -> Result<Self, String> {
        let mut schema_builder = Schema::builder();
        let content_idx_options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default().set_tokenizer("default")
        );

        let f_id = schema_builder.add_text_field("id", STRING | STORED);
        let f_parent_id = schema_builder.add_text_field("parent_id", STRING | STORED);
        let f_document = schema_builder.add_text_field("document", STRING | STORED);
        let f_issuer = schema_builder.add_text_field("issuer", STRING | STORED);
        let f_doc_type = schema_builder.add_text_field("doc_type", STRING | STORED);
        let f_year = schema_builder.add_text_field("year", STRING | STORED);
        let f_content = schema_builder.add_text_field("content", TextOptions::default().set_stored());
        let f_content_idx = schema_builder.add_text_field("content_idx", content_idx_options);
        let f_is_table = schema_builder.add_u64_field("is_table", STORED);
        let f_indicator = schema_builder.add_text_field("indicator", STRING | STORED);

        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema.clone());

        let mut index_writer = index.writer(50_000_000)
            .map_err(|e| format!("Error creando Tantivy writer RAM: {}", e))?;

        for child in children {
            let mut doc = doc!(
                f_id => child.id.clone(),
                f_parent_id => child.parent_id.clone(),
                f_document => child.document.clone(),
                f_issuer => child.issuer.clone(),
                f_doc_type => child.doc_type.clone(),
                f_year => child.year.clone(),
                f_content => child.content.clone(),
                f_content_idx => normalize_text(&child.content),
                f_is_table => if child.is_table { 1u64 } else { 0u64 },
            );
            if let Some(ind) = &child.indicator {
                doc.add_text(f_indicator, ind);
            }
            index_writer.add_document(doc)
                .map_err(|e| format!("Error indexando child RAM: {}", e))?;
        }

        index_writer.commit()
            .map_err(|e| format!("Error commiteando Tantivy writer RAM: {}", e))?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| format!("Error creando Tantivy reader RAM: {}", e))?;

        Ok(Self {
            index,
            reader,
            f_id,
            f_parent_id,
            f_document,
            f_issuer,
            f_doc_type,
            f_year,
            f_content,
            f_content_idx,
            f_is_table,
            f_indicator,
        })
    }

    pub fn search(
        &self,
        query_str: &str,
        issuer_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>, String> {
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.f_content_idx, self.f_indicator]);

        let tokens = tokenize_text(query_str);
        if tokens.is_empty() {
            return Ok(vec![]);
        }
        let safe_query = tokens.join(" ");

        let query = query_parser
            .parse_query(&safe_query)
            .map_err(|e| format!("Error parseando query Tantivy: {}", e))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit * 4))
            .map_err(|e| format!("Error ejecutando search Tantivy: {}", e))?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let retrieved: TantivyDocument = searcher.doc(doc_address)
                .map_err(|e| format!("Error recuperando doc Tantivy: {}", e))?;
            
            let id = retrieved.get_first(self.f_id).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let parent_id = retrieved.get_first(self.f_parent_id).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let doc_issuer = retrieved.get_first(self.f_issuer).and_then(|v| v.as_str()).unwrap_or("");

            if let Some(issuer) = issuer_filter {
                if !matches_issuer_filter(doc_issuer, issuer) {
                    continue;
                }
            }

            results.push((id, parent_id, score));
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }
}

pub struct VectorSearcher {
    pub child_ids: Vec<String>,
    pub parent_ids: Vec<String>,
    pub issuers: Vec<String>,
    pub vectors: Vec<Vec<f32>>,
    pub vocabulary: HashMap<String, usize>,
    pub idf: Vec<f32>,
}

impl VectorSearcher {
    pub fn build(children: &[ChildChunk]) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        let num_docs = children.len().max(1);

        let mut tokenized_docs = Vec::new();
        for child in children {
            let tokens = tokenize_text(&child.content);
            let mut unique_tokens: HashSet<String> = HashSet::new();
            for t in &tokens {
                unique_tokens.insert(t.clone());
            }
            for t in unique_tokens {
                *df.entry(t).or_default() += 1;
            }
            tokenized_docs.push(tokens);
        }

        let mut vocabulary: HashMap<String, usize> = HashMap::new();
        let mut idf = Vec::new();

        for (idx, (term, count)) in df.into_iter().enumerate() {
            vocabulary.insert(term, idx);
            let val = ((num_docs as f32 + 1.0) / (count as f32 + 1.0)).ln() + 1.0;
            idf.push(val);
        }

        let vocab_size = vocabulary.len();
        let mut vectors = Vec::with_capacity(children.len());
        let mut child_ids = Vec::with_capacity(children.len());
        let mut parent_ids = Vec::with_capacity(children.len());
        let mut issuers = Vec::with_capacity(children.len());

        for (i, tokens) in tokenized_docs.into_iter().enumerate() {
            let mut tf: HashMap<usize, f32> = HashMap::new();
            for t in &tokens {
                if let Some(&term_idx) = vocabulary.get(t) {
                    *tf.entry(term_idx).or_default() += 1.0;
                }
            }

            let mut vec = vec![0.0f32; vocab_size];
            for (term_idx, count) in tf {
                vec[term_idx] = count * idf[term_idx];
            }

            let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut vec {
                    *v /= norm;
                }
            }

            vectors.push(vec);
            child_ids.push(children[i].id.clone());
            parent_ids.push(children[i].parent_id.clone());
            issuers.push(children[i].issuer.clone());
        }

        Self {
            child_ids,
            parent_ids,
            issuers,
            vectors,
            vocabulary,
            idf,
        }
    }

    pub fn embed_query(&self, query: &str) -> Vec<f32> {
        let tokens = tokenize_text(query);
        let vocab_size = self.vocabulary.len();
        let mut vec = vec![0.0f32; vocab_size];
        let mut tf: HashMap<usize, f32> = HashMap::new();

        for t in &tokens {
            if let Some(&term_idx) = self.vocabulary.get(t) {
                *tf.entry(term_idx).or_default() += 1.0;
            }
        }

        for (term_idx, count) in tf {
            vec[term_idx] = count * self.idf[term_idx];
        }

        let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        vec
    }

    pub fn search(
        &self,
        query: &str,
        issuer_filter: Option<&str>,
        limit: usize,
    ) -> Vec<(String, String, f32)> {
        let q_vec = self.embed_query(query);
        let mut scored: Vec<(String, String, f32)> = Vec::new();

        for (idx, doc_vec) in self.vectors.iter().enumerate() {
            if let Some(issuer) = issuer_filter {
                if !matches_issuer_filter(&self.issuers[idx], issuer) {
                    continue;
                }
            }

            let sim = cosine_similarity(&q_vec, doc_vec);
            if sim > 0.0 {
                scored.push((self.child_ids[idx].clone(), self.parent_ids[idx].clone(), sim));
            }
        }

        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }
}

/// Propuesta A — tercer ranker semántico: e5-large servido por un endpoint local
/// (`embedding_server.py`). Los vectores de los children se precomputan con
/// `compute_embeddings.py` a `data/embeddings.bin` (bincode: Vec<Vec<f32>>,
/// alineado por índice con `corpus.children`). L2-normalizados → coseno = dot.
pub struct EmbeddingSearcher {
    vectors: Vec<Vec<f32>>,
    child_ids: Vec<String>,
    parent_ids: Vec<String>,
    issuers: Vec<String>,
}

impl EmbeddingSearcher {
    pub fn build(children: &[ChildChunk], vectors: Vec<Vec<f32>>) -> Result<Self, String> {
        if vectors.len() != children.len() {
            return Err(format!(
                "EmbeddingSearcher: {} vectores para {} children (desalineados)",
                vectors.len(),
                children.len()
            ));
        }
        let mut dim = 0usize;
        for v in &vectors {
            if dim == 0 {
                dim = v.len();
            } else if v.len() != dim {
                return Err("EmbeddingSearcher: dimensiones inconsistentes entre vectores".into());
            }
        }
        let child_ids = children.iter().map(|c| c.id.clone()).collect();
        let parent_ids = children.iter().map(|c| c.parent_id.clone()).collect();
        let issuers = children.iter().map(|c| c.issuer.clone()).collect();
        Ok(Self {
            vectors,
            child_ids,
            parent_ids,
            issuers,
        })
    }

    pub fn load_from_bin(children: &[ChildChunk], path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("embeddings.bin: {e}"))?;
        let mut cursor = std::io::Cursor::new(&bytes);
        let vectors: Vec<Vec<f32>> =
            bincode::deserialize_from(&mut cursor).map_err(|e| format!("bincode embeddings: {e}"))?;
        Self::build(children, vectors)
    }

    pub fn dims(&self) -> usize {
        self.vectors.first().map(|v| v.len()).unwrap_or(0)
    }

    /// Coseno = producto punto (vectores L2-normalizados). Filtra por emisor.
    pub fn search(
        &self,
        query_vec: &[f32],
        issuer_filter: Option<&str>,
        limit: usize,
    ) -> Vec<(String, String, f32)> {
        let mut scored: Vec<(String, String, f32)> = Vec::new();
        for (idx, doc_vec) in self.vectors.iter().enumerate() {
            if let Some(issuer) = issuer_filter {
                if !matches_issuer_filter(&self.issuers[idx], issuer) {
                    continue;
                }
            }
            let sim = cosine_similarity(query_vec, doc_vec);
            if sim > 0.0 {
                scored.push((self.child_ids[idx].clone(), self.parent_ids[idx].clone(), sim));
            }
        }
        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }
}

/// Cliente HTTP hacia `embedding_server.py` (API OpenAI-ish local).
/// El servidor aplica el prefijo e5 ("query: ") y normaliza L2.
pub struct EmbeddingClient {
    http: reqwest::Client,
    url: String,
}

impl EmbeddingClient {
    pub fn new(url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            url,
        }
    }

    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        let body = serde_json::json!({"input": [text], "mode": "query"});
        let resp = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("embedding HTTP: {e}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("embedding JSON: {e}"))?;
        resp["data"][0]["embedding"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
            .ok_or_else(|| "embedding: respuesta sin data[0].embedding".to_string())
    }
}

pub struct HybridRetriever {
    pub corpus: Corpus,
    pub tantivy: TantivyIndexWrapper,
    pub vector_searcher: VectorSearcher,
    pub embeddings: Option<EmbeddingSearcher>,
    pub parent_map: HashMap<String, ParentChunk>,
}

impl HybridRetriever {
    pub fn build(
        corpus: Corpus,
        index_dir: Option<&Path>,
        embeddings_bin: Option<&Path>,
    ) -> Result<Self, String> {
        let tantivy = if let Some(dir) = index_dir {
            TantivyIndexWrapper::create_in_dir(dir, &corpus.children)?
        } else {
            TantivyIndexWrapper::create_in_ram(&corpus.children)?
        };

        let vector_searcher = VectorSearcher::build(&corpus.children);

        let embeddings = match embeddings_bin {
            Some(path) if path.exists() => {
                match EmbeddingSearcher::load_from_bin(&corpus.children, path) {
                    Ok(es) => {
                        println!("[INFO] Embeddings cargados: {} x {} dims", es.vectors.len(), es.dims());
                        Some(es)
                    }
                    Err(e) => {
                        println!("[WARN] Sin embeddings ({}); retrieval 2 vías", e);
                        None
                    }
                }
            }
            _ => None,
        };

        let mut parent_map = HashMap::new();
        for p in &corpus.parents {
            parent_map.insert(p.id.clone(), p.clone());
        }

        Ok(Self {
            corpus,
            tantivy,
            vector_searcher,
            embeddings,
            parent_map,
        })
    }

    pub fn retrieve_parents(
        &self,
        question: &str,
        issuer_filter: Option<&str>,
        top_k: usize,
        query_vec: Option<&[f32]>,
    ) -> Vec<RetrievedParent> {
        let mut all_bm25_child_ranks: Vec<Vec<String>> = Vec::new();
        let mut all_vec_child_ranks: Vec<Vec<String>> = Vec::new();
        let mut all_emb_child_ranks: Vec<Vec<String>> = Vec::new();

        // Propuesta C — multi-query por entidad: si el query menciona ≥2 emisores
        // conocidos y no hay filtro externo, se ejecuta una sub-consulta por emisor:
        // la sub-frase del query que lo menciona, filtrada a ese emisor, además de
        // la consulta global. RRF agrupa los rankings: cada tema del query
        // compuesto queda representado en el top-k.
        let issuers = detect_issuers_in_query(question);
        // Multi-query por entidad: con 1 o más emisores detectados (y sin filtro
        // externo), la sub-consulta se filtra al emisor. Sin esto, "inversion
        // futura" de Ferreycorp pisa a "microsoft" (término con tf bajo por chunk).
        let use_multi = issuer_filter.is_none() && !issuers.is_empty();
        let sub_queries: Vec<(String, Option<String>)> = if use_multi {
            // Solo sub-consultas por emisor: cada tema tiene su segmento y su
            // filtro. La consulta global diluye el ranking (pág. 100 de Ferreycorp
            // pisa al Banco Mundial por tokens de "ventas").
            segment_query_by_issuer(question, &issuers)
                .into_iter()
                .map(|(canon, seg)| (seg, Some(canon)))
                .collect()
        } else {
            vec![(question.to_string(), issuer_filter.map(|s| s.to_string()))]
        };

        // En modo multi, cada sub-consulta se fusiona internamente con RRF
        // estándar y se normaliza por su score máximo (→ [0,1]). Así cada tema
        // pesa igual: el mejor parent de cada sub-consulta obtiene 1.0, sin
        // importar cuántas variantes genere. BM pág. 1 (rank 1º en su tema)
        // compite en igualdad con el mejor de Ferreycorp y el de Efectiva.
        let mut sub_fused: Vec<Vec<(String, f64)>> = Vec::new();
        let rrf_k = 60.0;

        for (sub_q, filter) in &sub_queries {
            if use_multi {
                let mut lists: Vec<Vec<String>> = Vec::new();
                let variants = expand_financial_queries(sub_q);
                for variant in &variants {
                    if let Ok(bm25_res) = self.tantivy.search(variant, filter.as_deref(), 25) {
                        let ids: Vec<String> = bm25_res.into_iter().map(|(id, _, _)| id).collect();
                        if !ids.is_empty() {
                            lists.push(ids);
                        }
                    }
                    let vec_res = self.vector_searcher.search(variant, filter.as_deref(), 25);
                    let ids: Vec<String> = vec_res.into_iter().map(|(id, _, _)| id).collect();
                    if !ids.is_empty() {
                        lists.push(ids);
                    }
                }
                let fused = reciprocal_rank_fusion(&lists, rrf_k, 30);
                let max_score = fused.first().map(|(_, s)| *s).unwrap_or(1.0);
                let normalized: Vec<(String, f64)> = fused
                    .into_iter()
                    .map(|(id, sc)| (id, sc / max_score))
                    .collect();
                sub_fused.push(normalized);
            } else {
                let variants = expand_financial_queries(sub_q);
                for variant in &variants {
                    if let Ok(bm25_res) = self.tantivy.search(variant, filter.as_deref(), 25) {
                        let ids: Vec<String> = bm25_res.into_iter().map(|(id, _, _)| id).collect();
                        if !ids.is_empty() {
                            all_bm25_child_ranks.push(ids);
                        }
                    }

                    let vec_res = self.vector_searcher.search(variant, filter.as_deref(), 25);
                    let ids: Vec<String> = vec_res.into_iter().map(|(id, _, _)| id).collect();
                    if !ids.is_empty() {
                        all_vec_child_ranks.push(ids);
                    }
                }

                // Propuesta A: tercer ranker semántico (mismo query_vec, filtro distinto).
                if let (Some(query_vec), Some(emb_searcher)) = (query_vec, &self.embeddings) {
                    if query_vec.len() == emb_searcher.dims() {
                        let emb_res = emb_searcher.search(query_vec, filter.as_deref(), 25);
                        let ids: Vec<String> = emb_res.into_iter().map(|(id, _, _)| id).collect();
                        if !ids.is_empty() {
                            all_emb_child_ranks.push(ids);
                        }
                    }
                }
            }
        }

        let fused_children: Vec<(String, f64)> = if use_multi {
            // Suma de scores normalizados por sub-consulta (cada tema ≤ 1.0).
            let mut acc: HashMap<String, f64> = HashMap::new();
            for scores in &sub_fused {
                for (id, sc) in scores {
                    *acc.entry(id.clone()).or_insert(0.0) += sc;
                }
            }
            let mut v: Vec<(String, f64)> = acc.into_iter().collect();
            v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            v.truncate(30);
            v
        } else {
            let mut combined_ranks = all_bm25_child_ranks;
            combined_ranks.extend(all_vec_child_ranks);
            combined_ranks.extend(all_emb_child_ranks);
            reciprocal_rank_fusion(&combined_ranks, rrf_k, 30)
        };

        let mut parent_scores: HashMap<String, f64> = HashMap::new();
        let mut parent_matched_children: HashMap<String, Vec<String>> = HashMap::new();

        let child_to_parent: HashMap<String, String> = self.corpus.children.iter()
            .map(|c| (c.id.clone(), c.parent_id.clone()))
            .collect();

        for (child_id, score) in fused_children {
            if let Some(parent_id) = child_to_parent.get(&child_id) {
                // Máximo (no suma): evita el sesgo de longitud — un parent con
                // muchos chunks rankeados (pág. larga con menciones repetidas de
                // "ventas") no debe pisotear al parent cuyo mejor chunk es el que
                // responde el total (p.ej. pág. 38 "ventas récord US$ 2,177 M").
                let entry = parent_scores.entry(parent_id.clone()).or_insert(0.0);
                *entry = entry.max(score);
                parent_matched_children.entry(parent_id.clone()).or_default().push(child_id);
            }
        }

        let mut sorted_parents: Vec<(String, f64)> = parent_scores.into_iter().collect();
        sorted_parents.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted_parents.truncate(top_k);

        let mut retrieved = Vec::new();
        for (parent_id, score) in sorted_parents {
            if let Some(parent_chunk) = self.parent_map.get(&parent_id) {
                let matched_children = parent_matched_children.remove(&parent_id).unwrap_or_default();
                let citation = format!("{}_{}_{} (pág. {})", 
                    parent_chunk.issuer, 
                    parent_chunk.doc_type, 
                    parent_chunk.year, 
                    parent_chunk.page
                );
                retrieved.push(RetrievedParent {
                    parent: parent_chunk.clone(),
                    score,
                    matched_child_ids: matched_children,
                    citations: vec![citation],
                });
            }
        }

        retrieved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::DocumentPage;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 1.0];
        let b = vec![1.0, 0.0, 1.0];
        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-5);
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_reciprocal_rank_fusion() {
        let list1 = vec!["docA".to_string(), "docB".to_string(), "docC".to_string()];
        let list2 = vec!["docB".to_string(), "docA".to_string(), "docD".to_string()];
        let fused = reciprocal_rank_fusion(&[list1, list2], 60.0, 3);
        assert!(!fused.is_empty());
        assert!(fused[0].0 == "docA" || fused[0].0 == "docB");
    }

    #[test]
    fn test_issuer_filter_matching() {
        assert!(matches_issuer_filter("Financiera Efectiva", "Financiera"));
        assert!(matches_issuer_filter("Banco Mundial", "WorldBank"));
        assert!(matches_issuer_filter("Banco Mundial", "World Bank"));
        assert!(matches_issuer_filter("Ferreycorp", "Ferreycorp"));
    }

    #[test]
    fn test_normalize_text_accents() {
        // La causa raíz del caso P5: "dolares" (query) debe matchear "dólares" (doc).
        assert_eq!(normalize_text("DÓLARES"), "dolares");
        assert_eq!(normalize_text("dólares"), "dolares");
        assert_eq!(normalize_text("Dolares"), "dolares");
        assert_eq!(normalize_text("Inversión 2025"), "inversion 2025");
        assert_eq!(normalize_text("US$ 2,177 MILLONES"), "us$ 2,177 millones");
    }

    #[test]
    fn test_segment_query_by_issuer() {
        let q = "que crecimiento del PBI proyecto el Banco Mundial para Peru en 2024, que ventas totales reporto Ferreycorp en dolares en 2025 y que patrimonio supero Financiera Efectiva en 2025?";
        let issuers = detect_issuers_in_query(q);
        assert_eq!(issuers.len(), 3);
        let segments = segment_query_by_issuer(q, &issuers);
        // worldbank → primer segmento (PBI), ferreycorp → segmento de ventas.
        let wb = segments.iter().find(|(c, _)| c == "worldbank").unwrap();
        assert!(wb.1.to_lowercase().contains("pbi"), "segmento worldbank: {}", wb.1);
        let fc = segments.iter().find(|(c, _)| c == "ferreycorp").unwrap();
        assert!(fc.1.to_lowercase().contains("ventas"), "segmento ferreycorp: {}", fc.1);
        let fe = segments.iter().find(|(c, _)| c == "financieraefectiva").unwrap();
        assert!(fe.1.to_lowercase().contains("patrimonio"), "segmento efectiva: {}", fe.1);
        // Sin separadores → fallback al query completo.
        let single = segment_query_by_issuer("ventas de Ferreycorp", &["ferreycorp".to_string()]);
        assert_eq!(single[0].1, "ventas de Ferreycorp");
    }

    #[test]
    fn test_detect_issuers_in_query() {
        // P5 compuesto: 3 emisores.
        let q = "PBI del Banco Mundial, ventas de Ferreycorp y patrimonio de Financiera Efectiva";
        let found = detect_issuers_in_query(q);
        assert_eq!(found.len(), 3, "debe detectar 3 emisores: {found:?}");
        assert_eq!(found[0], "worldbank");
        assert_eq!(found[1], "ferreycorp");
        assert_eq!(found[2], "financieraefectiva");

        // Sin emisores → vacío (sin multi-query).
        assert!(detect_issuers_in_query("¿cuál fue el crecimiento del PBI en 2024?").is_empty());

        // Deduplicación por sinónimos.
        let d = detect_issuers_in_query("Ferreycorp y Ferreyros y la financiera efectiva");
        assert_eq!(d.len(), 2, "Ferreycorp/Ferreyros es 1 emisor: {d:?}");

        // Alias cortos.
        assert_eq!(detect_issuers_in_query("Banco Mundial (WorldBank)"), vec!["worldbank".to_string()]);
    }

    #[test]
    fn test_hybrid_retriever_in_ram() {        let page = DocumentPage {
            document: "Ferreycorp_Memoria_2025.md".to_string(),
            page: 39,
            text: "Ferreycorp entregó 32 camiones de minería en 2025 alcanzando un ratio deuda EBITDA de 1.4x.".to_string(),
            issuer: "Ferreycorp".to_string(),
            doc_type: "memoria".to_string(),
            year: "2025".to_string(),
            title: "Ferreycorp Memoria 2025".to_string(),
        };

        let (parents, children) = crate::ingest::process_page_into_chunks(&page, 2000, 200, 400, 50);
        let mut corpus = Corpus::new();
        corpus.parents = parents;
        corpus.children = children;

        let retriever = HybridRetriever::build(corpus, None, None).expect("Error building retriever");
        let results = retriever.retrieve_parents("¿Cuántos camiones entregó Ferreycorp en 2025?", Some("Ferreycorp"), 3, None);
        assert!(!results.is_empty());
        assert!(results[0].parent.content.contains("32 camiones"));
    }

    // Propuesta A: tercer ranker semántico.
    fn chunk(id: &str, parent_id: &str, issuer: &str, content: &str) -> ChildChunk {
        ChildChunk {
            id: id.to_string(),
            parent_id: parent_id.to_string(),
            document: "doc.pdf".to_string(),
            page: 1,
            issuer: issuer.to_string(),
            doc_type: "memoria".to_string(),
            year: "2025".to_string(),
            title: "t".to_string(),
            content: content.to_string(),
            is_table: false,
            indicator: None,
        }
    }

    fn norm(v: &[f32]) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    #[test]
    fn test_embedding_searcher_ranks_by_similarity() {
        let children = vec![
            chunk("c1", "p1", "Ferreycorp", "ventas de maquinaria minera"),
            chunk("c2", "p2", "Ferreycorp", "resultados financieros auditados"),
            chunk("c3", "p3", "Efectiva", "patrimonio de la financiera"),
        ];
        // Vectores 4-d artificiales: c1 ~ "ventas", c3 ~ "patrimonio".
        let vectors = vec![
            norm(&[0.9, 0.1, 0.0, 0.0]),
            norm(&[0.2, 0.8, 0.1, 0.0]),
            norm(&[0.0, 0.1, 0.9, 0.2]),
        ];
        let searcher = EmbeddingSearcher::build(&children, vectors).expect("build ok");
        assert_eq!(searcher.dims(), 4);

        // Query apunta a "ventas" → c1 primero.
        let q = norm(&[0.9, 0.0, 0.0, 0.1]);
        let res = searcher.search(&q, None, 3);
        assert_eq!(res[0].0, "c1", "el chunk de ventas debe rankear primero");
        assert_eq!(res.len(), 3);

        // Filtro por emisor: solo Efectiva.
        let res_f = searcher.search(&q, Some("Efectiva"), 3);
        assert_eq!(res_f.len(), 1);
        assert_eq!(res_f[0].0, "c3");
    }

    #[test]
    fn test_embedding_searcher_rejects_mismatch() {
        let children = vec![chunk("c1", "p1", "Ferreycorp", "texto")];
        let vectors = vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]]; // 2 vectores, 1 child
        assert!(EmbeddingSearcher::build(&children, vectors).is_err());

        let children2 = vec![chunk("c1", "p1", "Ferreycorp", "t"), chunk("c2", "p2", "Efectiva", "u")];
        let vectors2 = vec![vec![0.1, 0.2], vec![0.3, 0.4, 0.5]]; // dims inconsistentes
        assert!(EmbeddingSearcher::build(&children2, vectors2).is_err());
    }

    #[test]
    fn test_hybrid_retriever_embeddings_fallback_without_query_vec() {
        // Con embeddings cargados pero sin query_vec, el retrieval es idéntico
        // al de 2 vías (degradación limpia).
        let page = DocumentPage {
            document: "Ferreycorp_Memoria_2025.md".to_string(),
            page: 1,
            text: "Ferreycorp entregó 32 camiones de minería en 2025 alcanzando un ratio deuda EBITDA de 1.4x.".to_string(),
            issuer: "Ferreycorp".to_string(),
            doc_type: "memoria".to_string(),
            year: "2025".to_string(),
            title: "Ferreycorp Memoria 2025".to_string(),
        };
        let (parents, children) = crate::ingest::process_page_into_chunks(&page, 2000, 200, 400, 50);
        let mut corpus = Corpus::new();
        corpus.parents = parents;
        corpus.children = children;

        let vectors: Vec<Vec<f32>> = corpus.children.iter().map(|_| norm(&[1.0, 0.0])).collect();
        let es = EmbeddingSearcher::build(&corpus.children, vectors).expect("build ok");
        let tantivy = TantivyIndexWrapper::create_in_ram(&corpus.children).expect("tantivy ram");
        let vector_searcher = VectorSearcher::build(&corpus.children);
        let parent_map: HashMap<String, ParentChunk> = corpus
            .parents
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        let retriever = HybridRetriever {
            corpus,
            tantivy,
            vector_searcher,
            embeddings: Some(es),
            parent_map,
        };
        let results = retriever.retrieve_parents("¿Cuántos camiones entregó Ferreycorp en 2025?", Some("Ferreycorp"), 3, None);
        assert!(!results.is_empty());
        assert!(results[0].parent.content.contains("32 camiones"));
    }
}
