use rag_core::guardrail::{
    query_with_self_correction, sanitize_hallucinations, verify_attribution,
    verify_numeric_grounding,
};
use rag_core::indicators::{expand_financial_queries, match_indicator_name};
use rag_core::ingest::{
    build_inverted_number_index, process_page_into_chunks, ChildChunk, Corpus, DocumentPage,
};
use rag_core::llm::{strip_think_blocks, LlmClient, LlmConfig};
use rag_core::retriever::{
    detect_issuers_in_query, segment_query_by_issuer, tokenize_text, EmbeddingClient,
    EmbeddingSearcher, HybridRetriever,
};

#[test]
fn test_full_ingest_and_serialization_cycle() {
    let page1 = DocumentPage {
        document: "Ferreycorp_Memoria_2025.md".to_string(),
        page: 14,
        text: "En 2025 Ferreycorp alcanzó un ROE de 16.5% y un margen operativo de 10.2%.".to_string(),
        issuer: "Ferreycorp".to_string(),
        doc_type: "memoria".to_string(),
        year: "2025".to_string(),
        title: "Memoria Anual Ferreycorp 2025".to_string(),
    };

    let page2 = DocumentPage {
        document: "WorldBank_Macro_Outlook_2024.md".to_string(),
        page: 1,
        text: "GDP growth is projected to reach 2.7 percent in 2024 with poverty rate at 33.2%.".to_string(),
        issuer: "Banco Mundial".to_string(),
        doc_type: "reporte".to_string(),
        year: "2024".to_string(),
        title: "Macro Poverty Outlook 2024".to_string(),
    };

    let pages = vec![page1, page2];
    let number_index = build_inverted_number_index(&pages);

    assert!(number_index.contains_key("2025"));
    assert!(number_index.contains_key("165")); // 16.5%
    assert!(number_index.contains_key("2024"));
    assert!(number_index.contains_key("27"));  // 2.7%

    let mut all_parents = Vec::new();
    let mut all_children = Vec::new();

    for p in &pages {
        let (parents, children) = process_page_into_chunks(p, 2000, 200, 400, 50);
        all_parents.extend(parents);
        all_children.extend(children);
    }

    let corpus = Corpus {
        parents: all_parents,
        children: all_children,
        number_index,
        file_hashes: Default::default(),
        manifest_updated: "2026-08-17T00:00:00Z".to_string(),
    };

    let tmp_bin = std::env::temp_dir().join("test_corpus.bin");
    corpus.save_to_binary(&tmp_bin).expect("Failed to save binary corpus");
    let loaded_corpus = Corpus::load_from_binary(&tmp_bin).expect("Failed to load binary corpus");

    assert_eq!(corpus.parents.len(), loaded_corpus.parents.len());
    assert_eq!(corpus.children.len(), loaded_corpus.children.len());
    assert_eq!(corpus.number_index.len(), loaded_corpus.number_index.len());

    let _ = std::fs::remove_file(tmp_bin);
}

#[test]
fn test_hybrid_search_precision_and_rrf() {
    let page = DocumentPage {
        document: "Financiera_Efectiva_2025.md".to_string(),
        page: 14,
        text: "Financiera Efectiva reportó una utilidad neta de S/. 152 millones y provisiones de S/. 45 millones.".to_string(),
        issuer: "Financiera Efectiva".to_string(),
        doc_type: "memoria".to_string(),
        year: "2025".to_string(),
        title: "Memoria Efectiva 2025".to_string(),
    };

    let (parents, children) = process_page_into_chunks(&page, 2000, 200, 400, 50);
    let corpus = Corpus {
        parents,
        children,
        number_index: Default::default(),
        file_hashes: Default::default(),
        manifest_updated: "2026-08-17".to_string(),
    };

    let retriever = HybridRetriever::build(corpus, None, None).expect("Retriever build failed");
    let results = retriever.retrieve_parents("¿Cuál fue la utilidad neta de Financiera Efectiva?", Some("Financiera"), 3, None);

    assert!(!results.is_empty());
    assert!(results[0].parent.content.contains("152 millones"));
    assert!(results[0].citations[0].contains("Financiera Efectiva"));
}

