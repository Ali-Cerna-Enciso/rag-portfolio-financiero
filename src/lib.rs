pub mod indicators;
pub mod ingest;
pub mod retriever;
pub mod llm;
pub mod guardrail;
pub mod numeric;

pub fn info() -> &'static str {
    "rag_core v0.1.0 - High-Fidelity Local Rust RAG"
}
