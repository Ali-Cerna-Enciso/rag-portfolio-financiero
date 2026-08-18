use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::LazyLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::indicators::match_indicator_name;

static NUMBER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \b(?:
            \d{1,3}(?:[.,'\s\u{2019}]\d{3})+(?:[.,]\d+)? | # Formatos de miles: 1,000.50 o 1.000,50 o 1 000 o 1'938,305
            \d+(?:[.,]\d+)?                               # Números simples o decimales: 59968 o 32.5 o 0,85
        )(?:%|[xXmMkK]|MM|mm)?\b"
    ).unwrap()
});

static RAG_PAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<!--\s*(?:rag:)?page\s+(\d+)\s*-->").unwrap()
});

static PIPE_ROW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*\|.*\|\s*$").unwrap()
});

static FILENAME_META_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Za-z0-9]+)_(.+)_(\d{4})\.(?:pdf|md)$").unwrap()
});
static TABLE_MERGED_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\|\s*(?P<indicator>.+?)\s+(?P<section>[A-ZÁÉÍÓÚÑ]{4,}(?:\s+(?:Y|E|DE|DEL|EN|[A-ZÁÉÍÓÚÑ]{2,}))*)\s*\|\s*(?P<rest>[\d.,\s\-|]+)$")
        .expect("Error al compilar regex de sanitización de tablas")
});

static STANDALONE_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\|\s*([A-ZÁÉÍÓÚÑ]{4,}(?:\s+(?:Y|E|DE|DEL|EN|[A-ZÁÉÍÓÚÑ]{2,}))*)\s*\|\s*\|\s*\|$")
        .expect("Error al compilar regex de encabezados solitarios")
});

pub fn sanitize_markdown_tables(input: &str) -> String {
    let step1 = TABLE_MERGED_HEADER_RE.replace_all(input, |caps: &regex::Captures| {
        let indicator = caps.name("indicator").map_or("", |m| m.as_str()).trim();
        let section = caps.name("section").map_or("", |m| m.as_str()).trim();
        let rest = caps.name("rest").map_or("", |m| m.as_str()).trim();
        format!("| **{}** |||\n| {} | {}", section, indicator, rest)
    });
    let step2 = STANDALONE_HEADER_RE.replace_all(&step1, "| **$1** |||");
    step2.into_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLocation {
    pub document: String,
    pub page: u32,
    pub section_snippet: String,
}

pub type InvertedNumberIndex = HashMap<String, Vec<SourceLocation>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentMetadata {
    pub issuer: String,
    pub doc_type: String,
    pub year: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentPage {
    pub document: String,
    pub page: u32,
    pub text: String,
    pub issuer: String,
    pub doc_type: String,
    pub year: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentChunk {
    pub id: String,
    pub document: String,
    pub page: u32,
    pub issuer: String,
    pub doc_type: String,
    pub year: String,
    pub title: String,
    pub content: String,
    pub child_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildChunk {
    pub id: String,
    pub parent_id: String,
    pub document: String,
    pub page: u32,
    pub issuer: String,
    pub doc_type: String,
    pub year: String,
    pub title: String,
    pub content: String,
    pub is_table: bool,
    pub indicator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Corpus {
    pub parents: Vec<ParentChunk>,
    pub children: Vec<ChildChunk>,
    pub number_index: InvertedNumberIndex,
    pub file_hashes: HashMap<String, String>,
    pub manifest_updated: String,
}

impl Corpus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn save_to_binary<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded: Vec<u8> = bincode::serialize(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        fs::write(path, encoded)
    }

    pub fn load_from_binary<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let bytes = fs::read(path)?;
        let corpus: Self = bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        Ok(corpus)
    }

    pub fn save_to_json<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json_str = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        fs::write(path, json_str)
    }

    pub fn load_from_json<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let corpus: Self = serde_json::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        Ok(corpus)
    }
}

pub fn normalize_number(token: &str) -> Option<String> {
    let clean = token
        .replace(',', "")
        .replace('.', "")
        .replace(' ', "")
        .replace('%', "")
        .replace('$', "")
        .replace("S/", "")
        .replace('\u{2019}', "")
        .replace('\'', "")
        .replace('x', "")
        .replace('X', "")
        .replace('m', "")
        .replace('M', "")
        .replace('k', "")
        .replace('K', "")
        .replace('b', "")
        .replace('B', "")
        .trim()
        .to_string();

    if !clean.is_empty() && clean.chars().all(|c| c.is_ascii_digit()) {
        Some(clean)
    } else {
        None
    }
}

pub fn extract_and_normalize_numbers(text: &str) -> HashSet<String> {
    let mut normalized = HashSet::new();
    for mat in NUMBER_RE.find_iter(text) {
        if let Some(num) = normalize_number(mat.as_str()) {
            normalized.insert(num);
        }
    }
    normalized
}

pub fn calculate_sha256<P: AsRef<Path>>(path: P) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn parse_metadata_from_filename(filename: &str) -> DocumentMetadata {
    if let Some(caps) = FILENAME_META_RE.captures(filename) {
        let issuer = caps.get(1).map_or("", |m| m.as_str()).to_string();
        let doc_type = caps.get(2).map_or("", |m| m.as_str()).replace('_', " ");
        let year = caps.get(3).map_or("", |m| m.as_str()).to_string();
        let title = format!("{} {} {}", issuer, doc_type, year);
        return DocumentMetadata {
            issuer,
            doc_type,
            year,
            title,
        };
    }

    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    
    let parts: Vec<&str> = stem.split(&['_', '-'][..]).collect();
    let issuer = parts.first().unwrap_or(&"General").to_string();
    let year = parts.iter()
        .find(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(&"2025")
        .to_string();

    DocumentMetadata {
        issuer: issuer.clone(),
        doc_type: "memoria".to_string(),
        year,
        title: stem.replace('_', " "),
    }
}

pub fn load_document_catalog<P: AsRef<Path>>(path: P) -> HashMap<String, DocumentMetadata> {
    let path = path.as_ref();
    if !path.exists() {
        return HashMap::new();
    }
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(raw_map) = serde_json::from_str::<HashMap<String, HashMap<String, serde_json::Value>>>(&content) {
            let mut out = HashMap::new();
            for (fname, fields) in raw_map {
                let issuer = fields.get("issuer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let doc_type = fields.get("doc_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let year = fields.get("year").and_then(|v| {
                    v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string()))
                }).unwrap_or_default();
                let title = fields.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                out.insert(fname, DocumentMetadata { issuer, doc_type, year, title });
            }
            return out;
        }
    }
    HashMap::new()
}