#[test]
fn test_guardrail_numeric_verification_speed_and_accuracy() {
    let context = "El ratio deuda EBITDA fue 1.4x en 2025 frente a 1.8x en 2024. Se invirtieron S/. 450 millones en CAPEX.";
    let question = "¿Cuál fue el ratio deuda EBITDA en 2025?";

    // 1. Valid response (100% grounded)
    let grounded_resp = "En 2025, el ratio deuda EBITDA se situó en 1.4x (frente a 1.8x en 2024).";
    let v1 = verify_numeric_grounding(grounded_resp, context, question);
    assert!(v1.is_valid);
    assert!(v1.hallucinated_numbers.is_empty());

    // 2. Hallucinated response (invented numbers: 3.5x, 900 millones)
    let hallucinated_resp = "El ratio fue de 3.5x y las inversiones alcanzaron S/. 900 millones.";
    let v2 = verify_numeric_grounding(hallucinated_resp, context, question);
    assert!(!v2.is_valid);
    assert!(v2.hallucinated_numbers.contains("35"));
    assert!(v2.hallucinated_numbers.contains("900"));

    // 3. Sanitization
    let sanitized = sanitize_hallucinations(hallucinated_resp, &v2.hallucinated_numbers);
    assert!(sanitized.contains("⚠️ Cifra no verificada"));
}

#[test]
fn test_indicators_and_think_stripping() {
    let raw = "<think>\nAnalizando el ratio de deuda...\n</think>El resultado neto fue positivo.";
    assert_eq!(strip_think_blocks(raw), "El resultado neto fue positivo.");

    let row_indicator = match_indicator_name("Utilidad Operativa (EBIT)");
    assert_eq!(row_indicator, Some("utilidad_operativa".to_string()));

    let expanded = expand_financial_queries("¿Qué nivel de apalancamiento y ROE presenta?");
    assert!(expanded.len() >= 2);
}

// ── Fase 4: verificación de atribución por indicador ────────────────────────

fn chunk(id: &str, issuer: &str, indicator: &str, content: &str) -> ChildChunk {
    ChildChunk {
        id: id.to_string(),
        parent_id: "p1".to_string(),
        document: format!("{}.md", issuer),
        page: 1,
        issuer: issuer.to_string(),
        doc_type: "memoria".to_string(),
        year: "2025".to_string(),
        title: format!("{} 2025", issuer),
        content: content.to_string(),
        is_table: false,
        indicator: Some(indicator.to_string()),
    }
}

fn corpus_with(children: Vec<ChildChunk>) -> Corpus {
    Corpus {
        parents: Vec::new(),
        children,
        number_index: Default::default(),
        file_hashes: Default::default(),
        manifest_updated: "2026-08-18T00:00:00Z".to_string(),
    }
}

