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

/// Segmenta el query compuesto en sub-frases, una por emisor.
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

/// B1 — detecta parents que son tabla de contenido / índice (sin cifras útiles).
/// En este corpus el índice es una tabla markdown (`||Contenido||`), no un `# Índice`.
/// Reglas: (a) marcador "contenido/índice/toc" en una línea de tabla o encabezado,
/// o (b) ≥60% de líneas son filas de tabla/encabezados; y (c) corto (<800 chars)
/// o casi sin cifras (<10 dígitos). "contenido" en prosa de una nota financiera
/// NO penaliza (se exige línea de tabla/encabezado).
pub fn is_toc_parent(content: &str) -> bool {
    let lineas: Vec<&str> = content.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lineas.len() < 3 {
        return false;
    }
    let marcadores = [
        "contenido", "indice", "índice", "indices", "tabla de contenido",
        "table of contents", "contents",
    ];
    let marcador = lineas.iter().any(|l| {
        (l.starts_with('|') || l.starts_with('#'))
            && marcadores.iter().any(|m| normalize_text(l).contains(m))
    });
    let filas_tabla = lineas.iter().filter(|l| l.starts_with('|') || l.starts_with('#')).count();
    let densidad = filas_tabla as f64 / lineas.len() as f64;
    let corto = content.chars().count() < 800;
    let sin_cifras = content.chars().filter(|c| c.is_ascii_digit()).count() < 10;
    // (a) marcador explícito en tabla/encabezado, o (b) densidad alta Y corto Y sin cifras.
    marcador || (densidad >= 0.6 && corto && sin_cifras)
}

/// Empaquetado — solape pregunta ∩ oración-con-cifra: cuántas oraciones del
/// contenido contienen UNA cifra Y un token de la pregunta. La carta ("Azure
/// surpassed $75 billion in revenue") puntúa 1 porque su oración-con-cifra
/// contiene "azure"; el bullet de Cloud ("Microsoft Cloud revenue... $168.9
/// billion") puntúa 0 porque "azure" está en otra oración. General: ataca el
/// patrón "parte vs todo" (una línea de negocio vs su agregado; una entidad vs el sistema).
pub fn score_solape(question_tokens: &[String], content: &str) -> usize {
    content
        .split(['.', '\n'])
        .filter(|o| {
            let tiene_cifra = o.chars().any(|c| c.is_ascii_digit());
            if !tiene_cifra {
                return false;
            }
            let tokens = tokenize_text(o);
            tokens.iter().any(|t| question_tokens.contains(t))
        })
        .count()
}