pub fn extract_text_from_pdf<P: AsRef<Path>>(pdf_path: P) -> Result<Vec<(u32, String)>, String> {
    let doc = lopdf::Document::load(pdf_path.as_ref())
        .map_err(|e| format!("Error abriendo PDF con lopdf: {}", e))?;
    
    let mut pages_out = Vec::new();
    let pages = doc.get_pages();
    let mut page_numbers: Vec<u32> = pages.keys().cloned().collect();
    page_numbers.sort();

    for (idx, &page_id) in page_numbers.iter().enumerate() {
        let page_num = (idx + 1) as u32;
        if let Ok(text) = doc.extract_text(&[page_id]) {
            let clean_text = text.trim().to_string();
            if !clean_text.is_empty() {
                pages_out.push((page_num, clean_text));
            }
        }
    }

    Ok(pages_out)
}

pub fn parse_markdown_pages(md_content: &str) -> Vec<(u32, String)> {
    let sanitized = sanitize_markdown_tables(md_content);
    let mut pages = Vec::new();
    let matches: Vec<_> = RAG_PAGE_RE.find_iter(&sanitized).collect();

    if matches.is_empty() {
        let clean = sanitized.trim();
        if !clean.is_empty() {
            pages.push((1, clean.to_string()));
        }
        return pages;
    }
    for (i, m) in matches.iter().enumerate() {
        let cap = RAG_PAGE_RE.captures(m.as_str()).unwrap();
        let page_num: u32 = cap.get(1).unwrap().as_str().parse().unwrap_or((i + 1) as u32);
        let start = m.end();
        let end = if i + 1 < matches.len() {
            matches[i + 1].start()
        } else {
            md_content.len()
        };
        let page_text = md_content[start..end].trim().to_string();
        if !page_text.is_empty() {
            pages.push((page_num, page_text));
        }
    }

    pages
}