#[test]
fn test_attribution_misattribution_detected() {
    // Corpus mínimo: Ferreycorp/ventas = US$ 2,177 millones en los totales.
    let corpus = corpus_with(vec![chunk(
        "c1",
        "Ferreycorp",
        "ventas",
        "Las ventas totales de Ferreycorp fueron US$ 2,177 millones en 2025.",
    )]);

    // La respuesta atribuye a los totales una cifra inexistente: US$ 950 millones.
    let findings = verify_attribution(
        "Las ventas totales de Ferreycorp fueron US$ 950 millones.",
        &corpus,
    );
    assert_eq!(findings.len(), 1, "debe haber exactamente 1 hallazgo: {findings:?}");
    let f = &findings[0];
    assert_eq!(f.indicator, "ventas");
    assert_eq!(f.issuer, "Ferreycorp");
    assert_eq!(f.claimed_currency, "usd");
    assert_eq!(f.claimed_value, "950000000");
    assert!(f.is_misattribution, "950000000 no está en el par (Ferreycorp, ventas)");
    assert!(!f.segment_qualifier);
    assert_eq!(f.correct_value.as_deref(), Some("usd|2177000000"));

    // Si la cifra sí existe en el corpus pero en un chunk de segmento
    // ("gran minería"), el hallazgo pasa a segment_qualifier (advertencia,
    // no misatribución dura).
    let corpus2 = corpus_with(vec![
        chunk(
            "c1",
            "Ferreycorp",
            "ventas",
            "Las ventas totales de Ferreycorp fueron US$ 2,177 millones en 2025.",
        ),
        chunk(
            "c2",
            "Ferreycorp",
            "ventas",
            "Las ventas del segmento gran minería alcanzaron US$ 950 millones.",
        ),
    ]);
    let findings2 = verify_attribution(
        "Las ventas totales de Ferreycorp fueron US$ 950 millones.",
        &corpus2,
    );
    assert_eq!(findings2.len(), 1, "debe haber exactamente 1 hallazgo: {findings2:?}");
    let f2 = &findings2[0];
    assert!(!f2.is_misattribution, "950000000 sí existe en el par (Ferreycorp, ventas)");
    assert!(f2.segment_qualifier, "950 existe pero en 'gran minería', no en los totales");
    assert_eq!(f2.claimed_value, "950000000");
}

#[test]
fn test_attribution_correct_value_suggested() {
    let corpus = corpus_with(vec![chunk(
        "c1",
        "Ferreycorp",
        "ventas",
        "Las ventas totales de Ferreycorp fueron US$ 2,177 millones en 2025.",
    )]);

    // Cifra correcta y sin "totales" → sin hallazgos.
    let findings = verify_attribution(
        "Las ventas de Ferreycorp alcanzaron US$ 2,177 millones.",
        &corpus,
    );
    assert!(findings.is_empty(), "cifra correcta no debe generar hallazgos: {findings:?}");
}

#[tokio::test]
async fn test_attribution_strict_flag() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    // Servidor LLM simulado: siempre devuelve una cifra de segmento reportada
    // como "ventas totales".
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let response_body = r#"{"id":"mock","choices":[{"message":{"role":"assistant","content":"Las ventas totales de Ferreycorp alcanzaron US$ 950 millones."},"finish_reason":"stop"}]}"#;
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            );
            let _ = stream.write_all(headers.as_bytes());
            let _ = stream.write_all(response_body.as_bytes());
            let _ = stream.flush();
        }
    });

    let client = LlmClient::new(LlmConfig {
        api_base: format!("http://{}", addr),
        model: "mock".to_string(),
        temperature: 0.1,
        max_tokens: 128,
        timeout_secs: 10,
    });

    let corpus = corpus_with(vec![chunk(
        "c1",
        "Ferreycorp",
        "ventas",
        "Las ventas del segmento gran minería alcanzaron US$ 950 millones.",
    )]);
    let context =
        "Ferreycorp reportó ventas del segmento gran minería por US$ 950 millones en 2025.";
    let question = "¿Cuáles fueron las ventas totales de Ferreycorp?";

    // Modo normal: el hallazgo de segmento NO invalida la respuesta.
    let res_lenient = query_with_self_correction(
        &client,
        "sys",
        context,
        question,
        3,
        Some(&corpus),
        false,
    )
    .await
    .expect("consulta leniente");
    assert!(res_lenient.is_valid, "sin strict_attribution el segment_qualifier no invalida");
    assert_eq!(res_lenient.attribution.len(), 1);
    assert!(res_lenient.attribution[0].segment_qualifier);
    assert!(!res_lenient.attribution[0].is_misattribution);

    // Modo estricto: el hallazgo de segmento invalida la respuesta.
    let res_strict = query_with_self_correction(
        &client,
        "sys",
        context,
        question,
        3,
        Some(&corpus),
        true,
    )
    .await
    .expect("consulta estricta");
    assert!(!res_strict.is_valid, "con strict_attribution el segment_qualifier invalida");
    assert_eq!(res_strict.attribution.len(), 1);
    assert!(res_strict.attribution[0].segment_qualifier);

    // El campo attribution debe serializarse (contrato Fase 4 / telemetría JSON).
    let json = serde_json::to_string(&res_strict).expect("GuardrailResponse serializable");
    assert!(json.contains("\"attribution\""), "attribution debe aparecer en el JSON");
    assert!(json.contains("\"segment_qualifier\":true"));
}

