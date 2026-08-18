use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use clap::{Parser, Subcommand};
use colored::*;

use rag_core::guardrail::{query_with_self_correction, CorrectionKind};
use rag_core::ingest::{build_corpus_from_dir, Corpus};
use rag_core::llm::{build_system_prompt, LlmClient, LlmConfig};
use rag_core::retriever::{EmbeddingClient, HybridRetriever};

#[derive(Parser, Debug)]
#[command(name = "rag_core")]
#[command(author = "INEI Data Intelligence")]
#[command(version = "0.1.0")]
#[command(about = "Motor RAG Local de Alta Fidelidad con Guardrail Numérico Determinista", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Pregunta directa a consultar
    #[arg(short, long)]
    pub query: Option<String>,

    /// Forzar reindexación completa de los documentos
    #[arg(long, default_value_t = false)]
    pub reindex: bool,

    /// Filtrar por emisor (ej: Ferreycorp, WorldBank, Financiera)
    #[arg(short, long)]
    pub filter_issuer: Option<String>,

    /// Modelo LLM a utilizar (qwen2.5-3b, qwen3.8-2b, qwen3.8-4b)
    #[arg(short, long, default_value = "qwen2.5-3b")]
    pub model: String,

    /// URL base de llama-server o endpoint compatible OpenAI
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    pub api_base: String,

    /// Número de documentos parent a recuperar (k)
    #[arg(short = 'k', long, default_value_t = 4)]
    pub top_k: usize,

    /// Ruta al directorio de datos (data/)
    #[arg(long, default_value = "data")]
    pub data_dir: String,

    /// Temperatura de generación del LLM
    #[arg(short, long, default_value_t = 0.1)]
    pub temperature: f32,

    /// Ejecutar batería de pruebas y benchmarks
    #[arg(long, default_value_t = false)]
    pub benchmark: bool,

    /// Salida JSON estructurada (telemetría I8) para query/benchmark
    #[arg(long, default_value_t = false, global = true)]
    pub json: bool,

    /// Modo estricto de atribución: los hallazgos de segmento (cifra de un
    /// segmento de negocio reportada como total) también invalidan la respuesta
    #[arg(long, default_value_t = false, global = true)]
    pub strict_attribution: bool,

    /// URL del servidor local de embeddings (Propuesta A, ej:
    /// http://127.0.0.1:8081/v1/embeddings). Vacío = retrieval 2 vías.
    #[arg(long, default_value = "", global = true)]
    pub embeddings_url: String,

    /// Ruta al binario de vectores precomputados (default: data/embeddings.bin)
    #[arg(long, default_value = "", global = true)]
    pub embeddings_bin: String,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Ingerir PDFs y generar índices binarios e invertidos
    Ingest {
        #[arg(long, default_value = "data")]
        data_dir: String,
        #[arg(long, default_value_t = true)]
        force: bool,
    },
    /// Ejecutar una consulta puntual en el corpus
    Query {
        question: String,
        #[arg(short, long)]
        issuer: Option<String>,
    },
    /// Ejecutar benchmark de velocidad y guardrail
    Benchmark,
}

fn print_banner() {
    println!("{}", "=========================================================================".cyan());
    println!("{}", "🦀 MOTOR RAG LOCAL EN RUST - INEI / FINANCIAL CORPUS 🦀".bright_green().bold());
    println!("{}", "• Búsqueda Híbrida (Tantivy BM25 + Vectorial RRF) en < 2 ms".white());
    println!("{}", "• Guardrail Numérico Determinista con Autocorrección en < 1 ms".white());
    println!("{}", "• Modelos Locales Soportados: Qwen 2.5 3B / Qwen 3.8 2B / Qwen 3.8 4B".white());
    println!("{}", "=========================================================================\n".cyan());
}