pub fn load_document_pages<P: AsRef<Path>>(
    file_path: P,
    catalog: &HashMap<String, DocumentMetadata>,
) -> Result<Vec<DocumentPage>, String> {
    let path = file_path.as_ref();
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
    
    let md_path = if path.extension().and_then(|s| s.to_str()) == Some("pdf") {
        let parent = path.parent().unwrap_or(Path::new(""));
        let grand_parent = parent.parent().unwrap_or(Path::new(""));
        let candidate_md = grand_parent.join("markdown").join(format!("{}.md", stem));
        if candidate_md.exists() {
            Some(candidate_md)
        } else {
            None
        }
    } else {
        None
    };

    let raw_pages = if let Some(md_p) = md_path {
        let content = fs::read_to_string(&md_p).map_err(|e| e.to_string())?;
        parse_markdown_pages(&content)
    } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        parse_markdown_pages(&content)
    } else {
        extract_text_from_pdf(path)?
    };

    let meta = catalog.get(file_name)
        .or_else(|| catalog.get(&format!("{}.pdf", stem)))
        .or_else(|| catalog.get(&format!("{}.md", stem)))
        .cloned()
        .unwrap_or_else(|| parse_metadata_from_filename(file_name));

    let doc_pages: Vec<DocumentPage> = raw_pages
        .into_iter()
        .map(|(page, text)| DocumentPage {
            document: file_name.to_string(),
            page,
            text,
            issuer: meta.issuer.clone(),
            doc_type: meta.doc_type.clone(),
            year: meta.year.clone(),
            title: meta.title.clone(),
        })
        .collect();

    Ok(doc_pages)
}

pub fn split_text_recursive(text: &str, chunk_size: usize, chunk_overlap: usize) -> Vec<String> {
    if text.len() <= chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    while start < len {
        let end = (start + chunk_size).min(len);
        let mut actual_end = end;

        if end < len {
            let window: String = chars[start..end].iter().collect();
            if let Some(pos) = window.rfind("\n\n") {
                actual_end = start + pos + 2;
            } else if let Some(pos) = window.rfind('\n') {
                actual_end = start + pos + 1;
            } else if let Some(pos) = window.rfind(". ") {
                actual_end = start + pos + 2;
            } else if let Some(pos) = window.rfind(' ') {
                actual_end = start + pos + 1;
            }
        }

        let chunk: String = chars[start..actual_end].iter().collect();
        let chunk_trimmed = chunk.trim().to_string();
        if !chunk_trimmed.is_empty() {
            chunks.push(chunk_trimmed);
        }

        if actual_end >= len {
            break;
        }

        let step = if actual_end > start + chunk_overlap {
            actual_end - chunk_overlap
        } else {
            actual_end
        };
        start = step.max(start + 1);
    }

    chunks
}

pub fn split_table_rows(table_md: &str, context_prefix: &str) -> Vec<(String, Option<String>)> {
    let lines: Vec<&str> = table_md.lines().collect();
    let mut header_lines = Vec::new();
    let mut data_rows = Vec::new();
    let mut seen_sep = false;

    for line in lines {
        let trimmed = line.trim();
        if !PIPE_ROW_RE.is_match(trimmed) {
            continue;
        }
        if !seen_sep {
            if trimmed.contains("---") {
                seen_sep = true;
            } else {
                header_lines.push(trimmed);
            }
        } else {
            data_rows.push(trimmed);
        }
    }

    if data_rows.is_empty() {
        return vec![(table_md.to_string(), None)];
    }

    let header_context = header_lines.join("\n");
    let mut out = Vec::new();

    for row in data_rows {
        let cells: Vec<&str> = row.split('|')
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .collect();
        
        let label = cells.first().unwrap_or(&"");
        let indicator = match_indicator_name(label);

        let mut row_chunk = String::new();
        if !context_prefix.is_empty() {
            row_chunk.push_str(context_prefix);
            row_chunk.push('\n');
        }
        if !header_context.is_empty() {
            row_chunk.push_str(&header_context);
            row_chunk.push('\n');
            row_chunk.push_str("|---|---|---|\n");
        }
        row_chunk.push_str(row);

        out.push((row_chunk, indicator));
    }

    out
}