/// P9: el LLM regurgita el prompt de corrección ("Tu respuesta anterior incluyó
/// cifras NO verificadas") en lugar de generar contenido nuevo. El guardrail no
/// debe entregar ese eco como respuesta: corta el bucle y entrega la última
/// respuesta REAL del modelo (sanitizada); el eco queda solo en raw_response.
#[tokio::test]
async fn test_echo_prompt_not_accepted_as_response() {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    // Respuestas secuenciales del "LLM":
    // 1) respuesta con alucinación (999 no está en el contexto),
    // 2) eco del prompt de corrección (fallo de generación).
    let respuestas = Arc::new(Mutex::new(VecDeque::from([
        "Las ventas de Ferreycorp fueron US$ 999 millones en 2025.".to_string(),
        "\n\n🚨 [AUDITORÍA NUMÉRICA INEI - REINTENTO 1/3]:\nTu respuesta anterior incluyó cifras NO verificadas: 'usd|999000000' no existe en ningún documento.".to_string(),
    ])));
    let queue = respuestas.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let content = queue.lock().unwrap().pop_front().unwrap_or_default();
            let body = serde_json::json!({
                "id": "mock",
                "choices": [{
                    "message": {"role": "assistant", "content": content},
                    "finish_reason": "stop"
                }]
            });
            let body = serde_json::to_string(&body).unwrap();
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(headers.as_bytes());
            let _ = stream.write_all(body.as_bytes());
            let _ = stream.flush();
        }
    });

    let client = LlmClient::new(LlmConfig {
        api_base: format!("http://{}", addr),
        model: "mock".to_string(),
        temperature: 0.1,
        max_tokens: 128,
        timeout_secs: 10,
    });

    let context = "Ferreycorp reportó ventas por US$ 450 millones en 2025.";
    let question = "¿Cuáles fueron las ventas?";

    let res = query_with_self_correction(&client, "sys", context, question, 3, None, false)
        .await
        .expect("consulta con eco");

    // No se entrega el eco del prompt como respuesta final.
    assert!(!res.final_response.contains("AUDITOR"), "el eco no debe ser la respuesta final");
    assert!(!res.final_response.contains("Tu respuesta anterior"), "el eco no debe ser la respuesta final");
    // Se entrega la respuesta REAL del modelo (la de 999), sanitizada.
    assert!(
        res.final_response.contains("999"),
        "la respuesta real previa debe entregarse sanitizada: {}",
        res.final_response
    );
    assert!(!res.is_valid, "la alucinación detectada implica is_valid=false");
    // El eco queda en raw_response para diagnóstico (lo último que generó el LLM).
    assert!(
        res.raw_response.contains("AUDITOR"),
        "raw_response debe conservar el eco para diagnóstico: {}",
        res.raw_response
    );
}

/// Propuesta A: EmbeddingClient contra un endpoint local (mock HTTP). El
/// servidor responde con un vector fijo de 4 dims.
#[tokio::test]
async fn test_embedding_client_http() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let body = r#"{"data":[{"index":0,"embedding":[0.5,0.5,0.5,0.5]}]}"#;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(headers.as_bytes());
            let _ = stream.write_all(body.as_bytes());
            let _ = stream.flush();
        }
    });

    let client = EmbeddingClient::new(format!("http://{}/v1/embeddings", addr));
    let v = client.embed_query("¿ventas de Ferreycorp?").await.expect("embed ok");
    assert_eq!(v.len(), 4);
    assert!((v[0] - 0.5).abs() < 1e-6);
}