fn load_or_build_corpus(data_dir_path: &Path, force_reindex: bool) -> Result<Corpus, String> {
    let corpus_bin_path = data_dir_path.join("corpus.bin");
    let corpus_json_path = data_dir_path.join("corpus.json");
    let metadata_path = data_dir_path.join("document_metadata.json");

    if !force_reindex && corpus_bin_path.exists() {
        println!("{}", format!("[INFO] Cargando corpus desde {}", corpus_bin_path.display()).yellow());
        match Corpus::load_from_binary(&corpus_bin_path) {
            Ok(corpus) => {
                println!("{}", format!("  ✔ Corpus cargado: {} parents, {} children, {} números indexados.", 
                    corpus.parents.len(), corpus.children.len(), corpus.number_index.len()).green());
                return Ok(corpus);
            }
            Err(e) => {
                println!("{}", format!("[WARN] Falló carga binaria ({}), reintentando JSON o reindex...", e).yellow());
                if corpus_json_path.exists() {
                    if let Ok(corpus) = Corpus::load_from_json(&corpus_json_path) {
                        return Ok(corpus);
                    }
                }
            }
        }
    }

    println!("{}", format!("[INFO] Procesando ingesta e índices en {}", data_dir_path.display()).bright_blue());
    let catalog_opt = if metadata_path.exists() { Some(metadata_path.as_path()) } else { None };
    
    let t0 = Instant::now();
    let corpus = build_corpus_from_dir(data_dir_path, catalog_opt)?;
    let elapsed = t0.elapsed();

    println!("{}", format!("  ✔ Ingesta completada en {:.2?}: {} parents, {} children, {} números únicos.", 
        elapsed, corpus.parents.len(), corpus.children.len(), corpus.number_index.len()).bright_green());

    let _ = corpus.save_to_binary(&corpus_bin_path);
    let _ = corpus.save_to_json(&corpus_json_path);
    println!("{}", format!("  ✔ Guardado en {} y {}", corpus_bin_path.display(), corpus_json_path.display()).green());

    Ok(corpus)
}