/// B — divide la pregunta en cláusulas (por ¿?, comas, puntos y la conjunción " y ").
pub fn split_clausulas(question: &str) -> Vec<String> {
    question
        .split([',', ';', '\n', '¿', '?', '.'])
        .flat_map(|s| s.split(" y "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A — ¿la cláusula es un preámbulo (solo nombra emisor + tipo de documento)?
/// "En el Annual Report 2025 de Microsoft," queda casi vacía al quitar el emisor
/// y las palabras de documento/año → preámbulo. "Según la carta del presidente de
/// Ferreycorp," conserva "presidente" → NO es preámbulo (la carta es la sección).
pub fn es_preambulo(clausula: &str, emisor: &str) -> bool {
    let sin_emisor = normalize_text(clausula).replace(&normalize_text(emisor), "");
    let tokens: Vec<String> = tokenize_text(&sin_emisor);
    const DOC_WORDS: &[&str] = &[
        "annual", "report", "memoria", "informe", "ficha", "outlook", "macro",
        "poverty", "anual", "2024", "2025", "2026", "2023", "2022", "2021",
        "en", "de", "el", "la", "los", "las", "del", "y", "que", "cual", "cuales",
    ];
    let significativos = tokens
        .iter()
        .filter(|t| !DOC_WORDS.contains(&t.as_str()))
        .count();
    significativos <= 1
}

/// C — anclas (nombres propios) de una cláusula: mayúscula inicial, no el primer
/// token, no el emisor, no tokens del título/archivo del documento (Annual, Report).
pub fn detectar_anclas(clausula: &str, emisor: &str, doc_tokens: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let emisor_norm = normalize_text(emisor);
    for (i, tok) in clausula.split_whitespace().enumerate() {
        if i == 0 {
            continue;
        }
        let first = tok.chars().next().unwrap_or(' ');
        if first.is_uppercase() {
            let t = normalize_text(tok.trim_matches(|c: char| !c.is_alphanumeric()));
            if !t.is_empty() && t != emisor_norm && !doc_tokens.contains(&t) && seen.insert(t.clone()) {
                out.push(t);
            }
        }
    }
    out
}

/// Detecta los emisores conocidos mencionados en el query.
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
        must: &[String],
        boost: &[String],
    ) -> Result<Vec<(String, String, f32)>, String> {
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.f_content_idx, self.f_indicator]);

        let tokens = tokenize_text(query_str);
        if tokens.is_empty() && must.is_empty() {
            return Ok(vec![]);
        }
        let mut safe_query = tokens.join(" ");
        // MUST (C): "+azure" — Tantivy lo trata como término obligatorio.
        for m in must {
            safe_query.push_str(&format!(" +{}", m));
        }
        // Boost suave en cláusulas hermanas: "azure^1.5".
        for b in boost {
            safe_query.push_str(&format!(" {}^1.5", b));
        }

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
    /// C1 — IDF del corpus para un token (ancla léxica). Sin diccionario:
    /// un token raro que el usuario ya escribió ("azure") recibe más peso.
    pub fn idf_of(&self, token: &str) -> Option<f32> {
        self.vocabulary.get(token).map(|&i| self.idf[i])
    }

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

/// Tercer ranker semántico: e5-large servido por un endpoint local
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

    /// C1 — repite los tokens de alta IDF del query (anclas léxicas que el
    /// usuario ya escribió: "azure", "caterpillar") para darles más peso en BM25
    /// y TF-IDF. General y sin diccionario: usa el IDF del corpus.
    fn boost_anchor_tokens(&self, query: &str) -> String {
        let tokens = tokenize_text(query);
        if tokens.is_empty() {
            return query.to_string();
        }
        let mut scored: Vec<(&String, f32)> = tokens
            .iter()
            .map(|t| (t, self.vector_searcher.idf_of(t).unwrap_or(0.0)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let anchors: HashSet<&String> = scored
            .iter()
            .take(3)
            .filter(|(_, idf)| *idf > 3.0)
            .map(|(t, _)| *t)
            .collect();
        if anchors.is_empty() {
            return query.to_string();
        }
        let mut out = String::new();
        for t in &tokens {
            if !out.is_empty() {
                out.push(' ');
            }
            if anchors.contains(t) {
                out.push_str(&format!("{} {} {}", t, t, t));
            } else {
                out.push_str(t);
            }
        }
        out
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

        // Multi-query por entidad: si el query menciona ≥2 emisores
        // conocidos y no hay filtro externo, se ejecuta una sub-consulta por emisor:
        // la sub-frase del query que lo menciona, filtrada a ese emisor, además de
        // la consulta global. RRF agrupa los rankings: cada tema del query
        // compuesto queda representado en el top-k.
        let issuers = detect_issuers_in_query(question);
        // Multi-query por entidad: con 1 o más emisores detectados (y sin filtro
        // externo), la sub-consulta se filtra al emisor. Sin esto, "inversion
        // futura" de Ferreycorp pisa a "microsoft" (término con tf bajo por chunk).
        let use_multi = issuer_filter.is_none() && !issuers.is_empty();

        // A+B+C: doc_tokens por emisor (tokens de título/archivo, para no marcar
        // "Annual"/"Report" como anclas), cláusulas métricas sin preámbulo, y
        // anclas (nombres propios) por cláusula: MUST en su cláusula, boost en
        // las hermanas del mismo emisor.
        let empty_doc: HashSet<String> = HashSet::new();
        let mut doc_tokens: HashMap<String, HashSet<String>> = HashMap::new();
        for iss in &issuers {
            let mut set: HashSet<String> = HashSet::new();
            for p in &self.corpus.parents {
                if p.issuer == *iss {
                    for t in tokenize_text(&p.document) {
                        set.insert(t);
                    }
                    for t in tokenize_text(&p.title) {
                        set.insert(t);
                    }
                }
            }
            doc_tokens.insert(iss.clone(), set);
        }

        let sub_queries: Vec<(String, Option<String>, Vec<String>, Vec<String>)> = if use_multi && issuers.len() == 1 {
            // A: con UN emisor, la sub-consulta NO es el fragmento del preámbulo
            // ("En el Annual Report 2025 de Microsoft,") sino las cláusulas
            // métricas ("ingresos anuales de Azure", "cantidad de modelos...").
            let iss = &issuers[0];
            let filtro = Some(iss.clone());
            let clausulas = split_clausulas(question);
            let metricas: Vec<String> = clausulas
                .iter()
                .filter(|c| !es_preambulo(c, iss))
                .cloned()
                .collect();
            if metricas.is_empty() {
                vec![(question.to_string(), filtro, Vec::new(), Vec::new())]
            } else {
                // Anclas de cada cláusula métrica (C).
                let anclas_por_clausula: Vec<Vec<String>> = metricas
                    .iter()
                    .map(|c| detectar_anclas(c, iss, doc_tokens.get(iss).unwrap_or(&empty_doc)))
                    .collect();
                // Boost: anclas de las cláusulas hermanas del mismo emisor.
                let todas: Vec<String> = anclas_por_clausula.iter().flatten().cloned().collect();
                metricas
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let must = anclas_por_clausula[i].clone();
                        let boost: Vec<String> = todas
                            .iter()
                            .filter(|a| !must.contains(a))
                            .cloned()
                            .collect();
                        (c, filtro.clone(), must, boost)
                    })
                    .collect()
            }
        } else if use_multi {
            // >=2 emisores: segmentación por emisor (la actual) + anclas.
            segment_query_by_issuer(question, &issuers)
                .into_iter()
                .map(|(canon, seg)| {
                    let must = detectar_anclas(&seg, &canon, doc_tokens.get(&canon).unwrap_or(&empty_doc));
                    (seg, Some(canon), must, Vec::new())
                })
                .collect()
        } else {
            vec![(question.to_string(), issuer_filter.map(|s| s.to_string()), Vec::new(), Vec::new())]
        };

        // En modo multi, cada sub-consulta se fusiona internamente con RRF
        // estándar y se normaliza por su score máximo (→ [0,1]). Así cada tema
        // pesa igual: el mejor parent de cada sub-consulta obtiene 1.0, sin
        // importar cuántas variantes genere. BM pág. 1 (rank 1º en su tema)
        // compite en igualdad con el mejor de Ferreycorp y el de Efectiva.
        let mut sub_fused: Vec<Vec<(String, f64)>> = Vec::new();
        let rrf_k = 60.0;
        let child_to_parent: HashMap<String, String> = self.corpus.children.iter()
            .map(|c| (c.id.clone(), c.parent_id.clone()))
            .collect();

        // C (por PÁGINA): el MUST se evalúa contra la página completa — si algún
        // child de la página tiene el ancla, TODOS sus siblings pasan el filtro
        // (si algún child de la página tiene el ancla,
        // todos sus siblings pasan el filtro; filtrar por child
        // descartaría el fragmento con la cifra antes de la rehidratación).
        let mut child_page: HashMap<String, (String, u32)> = HashMap::new();
        let mut page_text: HashMap<(String, u32), String> = HashMap::new();
        for c in &self.corpus.children {
            child_page.insert(c.id.clone(), (c.document.clone(), c.page));
            let e = page_text.entry((c.document.clone(), c.page)).or_default();
            e.push_str(&normalize_text(&c.content));
            e.push(' ');
        }
        let pasa_must_por_pagina = |id: &str, must: &[String]| -> bool {
            must.iter().all(|m| {
                child_page
                    .get(id)
                    .and_then(|k| page_text.get(k))
                    .map(|t| t.contains(m))
                    .unwrap_or(false)
            })
        };

        for (sub_q, filter, must, boost) in &sub_queries {
            if use_multi {
                let mut lists: Vec<Vec<String>> = Vec::new();
                let variants = expand_financial_queries(sub_q);
                for variant in &variants {
                    let bv = self.boost_anchor_tokens(variant);
                    // BM25 sin +must: el MUST se aplica por página post-filtro
                    // (el +must a nivel child se comería la pág. 3 del 11,000).
                    if let Ok(bm25_res) = self.tantivy.search(&bv, filter.as_deref(), 25, &[], boost) {
                        let ids: Vec<String> = bm25_res
                            .into_iter()
                            .filter(|(id, _, _)| pasa_must_por_pagina(id, must))
                            .map(|(id, _, _)| id)
                            .collect();
                        if !ids.is_empty() {
                            lists.push(ids);
                        }
                    }
                    // C: MUST también en TF-IDF (post-filtro por PÁGINA).
                    let vec_res = self.vector_searcher.search(&bv, filter.as_deref(), 25);
                    let ids: Vec<String> = vec_res
                        .into_iter()
                        .filter(|(id, _, _)| pasa_must_por_pagina(id, must))
                        .map(|(id, _, _)| id)
                        .collect();
                    if !ids.is_empty() {
                        lists.push(ids);
                    }
                }
                // Tercer ranker semántico también en el camino multi.
                // El query_vec ya se computa en el caller (async) y aquí se aplica
                // con el filtro de la sub-consulta (emisor). Conecta vocabulario
                // sin diccionario: "ingresos de Azure" ↔ "Azure revenue".
                if let (Some(query_vec), Some(emb_searcher)) = (query_vec, &self.embeddings) {
                    if query_vec.len() == emb_searcher.dims() {
                        let emb_res = emb_searcher.search(query_vec, filter.as_deref(), 25);
                        let ids: Vec<String> = emb_res
                            .into_iter()
                            .filter(|(id, _, _)| pasa_must_por_pagina(id, must))
                            .map(|(id, _, _)| id)
                            .collect();
                        if !ids.is_empty() {
                            lists.push(ids);
                        }
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
                    let bv = self.boost_anchor_tokens(variant);
                    if let Ok(bm25_res) = self.tantivy.search(&bv, filter.as_deref(), 25, &[], &[]) {
                        let ids: Vec<String> = bm25_res.into_iter().map(|(id, _, _)| id).collect();
                        if !ids.is_empty() {
                            all_bm25_child_ranks.push(ids);
                        }
                    }

                    let vec_res = self.vector_searcher.search(&bv, filter.as_deref(), 25);
                    let ids: Vec<String> = vec_res.into_iter().map(|(id, _, _)| id).collect();
                    if !ids.is_empty() {
                        all_vec_child_ranks.push(ids);
                    }
                }

                // Tercer ranker semántico (mismo query_vec, filtro distinto).
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

        // B1: penalizar parents TOC/índice (pág. 3 "Contenido" sin cifras útiles).
        let mut toc_cache: HashMap<String, bool> = HashMap::new();
        for (child_id, score) in fused_children {
            if let Some(parent_id) = child_to_parent.get(&child_id) {
                let mut s = score;
                let is_toc = *toc_cache.entry(parent_id.clone()).or_insert_with(|| {
                    self.corpus
                        .parents
                        .iter()
                        .find(|p| &p.id == parent_id)
                        .map(|p| is_toc_parent(&p.content))
                        .unwrap_or(false)
                });
                if is_toc {
                    s *= 0.1;
                }
                // Máximo (no suma): evita el sesgo de longitud — un parent con
                // muchos chunks rankeados (pág. larga con menciones repetidas de
                // "ventas") no debe pisotear al parent cuyo mejor chunk es el que
                // responde el total (p.ej. pág. 38 "ventas récord US$ 2,177 M").
                let entry = parent_scores.entry(parent_id.clone()).or_insert(0.0);
                *entry = entry.max(s);
                parent_matched_children.entry(parent_id.clone()).or_default().push(child_id);
            }
        }

        // Preferencia por cobertura numérica (general, sin diccionario): entre las
        // páginas que pasan el MUST, las que juntan el ancla del query con evidencia
        // de dinero/conteo (billion, millones, \d{4,}) son más probables de tener LA
        // cifra pedida — la carta del $75B gana al párrafo genérico de Azure. Se
        // aplica ANTES de la precedencia de siblings para que la ventana ±3 salga del
        // top-1 correcto.
        let parent_info: HashMap<String, (String, u32, String)> = self.corpus.parents.iter()
            .map(|p| (p.id.clone(), (p.document.clone(), p.page, p.issuer.clone())))
            .collect();
        let todas_anclas: Vec<&String> = sub_queries
            .iter()
            .flat_map(|(_, _, m, _)| m.iter())
            .collect();
        let pat_dinero = regex::Regex::new(r"billion|million|millones|millon|mil\b|us\$|s/|\d{4,}").unwrap();
        for (pid, score) in parent_scores.iter_mut() {
            if let Some((doc, page, _)) = parent_info.get(pid) {
                let texto = page_text.get(&(doc.clone(), *page)).map(|t| t.as_str()).unwrap_or("");
                let tiene_ancla = todas_anclas.iter().any(|a| texto.contains(a.as_str()));
                let tiene_dinero = pat_dinero.is_match(texto);
                if tiene_ancla && tiene_dinero {
                    *score *= 1.3;
                }
            }
        }

        let mut sorted_parents: Vec<(String, f64)> = parent_scores.into_iter().collect();
        sorted_parents.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Ventana de sección SOLO sobre el mejor hit por emisor + rehidratación
        // de página (la pág. 5 partida en 2 parents cuenta como UNA página; así la
        // carta completa 5-8 entra sin gastar 2 slots en la misma página).

        // 1) Siblings: los parents de la misma (doc, page) entran con el score del ancla.
        let mut con_siblings: Vec<(String, f64)> = Vec::new();
        for (pid, score) in &sorted_parents {
            if let Some((doc, page, _)) = parent_info.get(pid) {
                for cand in &self.corpus.parents {
                    if cand.document == *doc
                        && cand.page == *page
                        && !con_siblings.iter().any(|(i, _)| i == &cand.id)
                    {
                        con_siblings.push((cand.id.clone(), *score));
                    }
                }
            }
        }
        con_siblings.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // 2) Mejor hit por emisor (primer parent rankeado de cada emisor).
        let mut mejor_por_emisor: Vec<(String, f64)> = Vec::new();
        let mut visto: HashSet<String> = HashSet::new();
        for (pid, score) in &con_siblings {
            if let Some((_, _, iss)) = parent_info.get(pid) {
                if visto.insert(iss.clone()) {
                    mejor_por_emisor.push((pid.clone(), *score));
                }
            }
        }

        // 3) Vecinos ±3 del MEJOR hit por emisor, CAPADOS a 2 por ancla: la ventana
        // no puede monopolizar el top_k (los vecinos del top-1 ocupaban casi todos los slots y las págs\.
        // 3 y 5 del fused nunca competían).
        let mut vecinos: Vec<(String, f64)> = Vec::new();
        for (pid, score) in &mejor_por_emisor {
            if let Some((doc, page, iss)) = parent_info.get(pid) {
                let mut candidatos: Vec<(String, f64)> = Vec::new();
                for delta in 1..=3i32 {
                    for cand in &self.corpus.parents {
                        if cand.document == *doc
                            && cand.issuer == *iss
                            && (cand.page as i32 - *page as i32).abs() == delta
                            && !con_siblings.iter().any(|(i, _)| i == &cand.id)
                            && !candidatos.iter().any(|(i, _)| i == &cand.id)
                        {
                            candidatos.push((cand.id.clone(), score * (1.0 - 0.1 * delta as f64)));
                        }
                    }
                }
                candidatos.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for v in candidatos.into_iter().take(2) {
                    vecinos.push(v);
                }
            }
        }
        vecinos.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // 4) Otros rankeados del fused (no top-1 de su emisor, no vecinos) — van
        // ANTES que los vecinos: los parents del fused MUST compiten por slot.
        let otros: Vec<(String, f64)> = con_siblings
            .iter()
            .filter(|(i, _)| !mejor_por_emisor.iter().any(|(t, _)| t == i) && !vecinos.iter().any(|(v, _)| v == i))
            .cloned()
            .collect();

        // 5) Orden final: top-1 por emisor → OTROS fused (por score) → vecinos capados.
        let mut final_list: Vec<(String, f64)> = Vec::new();
        final_list.extend(mejor_por_emisor);
        final_list.extend(otros);
        final_list.extend(vecinos);

        // Rehidratación: agrupar por (documento, página) → un bloque por página.
        let mut por_pagina: HashMap<(String, u32), (f64, Vec<String>)> = HashMap::new();
        let mut orden_paginas: Vec<(String, u32)> = Vec::new();
        for (pid, score) in &final_list {
            if let Some((doc, page, _)) = parent_info.get(pid) {
                let key = (doc.clone(), *page);
                let entry = por_pagina.entry(key.clone()).or_insert_with(|| {
                    orden_paginas.push(key.clone());
                    (0.0, Vec::new())
                });
                entry.0 = entry.0.max(*score);
                entry.1.push(pid.clone());
            }
        }
        let mut bloques: Vec<(String, u32, f64)> = orden_paginas
            .into_iter()
            .map(|k| {
                let (s, _) = por_pagina.get(&k).unwrap();
                (k.0.clone(), k.1, *s)
            })
            .collect();
        // Mantener el orden de final_list (top-1 por emisor → vecinos → otros):
        // reordenar por score puro devolvería parents de baja relevancia por delante de los
        // vecinos de sección (0.50-0.64) y las págs. 7-8 nunca entrarían.
        // Presentación: ordenar los bloques por SOLAPE pregunta ∩ oración-con-cifra
        // (la carta con "Azure... $75 billion" sube; el bullet de Cloud baja), para
        // que el LLM lea primero la página cuyo dato coincide con lo pedido.
        let q_tokens = tokenize_text(question);
        bloques.sort_by(|a, b| {
            let sa = score_solape(&q_tokens, por_pagina.get(&(a.0.clone(), a.1)).map(|(_, p)| p.join("\n")).unwrap_or_default().as_str());
            let sb = score_solape(&q_tokens, por_pagina.get(&(b.0.clone(), b.1)).map(|(_, p)| p.join("\n")).unwrap_or_default().as_str());
            sb.cmp(&sa).then_with(|| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
        });
        bloques.truncate(top_k);

        let mut retrieved = Vec::new();
        for (doc, page, score) in bloques {
            let pids = &por_pagina[&(doc.clone(), page)].1;
            let mut content = String::new();
            let mut issuer = String::new();
            let mut doc_type = String::new();
            let mut year = String::new();
            let mut title = String::new();
            let mut child_ids = Vec::new();
            let mut matched_children = Vec::new();
            for pid in pids {
                if let Some(pc) = self.parent_map.get(pid) {
                    if !content.is_empty() {
                        content.push_str("\n\n");
                    }
                    content.push_str(&pc.content);
                    issuer = pc.issuer.clone();
                    doc_type = pc.doc_type.clone();
                    year = pc.year.clone();
                    title = pc.title.clone();
                    child_ids.extend(pc.child_ids.iter().cloned());
                    if let Some(mc) = parent_matched_children.get(pid) {
                        matched_children.extend(mc.iter().cloned());
                    }
                }
            }
            let bloque_parent = ParentChunk {
                id: format!("{}_p{}", doc, page),
                document: doc.clone(),
                page,
                issuer: issuer.clone(),
                doc_type: doc_type.clone(),
                year: year.clone(),
                title: title.clone(),
                content,
                child_ids,
            };
            let citation = format!("{}_{}_{} (pág. {})", issuer, doc_type, year, page);
            retrieved.push(RetrievedParent {
                parent: bloque_parent,
                score,
                matched_child_ids: matched_children,
                citations: vec![citation],
            });
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
        // Normaliza acentos: "dolares" == "dólares".
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
    fn test_embeddings_ranker_acts_in_multi_mode() {
        // Modo multi (2 emisores): el tercer ranker semántico DEBE ejecutarse.
        // BM25 tiende a elegir "ventas netas" (parent B) sobre "ventas en dólares"
        // (parent A); el query_vec apunta a A y debe elevarlo en la sub-consulta.
        let pages = vec![
            DocumentPage {
                document: "Ferreycorp_2025.md".to_string(),
                page: 1,
                text: "las ventas en dólares de la corporación fueron US$ 2,177 millones en 2025".to_string(),
                issuer: "Ferreycorp".to_string(),
                doc_type: "memoria".to_string(),
                year: "2025".to_string(),
                title: "Memoria Ferreycorp".to_string(),
            },
            DocumentPage {
                document: "Ferreycorp_2025.md".to_string(),
                page: 2,
                text: "las ventas netas ascendieron a S/. 7,798.3 millones en soles en 2025".to_string(),
                issuer: "Ferreycorp".to_string(),
                doc_type: "memoria".to_string(),
                year: "2025".to_string(),
                title: "Memoria Ferreycorp".to_string(),
            },
            DocumentPage {
                document: "Efectiva_2025.md".to_string(),
                page: 3,
                text: "el patrimonio superó S/ 405 millones en 2025".to_string(),
                issuer: "Financiera Efectiva".to_string(),
                doc_type: "memoria".to_string(),
                year: "2025".to_string(),
                title: "Memoria Efectiva".to_string(),
            },
        ];
        let mut all_parents = Vec::new();
        let mut all_children = Vec::new();
        for p in &pages {
            let (ps, cs) = crate::ingest::process_page_into_chunks(p, 2000, 200, 400, 50);
            all_parents.extend(ps);
            all_children.extend(cs);
        }
        let vectors: Vec<Vec<f32>> = all_children
            .iter()
            .map(|c| {
                if c.content.contains("dólares") || c.content.contains("dolares") {
                    norm(&[0.95, 0.05])
                } else if c.content.contains("netas") {
                    norm(&[0.05, 0.95])
                } else {
                    norm(&[0.50, 0.50])
                }
            })
            .collect();
        let es = EmbeddingSearcher::build(&all_children, vectors).expect("searcher ok");
        let tantivy = TantivyIndexWrapper::create_in_ram(&all_children).expect("tantivy");
        let vector_searcher = VectorSearcher::build(&all_children);
        let parent_map: HashMap<String, ParentChunk> = all_parents
            .iter()
            .map(|p| (p.id.clone(), p.clone()))
            .collect();
        let corpus = Corpus {
            parents: all_parents,
            children: all_children,
            number_index: Default::default(),
            file_hashes: Default::default(),
            manifest_updated: "2026-08-19".to_string(),
        };
        let retriever = HybridRetriever {
            corpus,
            tantivy,
            vector_searcher,
            embeddings: Some(es),
            parent_map,
        };

        let q = "que ventas reporto Ferreycorp en dolares 2025 y que patrimonio supero Financiera Efectiva";
        assert_eq!(detect_issuers_in_query(q).len(), 2, "modo multi activo");

        // Con query_vec: el ranker semántico corre en modo multi y eleva A.
        let qv = norm(&[0.9, 0.1]);
        let con_vec = retriever.retrieve_parents(q, None, 3, Some(qv.as_slice()));
        assert!(!con_vec.is_empty());

        let pos_2177 = con_vec.iter().position(|r| r.parent.content.contains("2,177"));
        let pos_7798 = con_vec.iter().position(|r| r.parent.content.contains("7,798"));
        assert!(
            pos_2177.is_some() && (pos_7798.is_none() || pos_2177 < pos_7798),
            "con embeddings en modo multi, 2177 debe rankear antes que 7798: {con_vec:?}"
        );
    }

    #[test]
    fn test_is_toc_parent() {
        // La pág. 3 real del corpus: tabla ||Contenido|| (~662 chars).
        let toc = "||Contenido||\n|---|---|\n|1. Carta del Presidente|5|\n|2. NEGOCIO|6|\n|3. Gestión Comercial|38|";
        assert!(is_toc_parent(toc), "tabla Contenido debe detectarse como TOC");
        // Un encabezado de índice corto también.
        assert!(is_toc_parent("# Índice\n\n1. Portada\n2. Resumen"));
        // Una nota financiera con "contenido" en prosa NO debe penalizar.
        let nota = "El contenido de los estados financieros auditados incluye el balance general y el estado de resultados. La utilidad neta alcanzó los S/ 481 millones, con un margen bruto de 23.0% y un ratio de mora de 3.3%.";
        assert!(!is_toc_parent(nota), "prosa con cifras no es TOC");
        // Prosa larga con tabla de cifras tampoco.
        let tabla = "|Ventas Netas|7,798.3|100.0|\n|Total|7,798.3|100.0%|\n\nLas ventas netas en el 2025 ascendieron a S/ 7,798.3 millones.";
        assert!(!is_toc_parent(tabla), "tabla financiera con cifras no es TOC");
    }

    #[test]
    fn test_sibling_same_page_included() {
        // Página partida en 2 parents (el split corta a ~2000 chars): el parent
        // con la cifra (sibling de la misma página) debe entrar al top aunque el
        // ranking prefiera el primer fragmento.
        let page = DocumentPage {
            document: "Ferreycorp_2025.md".to_string(),
            page: 5,
            // Texto largo para forzar 2 parents (≈3000 chars).
            text: format!(
                "{}US$ 2,177 millones en 2025.",
                "La carta del presidente y el contexto macroeconómico del Perú en 2025: crecimiento de 3.3%, inversión de 1.5% del PBI, BCR en 4.25%. ".repeat(40)
            ),
            issuer: "Ferreycorp".to_string(),
            doc_type: "memoria".to_string(),
            year: "2025".to_string(),
            title: "Memoria Ferreycorp".to_string(),
        };
        let (parents, children) = crate::ingest::process_page_into_chunks(&page, 2000, 200, 400, 50);
        let pids: Vec<String> = parents.iter().map(|p| p.id.clone()).collect();
        assert!(pids.len() >= 2, "la página larga debe partirse en ≥2 parents");

        let corpus = Corpus {
            parents,
            children,
            number_index: Default::default(),
            file_hashes: Default::default(),
            manifest_updated: "2026-08-19".to_string(),
        };
        let retriever = HybridRetriever::build(corpus, None, None).expect("build ok");
        let results = retriever.retrieve_parents("ventas en dolares de la carta del presidente 2025", None, 4, None);
        assert!(
            results.iter().any(|r| r.parent.content.contains("2,177")),
            "el sibling con la cifra debe entrar al top: {:?}",
            results.iter().map(|r| &r.parent.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_clausulas_y_anclas() {
        // A+B+C: el preámbulo "En el Annual Report 2025 de Microsoft," se descarta;
        // quedan 2 cláusulas métricas; solo "Azure" es ancla (no Annual/Report/Microsoft).
        let q = "En el Annual Report 2025 de Microsoft, ¿qué cifras menciona sobre los ingresos anuales de Azure y la cantidad de modelos disponibles en su plataforma?";
        let issuers = detect_issuers_in_query(q);
        assert_eq!(issuers, vec!["microsoft".to_string()]);

        let clausulas = split_clausulas(q);
        let metricas: Vec<String> = clausulas
            .iter()
            .filter(|c| !es_preambulo(c, &issuers[0]))
            .cloned()
            .collect();
        assert_eq!(metricas.len(), 2, "dos cláusulas métricas: {metricas:?}");
        assert!(metricas[0].to_lowercase().contains("azure"), "cláusula 1: {:?}", metricas[0]);
        assert!(metricas[1].to_lowercase().contains("modelos"), "cláusula 2: {:?}", metricas[1]);

        let doc_tokens: HashSet<String> = ["annual".into(), "report".into(), "2025".into()]
            .into_iter()
            .collect();
        let anclas = detectar_anclas(&metricas[0], &issuers[0], &doc_tokens);
        assert_eq!(anclas, vec!["azure".to_string()], "solo Azure es ancla: {anclas:?}");
        let anclas2 = detectar_anclas(&metricas[1], &issuers[0], &doc_tokens);
        assert!(anclas2.is_empty(), "la cláusula de modelos no tiene anclas: {anclas2:?}");

        // "Según la carta del presidente de Ferreycorp," no es preámbulo: la carta es la sección pedida.
        let q2 = "Según la carta del presidente de Ferreycorp, ¿a cuánto ascendieron las ventas en dólares en 2025?";
        let c2 = split_clausulas(q2);
        assert!(
            c2.iter().any(|c| !es_preambulo(c, "ferreycorp")),
            "la cláusula de la carta no debe descartarse: {c2:?}"
        );
    }

    #[test]
    fn test_score_solape() {
        let q = "qué cifras menciona sobre los ingresos anuales de Azure y la cantidad de modelos disponibles en su plataforma";
        let qt = tokenize_text(q);
        // La carta: la oración-con-cifra contiene "azure" → solape 1.
        let carta = "Azure surpassed $75 billion in revenue for the first time. We are proud of this milestone.";
        assert_eq!(score_solape(&qt, carta), 1, "la oración de Azure con cifra debe puntuar");
        // El bullet de Cloud: la cifra está en la oración de Cloud; "azure" en otra → 0.
        let cloud = "Microsoft Cloud revenue increased 23% to $168.9 billion. Microsoft Cloud, which includes Azure and other services, grew.";
        assert_eq!(score_solape(&qt, cloud), 0, "el agregado sin azure en su oración-con-cifra no puntúa");
    }

    #[test]
    fn test_detect_issuers_in_query() {
        // Query compuesto: 3 emisores.
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

    // Tercer ranker semántico.
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