/// Propuesta A: tercer ranker activo. Con vectors artificiales donde el chunk
/// semánticamente correcto es "c_ventas", el query_vec lo lleva al top.
#[tokio::test]
async fn test_hybrid_retriever_third_ranker() {
    use rag_core::retriever::TantivyIndexWrapper;

    fn norm(v: &[f32]) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    // Dos parents: Ferreycorp (ventas) y Efectiva (patrimonio).
    let page_v = DocumentPage {
        document: "Ferreycorp_Memoria_2025.md".to_string(),
        page: 4,
        text: "Las empresas de la corporación generaron ventas de US$ 2,177 millones, superiores en 8% frente a 2024.".to_string(),
        issuer: "Ferreycorp".to_string(),
        doc_type: "memoria".to_string(),
        year: "2025".to_string(),
        title: "Memoria Ferreycorp 2025".to_string(),
    };
    let page_p = DocumentPage {
        document: "Efectiva_2025.md".to_string(),
        page: 13,
        text: "El patrimonio efectivo total de la Financiera sumó S/ 359,711 miles.".to_string(),
        issuer: "Financiera Efectiva".to_string(),
        doc_type: "memoria".to_string(),
        year: "2025".to_string(),
        title: "Memoria Efectiva 2025".to_string(),
    };
    let mut parents = Vec::new();
    let mut children = Vec::new();
    for p in [&page_v, &page_p] {
        let (ps, cs) = process_page_into_chunks(p, 2000, 200, 400, 50);
        parents.extend(ps);
        children.extend(cs);
    }
    let corpus = Corpus {
        parents,
        children,
        number_index: Default::default(),
        file_hashes: Default::default(),
        manifest_updated: "2026-08-17".to_string(),
    };

    // Vectores artificiales alineados con children: 0 → "ventas", 1 → "patrimonio".
    let vectors: Vec<Vec<f32>> = corpus
        .children
        .iter()
        .map(|c| if c.content.contains("ventas") { norm(&[1.0, 0.0]) } else { norm(&[0.0, 1.0]) })
        .collect();
    let es = EmbeddingSearcher::build(&corpus.children, vectors).expect("searcher ok");
    let tantivy = TantivyIndexWrapper::create_in_ram(&corpus.children).expect("tantivy");
    let vector_searcher = rag_core::retriever::VectorSearcher::build(&corpus.children);
    let parent_map: std::collections::HashMap<String, _> = corpus
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

    // Query_vec apunta a "ventas" → el parent de ventas debe aparecer primero.
    let q = norm(&[0.9, 0.1]);
    let results = retriever.retrieve_parents(
        "¿cuánto vendió la corporación?",
        None,
        2,
        Some(q.as_slice()),
    );
    assert!(!results.is_empty());
    assert!(
        results[0].parent.content.contains("2,177"),
        "el tercer ranker debe elevar el chunk de ventas: {}",
        results[0].parent.content
    );
}