pub fn process_page_into_chunks(
    page: &DocumentPage,
    parent_chunk_size: usize,
    parent_chunk_overlap: usize,
    child_chunk_size: usize,
    child_chunk_overlap: usize,
) -> (Vec<ParentChunk>, Vec<ChildChunk>) {
    let parent_texts = split_text_recursive(&page.text, parent_chunk_size, parent_chunk_overlap);
    let mut parents = Vec::new();
    let mut children = Vec::new();

    for (p_idx, p_text) in parent_texts.into_iter().enumerate() {
        let parent_id = format!("{}_p{}_parent_{}", page.document, page.page, p_idx);
        let mut child_ids = Vec::new();

        let is_pipe_table = p_text.contains('|') && p_text.lines().filter(|l| PIPE_ROW_RE.is_match(l.trim())).count() >= 2;

        if is_pipe_table {
            let table_chunks = split_table_rows(&p_text, &page.title);
            for (c_idx, (t_text, ind)) in table_chunks.into_iter().enumerate() {
                let child_id = format!("{}_child_t_{}", parent_id, c_idx);
                child_ids.push(child_id.clone());

                children.push(ChildChunk {
                    id: child_id,
                    parent_id: parent_id.clone(),
                    document: page.document.clone(),
                    page: page.page,
                    issuer: page.issuer.clone(),
                    doc_type: page.doc_type.clone(),
                    year: page.year.clone(),
                    title: page.title.clone(),
                    content: t_text,
                    is_table: true,
                    indicator: ind,
                });
            }
        } else {
            let child_texts = split_text_recursive(&p_text, child_chunk_size, child_chunk_overlap);
            for (c_idx, c_text) in child_texts.into_iter().enumerate() {
                let child_id = format!("{}_child_{}", parent_id, c_idx);
                child_ids.push(child_id.clone());

                let ind = match_indicator_name(&c_text);
                children.push(ChildChunk {
                    id: child_id,
                    parent_id: parent_id.clone(),
                    document: page.document.clone(),
                    page: page.page,
                    issuer: page.issuer.clone(),
                    doc_type: page.doc_type.clone(),
                    year: page.year.clone(),
                    title: page.title.clone(),
                    content: c_text,
                    is_table: false,
                    indicator: ind,
                });
            }
        }

        parents.push(ParentChunk {
            id: parent_id,
            document: page.document.clone(),
            page: page.page,
            issuer: page.issuer.clone(),
            doc_type: page.doc_type.clone(),
            year: page.year.clone(),
            title: page.title.clone(),
            content: p_text,
            child_ids,
        });
    }

    (parents, children)
}

pub fn safe_extract_snippet(text: &str, byte_start: usize, byte_end: usize, window_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let char_start = text[..byte_start].chars().count();
    let char_end = text[..byte_end].chars().count();
    let from = char_start.saturating_sub(window_chars);
    let to = (char_end + window_chars).min(chars.len());
    chars[from..to].iter().collect::<String>().replace('\n', " ").trim().to_string()
}

pub fn build_inverted_number_index(pages: &[DocumentPage]) -> InvertedNumberIndex {
    let mut index: InvertedNumberIndex = HashMap::new();

    for page in pages {
        for mat in NUMBER_RE.find_iter(&page.text) {
            let raw_token = mat.as_str();
            if let Some(norm) = normalize_number(raw_token) {
                let snippet = safe_extract_snippet(&page.text, mat.start(), mat.end(), 60);

                let loc = SourceLocation {
                    document: page.document.clone(),
                    page: page.page,
                    section_snippet: snippet,
                };

                let entry = index.entry(norm).or_default();
                if !entry.iter().any(|existing| existing.document == loc.document && existing.page == loc.page) {
                    entry.push(loc);
                }
            }
        }
    }

    index
}