async fn execute_query(
    retriever: &HybridRetriever,
    client: &LlmClient,
    question: &str,
    issuer_filter: Option<&str>,
    top_k: usize,
    json_output: bool,
    strict_attribution: bool,
    embeddings: Option<&EmbeddingClient>,
) {
    // 1. Retrieval
    let t_retrieval = Instant::now();
    // Propuesta A: embedding del query (HTTP local, async). Fallback silencioso.
    let query_vec = if let Some(ec) = embeddings {
        match ec.embed_query(question).await {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("{}", format!("[WARN] embeddings no disponibles: {}", e).yellow());
                None
            }
        }
    } else {
        None
    };
    let retrieved_parents = retriever.retrieve_parents(question, issuer_filter, top_k, query_vec.as_deref());
    let retrieval_time = t_retrieval.elapsed();

    let mut sources: Vec<String> = Vec::new();
    let mut context_blocks = Vec::new();
    for r in &retrieved_parents {
        let citation = r.citations.first().cloned().unwrap_or_else(|| r.parent.document.clone());
        sources.push(citation.clone());
        context_blocks.push(format!("DOCUMENTO: {}\nCONTENIDO:\n{}", citation, r.parent.content));
    }

    if retrieved_parents.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::json!({
                    "ok": false,
                    "question": question,
                    "error": "No se encontraron documentos relevantes",
                })
            );
        } else {
            println!("{}", "❌ No se encontraron documentos relevantes para esta consulta.".red());
        }
        return;
    }

    let combined_context = context_blocks.join("\n\n");
    let system_prompt = build_system_prompt();

    // 2. LLM Generation + Guardrail Self-Correction
    let is_online = client.check_health().await;
    if !is_online {
        if json_output {
            println!(
                "{}",
                serde_json::json!({
                    "ok": false,
                    "question": question,
                    "error": "llama-server no responde",
                    "retrieval_ms": retrieval_time.as_secs_f64() * 1000.0,
                })
            );
        } else {
            println!("\n{}", "⚠️ AVISO: llama-server no responde en el puerto configurado.".yellow());
            println!("  Retrieval híbrido completado con éxito. Para respuesta con LLM local:");
            println!("  Ejecuta: llama-server.exe -m llama_cpp/{} -c 4096 --port 8080\n", client.config.model);
        }
        return;
    }

    let guardrail_result = query_with_self_correction(
        client,
        system_prompt,
        &combined_context,
        question,
        3,
        Some(&retriever.corpus),
        strict_attribution,
    )
    .await;

    let Ok(guardrail_res) = guardrail_result else {
        let err = guardrail_result.unwrap_err();
        if json_output {
            println!(
                "{}",
                serde_json::json!({
                    "ok": false,
                    "question": question,
                    "error": err,
                    "retrieval_ms": retrieval_time.as_secs_f64() * 1000.0,
                    "sources": sources,
                })
            );
        } else {
            println!("{}", format!("❌ Error durante la generación: {}", err).red());
        }
        return;
    };

    if json_output {
        let report = serde_json::json!({
            "ok": true,
            "question": question,
            "issuer_filter": issuer_filter,
            "retrieval_ms": retrieval_time.as_secs_f64() * 1000.0,
            "sources": sources,
            "guardrail": &guardrail_res,
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return;
    }

    // Salida legible original.
    println!("\n{}", "-------------------------------------------------------------------------".cyan());
    println!("{}: {}", "🔎 PREGUNTA".bright_yellow().bold(), question);
    if let Some(issuer) = issuer_filter {
        println!("{}: {}", "🏢 FILTRO EMISOR".bright_cyan(), issuer);
    }
    println!("{}: {:.2?} ({} parents recuperados)", "⚡ TIEMPO RETRIEVAL HÍBRIDO".bright_green().bold(), retrieval_time, retrieved_parents.len());
    println!("\n{}", "📄 FUENTES Y CITAS ENCONTRADAS:".bright_blue().bold());
    for (idx, s) in sources.iter().enumerate() {
        println!("  [{}] {} (Score RRF: {:.4})", idx + 1, s.bright_white().bold(), retrieved_parents[idx].score);
    }

    println!("\n{}", "🤖 GENERANDO RESPUESTA CON GUARDRAIL NUMÉRICO...".bright_magenta().bold());
    println!("\n{}", "========================== RESPUESTA EJECUTIVA ==========================".bright_green().bold());
    println!("{}", guardrail_res.final_response);
    println!("{}", "=========================================================================".bright_green().bold());

    println!("\n{}", "🛡️ REPORTE DE AUDITORÍA Y GUARDRAIL NUMÉRICO:".bright_cyan().bold());
    if guardrail_res.is_valid {
        println!("  ✔ Estado: {} (100% cifras respaldadas)", "VALIDADO".bright_green().bold());
    } else {
        println!("  ⚠ Estado: {} (Cifras dudosas detectadas y saneadas)", "ADVERTENCIA".bright_yellow().bold());
        println!("  🚨 Cifras no verificadas: {:?}", guardrail_res.hallucinated_numbers);
        if !guardrail_res.hallucinated_money.is_empty() {
            println!("  🚨 Frases monetarias no verificadas: {:?}", guardrail_res.hallucinated_money);
        }
        for c in &guardrail_res.corrections {
            match c.kind {
                CorrectionKind::Moneda => {
                    println!("  💡 Corrección sugerida: {} → {} [{}]", c.hallucinated, c.suggested_value.as_deref().unwrap_or(""), c.source.as_deref().unwrap_or(""));
                }
                CorrectionKind::EnCorpus => {
                    println!("  ⚠ Atribución: '{}' existe en [{}] — verificar concepto.", c.hallucinated, c.source.as_deref().unwrap_or("contexto"));
                }
                _ => {}
            }
        }
    }
    if !guardrail_res.attribution.is_empty() {
        println!("  🔍 Atribución por indicador (Fase 4):");
        for f in &guardrail_res.attribution {
            if f.is_misattribution {
                println!("  🚨 Mala atribución: '{} {}' para '{}' en '{}' no aparece en el documento para ese indicador.", f.claimed_currency, f.claimed_value, f.indicator, f.issuer);
                if let Some(v) = &f.correct_value {
                    println!("     ✔ Valor único en el corpus para ese indicador: {}", v);
                }
            }
            if f.segment_qualifier {
                println!("  ⚠ Segmento vs. totales: '{} {}' de '{}' en '{}' corresponde a un segmento (ej. gran minería), no a los totales.", f.claimed_currency, f.claimed_value, f.indicator, f.issuer);
            }
        }
    }
    println!("  • Reintentos / Autocorrección: {} intento(s) (Auto-corregido: {})", guardrail_res.attempts, if guardrail_res.auto_corrected { "SÍ".bright_yellow() } else { "NO".white() });
    println!("  • Tiempo LLM Total: {:.2} ms", guardrail_res.latency_ms);
    println!("  • Latencia Validación Numérica: {} µs (< 1 ms)", guardrail_res.guardrail_latency_micros);
}

async fn run_benchmarks(retriever: &HybridRetriever, _client: &LlmClient) {
    println!("\n{}", "📊 EJECUTANDO BATERÍA DE BENCHMARKS Y PRUEBAS DE ESTRÉS 📊".bright_yellow().bold());
    println!("{}", "=========================================================================".cyan());

    let test_queries = vec![
        ("¿Cuántos camiones de minería entregó Ferreycorp en 2025 y qué ratio de deuda reportó?", Some("Ferreycorp")),
        ("¿Cuál fue el ROE y el margen operativo de Ferreycorp en 2025?", Some("Ferreycorp")),
        ("¿Cuáles fueron los principales factores macroeconómicos y de pobreza según el Banco Mundial?", Some("WorldBank")),
        ("¿Cuál fue la utilidad neta y el nivel de provisiones reportado por Financiera Efectiva?", Some("Financiera")),
        ("¿Qué proyectos de inversión y capex se ejecutaron en el periodo 2024-2025?", None),
    ];

    let mut retrieval_latencies = Vec::new();

    for (i, (query, issuer)) in test_queries.iter().enumerate() {
        println!("\n[Test {}/{}] Consulta: {}", i + 1, test_queries.len(), query.bright_white());
        let t0 = Instant::now();
        let results = retriever.retrieve_parents(query, *issuer, 4, None);
        let lat = t0.elapsed();
        retrieval_latencies.push(lat);

        println!("  ✔ Retrieval completado en: {:.3?} | Docs recuperados: {}", lat, results.len());
        for (r_i, r) in results.iter().enumerate() {
            let cit = r.citations.first().cloned().unwrap_or_default();
            println!("     [{}] {} (RRF: {:.4})", r_i + 1, cit, r.score);
        }
    }

    let avg_micros: u128 = retrieval_latencies.iter().map(|d| d.as_micros()).sum::<u128>() / retrieval_latencies.len().max(1) as u128;
    println!("\n{}", "-------------------------------------------------------------------------".cyan());
    println!("{}: {:.2} ms ({} µs) [Meta: < 3.0 ms]", 
        "🏆 LATENCIA PROMEDIO DE RETRIEVAL HÍBRIDO".bright_green().bold(), 
        avg_micros as f64 / 1000.0, 
        avg_micros
    );

    // Test Guardrail speed
    let fake_context = "Las ventas de 2025 fueron 59968 unidades y la deuda fue 1400 millones.";
    let fake_resp_good = "En 2025 se vendieron 59968 unidades.";
    let fake_resp_bad = "En 2025 se vendieron 99999 unidades.";

    let t_g0 = Instant::now();
    let v_good = rag_core::guardrail::verify_numeric_grounding(fake_resp_good, fake_context, "¿Ventas 2025?");
    let g_lat_good = t_g0.elapsed();

    let t_g1 = Instant::now();
    let v_bad = rag_core::guardrail::verify_numeric_grounding(fake_resp_bad, fake_context, "¿Ventas 2025?");
    let g_lat_bad = t_g1.elapsed();

    println!("{}: {:.2?} (Válido: {})", "🛡️ TEST GUARDRAIL GROUNDED".bright_cyan(), g_lat_good, v_good.is_valid);
    println!("{}: {:.2?} (Alucinación detectada: {:?})", "🛡️ TEST GUARDRAIL ALUCINACIÓN".bright_cyan(), g_lat_bad, v_bad.hallucinated_numbers);
    println!("{}\n", "=========================================================================".cyan());
}

async fn interactive_loop(
    retriever: &HybridRetriever,
    client: &LlmClient,
    default_issuer: Option<String>,
    strict_attribution: bool,
    embeddings: Option<&EmbeddingClient>,
) {
    let mut current_issuer = default_issuer;
    let stdin = io::stdin();

    println!("{}", "Modo Interactivo Iniciado. Comandos especiales:".bright_yellow());
    println!("  :filter <EMISOR>  -> Establecer filtro de emisor (ej: :filter Ferreycorp)");
    println!("  :clear            -> Limpiar filtro de emisor");
    println!("  :bench            -> Correr benchmark de latencia");
    println!("  :exit / :quit     -> Salir del programa\n");

    loop {
        let filter_display = current_issuer.as_deref().unwrap_or("Todos");
        print!("{} [{}] > ", "rag_core".bright_green().bold(), filter_display.bright_cyan());
        let _ = io::stdout().flush();

        let mut input = String::new();
        if stdin.read_line(&mut input).is_err() {
            break;
        }

        let query = input.trim();
        if query.is_empty() {
            continue;
        }

        if query.eq_ignore_ascii_case(":exit") || query.eq_ignore_ascii_case(":quit") || query.eq_ignore_ascii_case("exit") {
            println!("{}", "Cerrando sesión RAG. ¡Hasta pronto!".green());
            break;
        }

        if query.starts_with(":filter ") {
            let issuer = query[8..].trim().to_string();
            current_issuer = if issuer.is_empty() { None } else { Some(issuer) };
            println!("{}", format!("Filtro de emisor fijado a: {:?}", current_issuer).bright_blue());
            continue;
        }

        if query.eq_ignore_ascii_case(":clear") {
            current_issuer = None;
            println!("{}", "Filtro de emisor eliminado (búsqueda en todo el corpus).".bright_blue());
            continue;
        }

        if query.eq_ignore_ascii_case(":bench") {
            run_benchmarks(retriever, client).await;
            continue;
        }

        execute_query(retriever, client, query, current_issuer.as_deref(), 4, false, strict_attribution, embeddings).await;
    }
}

#[tokio::main]
async fn main() {
    print_banner();
    let cli = Cli::parse();

    let data_dir = PathBuf::from(&cli.data_dir);
    let tantivy_dir = data_dir.join("tantivy_index");

    let corpus = match load_or_build_corpus(&data_dir, cli.reindex) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("{}", format!("❌ Error cargando corpus: {}", err).red().bold());
            std::process::exit(1);
        }
    };

    println!("{}", "[INFO] Inicializando Motor de Búsqueda Híbrida Tantivy + Vectorial...".bright_blue());
    let t_init = Instant::now();

    // Propuesta A: tercer ranker semántico opcional (endpoint local e5-large).
    let embeddings_bin: Option<PathBuf> = if cli.embeddings_url.is_empty() {
        None
    } else if cli.embeddings_bin.is_empty() {
        Some(data_dir.join("embeddings.bin"))
    } else {
        Some(PathBuf::from(&cli.embeddings_bin))
    };
    let embedding_client = if cli.embeddings_url.is_empty() {
        None
    } else {
        Some(EmbeddingClient::new(cli.embeddings_url.clone()))
    };

    let retriever = match HybridRetriever::build(corpus, Some(&tantivy_dir), embeddings_bin.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}", format!("❌ Error inicializando HybridRetriever: {}", e).red().bold());
            std::process::exit(1);
        }
    };
    println!("{}", format!("  ✔ Retriever listo en {:.2?}", t_init.elapsed()).green());

    let resolved_model = LlmClient::resolve_model_alias(&cli.model);
    let llm_config = LlmConfig {
        api_base: cli.api_base.clone(),
        model: resolved_model.clone(),
        temperature: cli.temperature,
        max_tokens: 1024,
        timeout_secs: 120,
    };
    let client = LlmClient::new(llm_config);

    if cli.benchmark {
        run_benchmarks(&retriever, &client).await;
        return;
    }

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Ingest { data_dir: d, force } => {
                let p = PathBuf::from(d);
                let _ = load_or_build_corpus(&p, force);
                return;
            }
            Commands::Query { question, issuer } => {
                let filter = issuer.or(cli.filter_issuer);
                execute_query(&retriever, &client, &question, filter.as_deref(), cli.top_k, cli.json, cli.strict_attribution, embedding_client.as_ref()).await;
                return;
            }
            Commands::Benchmark => {
                run_benchmarks(&retriever, &client).await;
                return;
            }
        }
    }

    if let Some(question) = cli.query {
        execute_query(&retriever, &client, &question, cli.filter_issuer.as_deref(), cli.top_k, cli.json, cli.strict_attribution, embedding_client.as_ref()).await;
        return;
    }

    // Default: Interactive loop
    interactive_loop(&retriever, &client, cli.filter_issuer, cli.strict_attribution, embedding_client.as_ref()).await;
}