/// Propuesta C — diagnóstico sobre el corpus REAL (data/corpus.bin): ranking de
/// parents para P5 con multi-query. #[ignore] porque requiere el corpus local.
#[test]
#[ignore = "requiere data/corpus.bin local"]
fn test_multi_query_p5_real_corpus_diagnostic() {
    use std::path::Path;
    let corpus = Corpus::load_from_binary(Path::new("data/corpus.bin"))
        .expect("cargar corpus.bin local");
    let retriever = HybridRetriever::build(corpus, None, None).expect("build");
    let question = "Comparando los reportes del corpus: que crecimiento del PBI proyecto el Banco Mundial para Peru en 2024, que ventas totales reporto Ferreycorp en dolares en 2025 y que patrimonio supero Financiera Efectiva en 2025?";
    println!("emisores detectados: {:?}", detect_issuers_in_query(question));
    let results = retriever.retrieve_parents(question, None, 6, None);
    for (i, r) in results.iter().enumerate() {
        let head: String = r.parent.content.chars().take(90).collect();
        println!("  top{} [{:.4}] {}", i + 1, r.score, head.replace('\n', " "));
    }
    assert!(!results.is_empty());

    // Diagnóstico Microsoft: el documento existe (958 children) pero no rankea.
    let ms_q = "cuales son las proyecciones futuras de microsoft ?";
    println!("=== DIAGNOSTICO MICROSOFT: {ms_q}");
    let ms_res = retriever.retrieve_parents(ms_q, None, 4, None);
    for (i, r) in ms_res.iter().enumerate() {
        let head: String = r.parent.content.chars().take(90).collect();
        println!("  top{} [{:.4}] {}", i + 1, r.score, head.replace('\n', " "));
    }
    println!("--- bm25 por término individual");
    for term in ["microsoft", "forward", "outlook", "futuro", "futuras", "estrategia"] {
        if let Ok(bm) = retriever.tantivy.search(term, None, 2) {
            let line: Vec<String> = bm
                .iter()
                .map(|(id, _, sc)| {
                    retriever
                        .corpus
                        .children
                        .iter()
                        .find(|c| &c.id == id)
                        .map(|c| format!("{}:{} ({:.2})", c.issuer, c.page, sc))
                        .unwrap_or_default()
                })
                .collect();
            println!("   '{term}' -> {}", line.join(" | "));
        }
    }
    let ms_count = retriever
        .corpus
        .children
        .iter()
        .filter(|c| c.issuer == "Microsoft")
        .count();
    println!("   children Microsoft en corpus: {ms_count}");
    let ms_tokens = retriever
        .corpus
        .children
        .iter()
        .filter(|c| c.issuer == "Microsoft")
        .flat_map(|c| tokenize_text(&c.content))
        .filter(|t| t == "microsoft")
        .count();
    println!("   tokens 'microsoft' en content de MS: {ms_tokens}");
    let fer_tokens = retriever
        .corpus
        .children
        .iter()
        .filter(|c| c.issuer == "Ferreycorp")
        .flat_map(|c| tokenize_text(&c.content))
        .filter(|t| t == "microsoft")
        .count();
    println!("   tokens 'microsoft' en content de Ferreycorp: {fer_tokens}");
    println!("--- bm25 directo (sin filtro)");
    if let Ok(bm) = retriever.tantivy.search(ms_q, None, 3) {
        for (i, (id, _, sc)) in bm.iter().enumerate() {
            let doc = retriever
                .corpus
                .children
                .iter()
                .find(|c| &c.id == id)
                .map(|c| format!("{} pág {}", c.issuer, c.page))
                .unwrap_or_default();
            println!("   bm25[{i}] {sc:.3} {doc}");
        }
    }
    let vec = retriever.vector_searcher.search(ms_q, None, 3);
    for (i, (id, _, sc)) in vec.iter().enumerate() {
        let doc = retriever
            .corpus
            .children
            .iter()
            .find(|c| &c.id == id)
            .map(|c| format!("{} pág {}", c.issuer, c.page))
            .unwrap_or_default();
        println!("   vec[{i}] {sc:.3} {doc}");
    }

    // Desglose: ranking de cada sub-consulta (BM25 y TF-IDF por separado).
    let issuers = detect_issuers_in_query(question);
    for (canon, seg) in segment_query_by_issuer(question, &issuers) {
        println!("--- sub-consulta [{canon}] {seg}");
        for variant in expand_financial_queries(&seg).iter().take(2) {
            if let Ok(bm) = retriever.tantivy.search(variant, Some(&canon), 3) {
                for (i, (id, _, sc)) in bm.iter().enumerate() {
                    let doc_id = retriever
                        .corpus
                        .children
                        .iter()
                        .find(|c| &c.id == id)
                        .map(|c| format!("pág {} {}", c.page, &c.content[..60.min(c.content.len())]))
                        .unwrap_or_default();
                    println!("   bm25[{i}] {sc:.3} {doc_id}");
                }
            }
            let vec = retriever.vector_searcher.search(variant, Some(&canon), 3);
            for (i, (id, _, sc)) in vec.iter().enumerate() {
                let doc_id = retriever
                    .corpus
                    .children
                    .iter()
                    .find(|c| &c.id == id)
                    .map(|c| format!("pág {} {}", c.page, &c.content[..60.min(c.content.len())]))
                    .unwrap_or_default();
                println!("   vec[{i}] {sc:.3} {doc_id}");
            }
        }
    }
}
#[test]
fn test_multi_query_p5_recovers_sales_page() {
    // 4 parents: BM (PBI), Efectiva (patrimonio), Ferreycorp pág.38 (ventas US$, en dólares),
    // Ferreycorp pág.100 (ventas netas S/.). La 38 y la 100 compiten por el slot de Ferreycorp.
    let pages = vec![
        DocumentPage {
            document: "WorldBank_2024.md".to_string(),
            page: 1,
            text: "El Banco Mundial proyecta un crecimiento del PBI de 2.7% para Perú en 2024.".to_string(),
            issuer: "Banco Mundial".to_string(),
            doc_type: "reporte_macro".to_string(),
            year: "2024".to_string(),
            title: "Macro Poverty Outlook".to_string(),
        },
        DocumentPage {
            document: "Efectiva_2025.md".to_string(),
            page: 3,
            text: "Financiera Efectiva superó un patrimonio de S/ 405 millones en 2025.".to_string(),
            issuer: "Financiera Efectiva".to_string(),
            doc_type: "memoria".to_string(),
            year: "2025".to_string(),
            title: "Memoria Efectiva 2025".to_string(),
        },
        DocumentPage {
            document: "Ferreycorp_2025.md".to_string(),
            page: 38,
            text: "La corporación alcanzó ventas en dólares de US$ 2,177 millones en 2025.".to_string(),
            issuer: "Ferreycorp".to_string(),
            doc_type: "memoria".to_string(),
            year: "2025".to_string(),
            title: "Memoria Ferreycorp 2025".to_string(),
        },
        DocumentPage {
            document: "Ferreycorp_2025.md".to_string(),
            page: 100,
            text: "Las ventas netas ascendieron a S/. 7,798.3 millones en soles en 2025.".to_string(),
            issuer: "Ferreycorp".to_string(),
            doc_type: "memoria".to_string(),
            year: "2025".to_string(),
            title: "Memoria Ferreycorp 2025".to_string(),
        },
    ];
    let mut parents = Vec::new();
    let mut children = Vec::new();
    for p in &pages {
        let (ps, cs) = process_page_into_chunks(p, 2000, 200, 400, 50);
        parents.extend(ps);
        children.extend(cs);
    }
    let corpus = Corpus {
        parents,
        children,
        number_index: Default::default(),
        file_hashes: Default::default(),
        manifest_updated: "2026-08-17".to_string(),
    };
    let retriever = HybridRetriever::build(corpus, None, None).expect("build ok");

    let question = "Comparando los reportes: que PBI proyecto el Banco Mundial para 2024, que ventas en dolares reporto Ferreycorp en 2025 y que patrimonio supero Financiera Efectiva?";
    assert_eq!(detect_issuers_in_query(question).len(), 3, "query compuesto detecta 3 emisores");

    let results = retriever.retrieve_parents(question, None, 3, None);
    assert_eq!(results.len(), 3);
    let top_texts: Vec<String> = results.iter().map(|r| r.parent.content.clone()).collect();
    // El objetivo de la C: la página con "ventas en dólares US$ 2,177 M" debe
    // rankear ANTES que la de "ventas netas en soles 7,798.3 M".
    let pos_2177 = top_texts.iter().position(|t| t.contains("2,177"));
    let pos_7798 = top_texts.iter().position(|t| t.contains("7,798"));
    assert!(
        pos_2177.is_some(),
        "la página de ventas US$ 2,177 debe entrar al top-3: {top_texts:?}"
    );
    assert!(
        pos_7798.is_none() || pos_2177 < pos_7798,
        "la pág. con 2177 debe rankear antes que la de solo-soles: {top_texts:?}"
    );
    // El patrimonio de Efectiva debe tener representación.
    assert!(top_texts.iter().any(|t| t.contains("405")), "patrimonio Efectiva presente: {top_texts:?}");
}