pub fn build_corpus_from_dir<P: AsRef<Path>>(
    data_dir: P,
    catalog_path: Option<P>,
) -> Result<Corpus, String> {
    let data_path = data_dir.as_ref();
    let pdf_dir = data_path.join("pdfs");
    let md_dir = data_path.join("markdown");

    let catalog = catalog_path
        .map(|p| load_document_catalog(p))
        .unwrap_or_default();

    let mut all_pages = Vec::new();
    let mut file_hashes = HashMap::new();

    if md_dir.exists() {
        if let Ok(entries) = fs::read_dir(&md_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    let fname = path.file_name().unwrap().to_str().unwrap().to_string();
                    if let Ok(hash) = calculate_sha256(&path) {
                        file_hashes.insert(fname.clone(), hash);
                    }
                    match load_document_pages(&path, &catalog) {
                        Ok(pages) => all_pages.extend(pages),
                        Err(err) => eprintln!("[WARN] No se pudo parsear MD {}: {}", fname, err),
                    }
                }
            }
        }
    }

    if all_pages.is_empty() && pdf_dir.exists() {
        if let Ok(entries) = fs::read_dir(&pdf_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("pdf") {
                    let fname = path.file_name().unwrap().to_str().unwrap().to_string();
                    if let Ok(hash) = calculate_sha256(&path) {
                        file_hashes.insert(fname.clone(), hash);
                    }
                    match load_document_pages(&path, &catalog) {
                        Ok(pages) => all_pages.extend(pages),
                        Err(err) => eprintln!("[WARN] No se pudo parsear PDF {}: {}", fname, err),
                    }
                }
            }
        }
    }

    if all_pages.is_empty() {
        return Err("No se encontraron páginas de documentos para ingerir en data/.".to_string());
    }

    let number_index = build_inverted_number_index(&all_pages);

    let mut all_parents = Vec::new();
    let mut all_children = Vec::new();

    for page in &all_pages {
        let (parents, children) = process_page_into_chunks(page, 2000, 200, 400, 50);
        all_parents.extend(parents);
        all_children.extend(children);
    }

    let now_iso = chrono::Utc::now().to_rfc3339();

    Ok(Corpus {
        parents: all_parents,
        children: all_children,
        number_index,
        file_hashes,
        manifest_updated: now_iso,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_normalization() {
        assert_eq!(normalize_number("59,968"), Some("59968".to_string()));
        assert_eq!(normalize_number("1.250,50"), Some("125050".to_string()));
        assert_eq!(normalize_number("32.5%"), Some("325".to_string()));
        assert_eq!(normalize_number("1.4x"), Some("14".to_string()));
        assert_eq!(normalize_number("3.5x"), Some("35".to_string()));
        assert_eq!(normalize_number("150M"), Some("150".to_string()));
        assert_eq!(normalize_number("S/. 4500"), Some("4500".to_string()));
        assert_eq!(normalize_number("palabra"), None);
    }

    #[test]
    fn test_extract_and_normalize_numbers() {
        let text = "En 2025 Ferreycorp vendió 59,968 unidades con un margen del 14.8% y deuda de 1.4x alcanzando S/. 1200 millones.";
        let nums = extract_and_normalize_numbers(text);
        assert!(nums.contains("2025"));
        assert!(nums.contains("59968"));
        assert!(nums.contains("148"));
        assert!(nums.contains("14"));
        assert!(nums.contains("1200"));
    }

    #[test]
    fn test_table_row_splitting() {
        let md_table = "| Indicador | 2024 | 2025 |\n|---|---|---|\n| ROE | 14.2% | 16.5% |\n| Utilidad Neta | 120 M | 150 M |";
        let rows = split_table_rows(md_table, "Ferreycorp Memoria 2025");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, Some("ROE".to_string()));
        assert_eq!(rows[1].1, Some("utilidad_neta".to_string()));
    }

    #[test]
    fn test_utf8_snippet_extraction() {
        let text = "La compa\u{f1}\u{ed}a Ferreycorp alcanz\u{f3} un r\u{e9}cord hist\u{f3}rico de 59968 unidades en 2025.";
        let mat = NUMBER_RE.find(text).unwrap();
        let snippet = safe_extract_snippet(text, mat.start(), mat.end(), 10);
        assert!(snippet.contains("59968"));
    }
}
