//! Guardrail Numérico v3 — "tutor que corrige con evidencia".
//!
//! Fase 1: verificación dual (números crudos + frases monetarias + porcentajes)
//!         con canonicalización de alias y multiplicadores.
//! Fase 2: grounded correction — en cada reintento se envían el valor correcto
//!         y la fuente (vía `InvertedNumberIndex`) en lugar de negar genérico (P6).
//!         Corte de rebote: si el set de alucinaciones se repite idéntico,
//!         se sale a sanitización sin quemar generaciones.
//! Fase 3: sanitización por frase monetaria completa, no por dígito.
//! Fase 4: verificación de atribución por indicador (emisor × indicador):
//!         detecta cifras que no pertenecen al par (emisor, indicador) del
//!         corpus y advierte cuando una cifra del corpus corresponde a un
//!         segmento de negocio y no a los totales.

use std::collections::{HashMap, HashSet};
use std::time::Instant;
use serde::{Deserialize, Serialize};

use crate::indicators::find_matched_indicators;
use crate::ingest::Corpus;
use crate::llm::LlmClient;
use crate::numeric::{
    extract_money_phrases, extract_percent_phrases, extract_raw_numbers, extract_years,
    MoneyPhrase, PercentPhrase,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub is_valid: bool,
    pub allowed_numbers: HashSet<String>,
    pub response_numbers: HashSet<String>,
    pub hallucinated_numbers: HashSet<String>,
    pub hallucinated_money: Vec<MoneyPhrase>,
    pub hallucinated_percent: Vec<PercentPhrase>,
    pub latency_micros: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CorrectionKind {
    /// El número no existe en el corpus ni en el contexto.
    NoEspecificado,
    /// Mismo valor con otra moneda: se sugiere el par correcto.
    Moneda,
    /// Variante de escala/representación: se sugiere la forma literal.
    Representacion,
    /// Existe en el corpus pero fuera del contexto recuperado (posible
    /// atribución errónea): se advierte sin sugerir un valor.
    EnCorpus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub kind: CorrectionKind,
    pub hallucinated: String,
    pub suggested_value: Option<String>,
    pub source: Option<String>,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptInfo {
    pub attempt: usize,
    pub is_valid: bool,
    pub hallucinated_numbers: Vec<String>,
    pub hallucinated_money: Vec<String>,
    pub corrections_sent: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailResponse {
    pub raw_response: String,
    pub final_response: String,
    pub is_valid: bool,
    pub attempts: usize,
    pub hallucinated_numbers: Vec<String>,
    pub hallucinated_money: Vec<String>,
    pub corrections: Vec<Correction>,
    pub attempt_log: Vec<AttemptInfo>,
    pub latency_ms: f64,
    pub guardrail_latency_micros: u128,
    pub auto_corrected: bool,
    /// Hallazgos de atribución por indicador (Fase 4).
    pub attribution: Vec<AttributionFinding>,
}

/// Hallazgo de la verificación de atribución (Fase 4): una cifra monetaria de
/// la respuesta no corresponde al par (emisor, indicador) del corpus, o bien
/// pertenece a un segmento de negocio y no a los totales del emisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionFinding {
    /// Nombre canónico del indicador, ej. "ventas".
    pub indicator: String,
    /// Emisor canónico detectado, ej. "Ferreycorp".
    pub issuer: String,
    /// Moneda reclamada en la respuesta ("usd" | "pen").
    pub claimed_currency: String,
    /// Valor canónico reclamado, ej. "950000000".
    pub claimed_value: String,
    /// Único valor "(currency|value)" del par (emisor, indicador) en el corpus,
    /// si el conjunto tiene exactamente 1 elemento; si no, None.
    pub correct_value: Option<String>,
    /// La oración menciona "total"/"totales" pero la cifra proviene de un
    /// chunk con calificador de segmento (gran minería, segmento, etc.).
    pub segment_qualifier: bool,
    /// La cifra no existe en ningún chunk del par (emisor, indicador).
    pub is_misattribution: bool,
}

/// Aliases de emisor → nombre canónico usado en el corpus.
const ISSUER_ALIASES: &[(&str, &[&str])] = &[
    ("Ferreycorp", &["ferreycorp", "ferreyros"]),
    ("Financiera Efectiva", &["financiera efectiva"]),
    ("Banco Mundial", &["banco mundial", "world bank", "worldbank"]),
    ("Microsoft", &["microsoft", "msft"]),
];

/// Calificadores que indican que la cifra pertenece a un segmento de negocio
/// y no a los totales del emisor.
const SEGMENT_QUALIFIERS: &[&str] = &[
    "gran minería",
    "gran mineria",
    "segmento",
    "récord de ventas",
    "record de ventas",
    "línea de negocio",
    "linea de negocio",
];

/// ¿La frase monetaria está respaldada por el contexto o el corpus?
fn is_money_grounded(
    m: &MoneyPhrase,
    money_by_currency: &HashMap<String, HashSet<String>>,
    allowed_raw: &HashSet<String>,
) -> bool {
    if let Some(vals) = money_by_currency.get(&m.currency) {
        vals.contains(&m.value)
    } else {
        // Moneda ausente en el contexto → fallback a crudo (tabla sin símbolo).
        allowed_raw.contains(&m.base_digits)
    }
}

fn is_percent_grounded(
    p: &PercentPhrase,
    allowed_pct: &HashSet<String>,
    allowed_raw: &HashSet<String>,
) -> bool {
    allowed_pct.contains(&p.canon) || allowed_raw.contains(&p.base_digits)
}

pub fn verify_numeric_grounding(
    response_text: &str,
    context_text: &str,
    question_text: &str,
) -> VerificationResult {
    let start = Instant::now();

    // Conjunto permitido de números crudos: contexto + pregunta + ordinales + años.
    let mut allowed_raw = extract_raw_numbers(context_text);
    allowed_raw.extend(extract_raw_numbers(question_text));
    allowed_raw.extend(extract_years(context_text));
    allowed_raw.extend(extract_years(question_text));
    for i in 0..=10 {
        allowed_raw.insert(i.to_string());
    }

    // Frases monetarias permitidas (contexto + pregunta), agrupadas por moneda.
    let allowed_money: Vec<MoneyPhrase> = extract_money_phrases(context_text)
        .into_iter()
        .chain(extract_money_phrases(question_text))
        .collect();
    let mut money_by_currency: HashMap<String, HashSet<String>> = HashMap::new();
    for m in &allowed_money {
        money_by_currency
            .entry(m.currency.clone())
            .or_default()
            .insert(m.value.clone());
    }

    // Porcentajes permitidos.
    let allowed_pct: HashSet<String> = extract_percent_phrases(context_text)
        .into_iter()
        .chain(extract_percent_phrases(question_text))
        .map(|p| p.canon)
        .collect();

    // Respuesta.
    let response_nums = extract_raw_numbers(response_text);
    let response_money = extract_money_phrases(response_text);
    let response_pct = extract_percent_phrases(response_text);

    // Números crudos que están dentro de frases monetarias/porcentajes válidas
    // quedan protegidos (no se marcan como alucinación).
    let mut protected: HashSet<String> = HashSet::new();
    let mut hallucinated_money: Vec<MoneyPhrase> = Vec::new();
    for m in &response_money {
        if is_money_grounded(m, &money_by_currency, &allowed_raw) {
            protected.insert(m.base_digits.clone());
        } else {
            hallucinated_money.push(m.clone());
        }
    }

    let mut hallucinated_percent: Vec<PercentPhrase> = Vec::new();
    for p in &response_pct {
        if is_percent_grounded(p, &allowed_pct, &allowed_raw) {
            protected.insert(p.base_digits.clone());
        } else {
            hallucinated_percent.push(p.clone());
        }
    }

    let hallucinated_numbers: HashSet<String> = response_nums
        .difference(&allowed_raw)
        .filter(|n| !protected.contains(*n))
        .cloned()
        .collect();

    let latency_micros = start.elapsed().as_micros();

    VerificationResult {
        is_valid: hallucinated_numbers.is_empty()
            && hallucinated_money.is_empty()
            && hallucinated_percent.is_empty(),
        allowed_numbers: allowed_raw,
        response_numbers: response_nums,
        hallucinated_numbers,
        hallucinated_money,
        hallucinated_percent,
        latency_micros,
    }
}

/// ¿El texto (en minúsculas) contiene la palabra completa (con límites de
/// palabra, sin acentos exigidos en el resto del texto)?
fn sentence_has_word(text_low: &str, word_low: &str) -> bool {
    let bytes = text_low.as_bytes();
    let mut search_from = 0;
    while let Some(rel) = text_low[search_from..].find(word_low) {
        let pos = search_from + rel;
        let before_ok = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
        let after_pos = pos + word_low.len();
        let after_ok = after_pos >= bytes.len() || !bytes[after_pos].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        search_from = pos + 1;
    }
    false
}

/// Divide la respuesta en oraciones sin cortar decimales ("2.7%") ni la
/// moneda "S/." — los fragmentos residuales sin letras (continuaciones
/// numéricas) se unen a la oración anterior.
fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut raw: Vec<String> = Vec::new();
    let mut current = String::new();

    for (i, &c) in chars.iter().enumerate() {
        let boundary = match c {
            '.' => {
                let prev_is_digit = i > 0 && chars[i - 1].is_ascii_digit();
                let next_is_digit = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
                let prev_is_currency =
                    i > 0 && (chars[i - 1] == '/' || chars[i - 1] == 's' || chars[i - 1] == 'S');
                !(prev_is_digit && next_is_digit) && !prev_is_currency
            }
            '?' | '!' | '\n' | '\r' => true,
            _ => false,
        };
        current.push(c);
        if boundary {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                raw.push(trimmed);
            }
            current.clear();
        }
    }
    let tail = current.trim().to_string();
    if !tail.is_empty() {
        raw.push(tail);
    }

    let mut merged: Vec<String> = Vec::new();
    for s in raw {
        let has_alpha = s.chars().any(|c| c.is_alphabetic());
        if has_alpha {
            merged.push(s);
        } else if let Some(last) = merged.last_mut() {
            last.push(' ');
            last.push_str(&s);
        } else {
            merged.push(s);
        }
    }
    merged
}

/// Detecta el emisor por alias y devuelve su nombre canónico del corpus.
fn detect_issuer(sentence_low: &str) -> Option<&'static str> {
    for (canon, aliases) in ISSUER_ALIASES {
        for alias in *aliases {
            if sentence_has_word(sentence_low, alias) {
                return Some(canon);
            }
        }
    }
    None
}

/// Fase 4: verifica por oración que cada cifra monetaria de la respuesta
/// corresponda al par (emisor, indicador) presente en el corpus.
///
/// - Si la cifra no existe en ningún chunk del par → `is_misattribution`.
/// - Si la cifra existe pero la oración habla de "totales" y el chunk donde
///   aparece el valor corresponde a un segmento de negocio → `segment_qualifier`.
pub fn verify_attribution(response: &str, corpus: &Corpus) -> Vec<AttributionFinding> {
    let mut findings = Vec::new();

    for sentence in split_sentences(response) {
        let sentence_low = sentence.to_lowercase();
        let Some(issuer) = detect_issuer(&sentence_low) else {
            continue;
        };
        let Some(indicator) = find_matched_indicators(&sentence).into_iter().next() else {
            continue;
        };
        let money_phrases = extract_money_phrases(&sentence);
        if money_phrases.is_empty() {
            continue;
        }

        // Valores "(currency|value)" presentes en el corpus para el par
        // (emisor, indicador), con los snippets de los chunks por valor.
        let mut pair_values: HashSet<String> = HashSet::new();
        let mut snippets_by_value: HashMap<String, Vec<String>> = HashMap::new();
        for chunk in &corpus.children {
            let issuer_matches = chunk
                .issuer
                .to_lowercase()
                .contains(&issuer.to_lowercase());
            let indicator_matches = chunk.indicator.as_deref() == Some(indicator.as_str());
            if !issuer_matches || !indicator_matches {
                continue;
            }
            for m in extract_money_phrases(&chunk.content) {
                let key = format!("{}|{}", m.currency, m.value);
                pair_values.insert(key.clone());
                snippets_by_value
                    .entry(key)
                    .or_default()
                    .push(chunk.content.clone());
            }
        }

        let sentence_mentions_total = sentence_has_word(&sentence_low, "total")
            || sentence_has_word(&sentence_low, "totales");

        for m in &money_phrases {
            let key = format!("{}|{}", m.currency, m.value);
            let in_pair = pair_values.contains(&key);
            let mut finding = AttributionFinding {
                indicator: indicator.clone(),
                issuer: issuer.to_string(),
                claimed_currency: m.currency.clone(),
                claimed_value: m.value.clone(),
                correct_value: if pair_values.len() == 1 {
                    pair_values.iter().next().cloned()
                } else {
                    None
                },
                segment_qualifier: false,
                is_misattribution: false,
            };

            if !in_pair {
                finding.is_misattribution = true;
            } else if sentence_mentions_total {
                // La cifra existe en el corpus, pero el chunk
                // donde aparece tiene calificador de segmento sin mencionar
                // "total" → no corresponde a los totales del emisor.
                let segment_snippet = snippets_by_value.get(&key).map_or(false, |snippets| {
                    snippets.iter().any(|snip| {
                        let snip_low = snip.to_lowercase();
                        let has_qualifier =
                            SEGMENT_QUALIFIERS.iter().any(|q| snip_low.contains(q));
                        let mentions_total = sentence_has_word(&snip_low, "total")
                            || sentence_has_word(&snip_low, "totales");
                        has_qualifier && !mentions_total
                    })
                });
                finding.segment_qualifier = segment_snippet;
            }

            if finding.is_misattribution || finding.segment_qualifier {
                findings.push(finding);
            }
        }
    }

    findings
}

/// Sanitización por frase: primero tacha frases monetarias/porcentajes
/// completas, luego dígitos sueltos no protegidos.
pub fn sanitize_hallucinations_full(
    text: &str,
    hallucinations: &HashSet<String>,
    hallucinated_money: &[MoneyPhrase],
    hallucinated_percent: &[PercentPhrase],
) -> String {
    if hallucinations.is_empty() && hallucinated_money.is_empty() && hallucinated_percent.is_empty() {
        return text.to_string();
    }

    let mut sanitized = text.to_string();
    let mut placeholders: Vec<(String, String)> = Vec::new();

    // 1. Frases monetarias → placeholder temporal (evita doble tachado).
    for (i, m) in hallucinated_money.iter().enumerate() {
        let ph = format!("\u{0}MON{i}\u{0}");
        if sanitized.contains(&m.raw_text) {
            sanitized = sanitized.replace(&m.raw_text, &ph);
            placeholders.push((ph, format!("~~{}~~ [⚠️ Cifra no verificada en fuentes]", m.raw_text)));
        }
    }
    // 2. Porcentajes → placeholder.
    for (i, p) in hallucinated_percent.iter().enumerate() {
        let ph = format!("\u{0}PCT{i}\u{0}");
        if sanitized.contains(&p.raw_text) {
            sanitized = sanitized.replace(&p.raw_text, &ph);
            placeholders.push((ph, format!("~~{}~~ [⚠️ Cifra no verificada en fuentes]", p.raw_text)));
        }
    }
    // 3. Dígitos sueltos.
    for bad_num in hallucinations {
        let replacement = format!("~~{}~~ [⚠️ Cifra no verificada en fuentes]", bad_num);
        sanitized = sanitized.replace(bad_num, &replacement);
    }
    // 4. Restaurar frases.
    for (ph, final_text) in placeholders {
        sanitized = sanitized.replace(&ph, &final_text);
    }
    sanitized
}

/// Compatibilidad: sanitización por dígito (firma original).
pub fn sanitize_hallucinations(text: &str, hallucinations: &HashSet<String>) -> String {
    sanitize_hallucinations_full(text, hallucinations, &[], &[])
}

/// Fase 2: busca para cada alucinación su corrección con fuente en el corpus.
pub fn find_corrections(
    hallucinated_numbers: &[String],
    hallucinated_money: &[MoneyPhrase],
    hallucinated_percent: &[PercentPhrase],
    corpus: Option<&Corpus>,
    context: &str,
) -> Vec<Correction> {
    let mut corrections = Vec::new();
    let index = corpus.map(|c| &c.number_index);

    let context_raw = extract_raw_numbers(context);

    // Frases monetarias alucinadas.
    for m in hallucinated_money {
        let key = format!("{}|{}", m.currency, m.value);
        let mut correction = Correction {
            kind: CorrectionKind::NoEspecificado,
            hallucinated: key.clone(),
            suggested_value: None,
            source: None,
            snippet: None,
        };

        // 1) Localizar el valor en el índice por clave canónica O clave base
        //    (el índice real indexa crudos: "405", no "405000000").
        let mut locs_found: Vec<&crate::ingest::SourceLocation> = Vec::new();
        if let Some(idx) = index {
            if let Some(locs) = idx.get(&m.value) {
                locs_found.extend(locs.iter());
            }
            if let Some(locs) = idx.get(&m.base_digits) {
                locs_found.extend(locs.iter());
            }
        }

        for loc in &locs_found {
            for alt in extract_money_phrases(&loc.section_snippet) {
                // Mismo valor, moneda distinta → mix-up de moneda.
                if alt.value == m.value && alt.currency != m.currency {
                    correction.kind = CorrectionKind::Moneda;
                    correction.suggested_value = Some(format!("{}|{}", alt.currency, alt.value));
                    correction.source = Some(format!("{} pág. {}", loc.document, loc.page));
                    correction.snippet = Some(loc.section_snippet.clone());
                    break;
                }
            }
            if correction.kind == CorrectionKind::Moneda {
                break;
            }
        }

        // 2) El valor existe con la misma moneda (variante de representación)
        //    o como crudo sin par de moneda confirmado.
        if correction.kind == CorrectionKind::NoEspecificado && !locs_found.is_empty() {
            correction.kind = CorrectionKind::Representacion;
            correction.source = Some(format!(
                "{} pág. {}",
                locs_found[0].document, locs_found[0].page
            ));
            correction.snippet = Some(locs_found[0].section_snippet.clone());
            correction.suggested_value = Some(locs_found[0].section_snippet.clone());
        }

        // 3) Fallback: no existe en el corpus → ausencia (NoEspecificado).
        corrections.push(correction);
    }

    // Números crudos alucinados.
    for num in hallucinated_numbers {
        if context_raw.contains(num) {
            // Existe en el contexto: no es alucinación de existencia, el modelo
            // se atribuyó mal. Advertir sin sugerir un valor.
            corrections.push(Correction {
                kind: CorrectionKind::EnCorpus,
                hallucinated: num.clone(),
                suggested_value: None,
                source: None,
                snippet: None,
            });
            continue;
        }
        let mut correction = Correction {
            kind: CorrectionKind::NoEspecificado,
            hallucinated: num.clone(),
            suggested_value: None,
            source: None,
            snippet: None,
        };
        if let Some(idx) = index {
            if let Some(locs) = idx.get(num) {
                correction.kind = CorrectionKind::EnCorpus;
                correction.source = Some(format!("{} pág. {}", locs[0].document, locs[0].page));
                correction.snippet = Some(locs[0].section_snippet.clone());
            }
        }
        corrections.push(correction);
    }

    // Porcentajes alucinados.
    for p in hallucinated_percent {
        corrections.push(Correction {
            kind: CorrectionKind::NoEspecificado,
            hallucinated: p.canon.clone(),
            suggested_value: None,
            source: None,
            snippet: Some(p.raw_text.clone()),
        });
    }

    corrections
}

/// Convierte las correcciones en un prompt de reintento dirigido (≤ ~250 tokens).
pub fn build_correction_prompt(corrections: &[Correction], attempt: usize, retries: usize) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "\n\n🚨 [AUDITORÍA NUMÉRICA - REINTENTO {}/{}]:",
        attempt, retries
    ));
    lines.push("Tu respuesta anterior incluyó cifras NO verificadas:".to_string());

    for c in corrections.iter().take(5) {
        match c.kind {
            CorrectionKind::NoEspecificado => {
                lines.push(format!(
                    "  - '{}': no existe en ningún documento → responde '[Dato no especificado en las fuentes]'.",
                    c.hallucinated
                ));
            }
            CorrectionKind::Moneda => {
                let val = c.suggested_value.as_deref().unwrap_or("");
                let src = c.source.as_deref().unwrap_or("");
                lines.push(format!(
                    "  - '{}': la moneda es incorrecta. El dato correcto en el documento es '{}' [{}].",
                    c.hallucinated, val, src
                ));
            }
            CorrectionKind::Representacion => {
                let src = c.source.as_deref().unwrap_or("");
                lines.push(format!(
                    "  - '{}': usa la cifra literal del documento [{}]. NO la conviertas ni cambies el formato.",
                    c.hallucinated, src
                ));
            }
            CorrectionKind::EnCorpus => {
                let src = c.source.as_deref().unwrap_or("el contexto recuperado");
                lines.push(format!(
                    "  - '{}': la cifra existe en [{}] pero verifica a qué concepto corresponde (posible atribución errónea).",
                    c.hallucinated, src
                ));
            }
        }
    }

    lines.push(
        "INSTRUCCIÓN: copia literalmente las cifras verificadas con su moneda y formato del documento; NO conviertas ni inventes."
            .to_string(),
    );

    lines.join("\n")
}

/// Fase 2: bucle de autocorrección con feedback grounded y corte de rebote.
/// Fase 4: verificación de atribución por indicador; con `strict_attribution`
///         los hallazgos de segmento también invalidan la respuesta.
pub async fn query_with_self_correction(
    client: &LlmClient,
    system_prompt: &str,
    context: &str,
    question: &str,
    max_retries: usize,
    corpus: Option<&Corpus>,
    strict_attribution: bool,
) -> Result<GuardrailResponse, String> {
    let overall_start = Instant::now();
    let mut current_user_prompt = crate::llm::build_context_prompt(&[context.to_string()], question);
    let mut last_response = String::new();
    let mut last_raw = String::new();
    let mut prev_response = String::new();
    let mut last_hallucinations: Vec<String> = Vec::new();
    let mut last_money_hallucinations: Vec<String> = Vec::new();
    let mut last_attribution: Vec<AttributionFinding> = Vec::new();
    let mut total_guardrail_micros = 0u128;
    let mut auto_corrected = false;
    let mut attempt_log: Vec<AttemptInfo> = Vec::new();
    let mut final_corrections: Vec<Correction> = Vec::new();
    let mut prev_rebote_key: Option<String> = None;

    let retries = max_retries.max(1);

    for attempt in 1..=retries {
        let resp = client.generate(system_prompt, &current_user_prompt).await?;
        last_raw = resp.clone();

        // Eco del prompt de corrección: modelos pequeños a veces regurgitan
        // la instrucción de reintento ("Tu respuesta anterior incluyó cifras NO
        // verificadas") en lugar de generar contenido nuevo. Ese texto no es una
        // respuesta: se corta el bucle y se entrega la última respuesta REAL del
        // modelo (sanitizada); el eco queda solo como raw_response de diagnóstico.
        const CORRECTION_MARKER: &str = "Tu respuesta anterior incluyó cifras NO verificadas";
        if resp.contains(CORRECTION_MARKER) || resp.contains("AUDITORÍA NUMÉRICA") {
            last_response = if prev_response.is_empty() {
                resp
            } else {
                prev_response
            };
            let v_echo = verify_numeric_grounding(&last_response, context, question);
            total_guardrail_micros += v_echo.latency_micros;
            last_hallucinations = v_echo.hallucinated_numbers.iter().cloned().collect();
            last_money_hallucinations = v_echo
                .hallucinated_money
                .iter()
                .map(|m| format!("{}|{}", m.currency, m.value))
                .collect();
            break;
        }

        prev_response = resp.clone();
        last_response = resp.clone();

        let verification = verify_numeric_grounding(&resp, context, question);
        total_guardrail_micros += verification.latency_micros;

        // Fase 4: verificación de atribución (emisor × indicador) sobre el corpus.
        let mut attribution_findings: Vec<AttributionFinding> = Vec::new();
        if let Some(corpus_ref) = corpus {
            attribution_findings = verify_attribution(&resp, corpus_ref);
        }
        let attribution_valid = attribution_findings.iter().all(|f| {
            !f.is_misattribution && !(strict_attribution && f.segment_qualifier)
        });

        let mut nums: Vec<String> = verification.hallucinated_numbers.iter().cloned().collect();
        nums.sort();
        let money: Vec<String> = verification
            .hallucinated_money
            .iter()
            .map(|m| format!("{}|{}", m.currency, m.value))
            .collect();

        // Corte de rebote: mismo set de alucinaciones + hallazgos de atribución
        // que el intento anterior.
        let attribution_key: Vec<String> = attribution_findings
            .iter()
            .map(|f| {
                format!(
                    "{}|{}|{}|{}|{}",
                    f.issuer,
                    f.indicator,
                    f.claimed_currency,
                    f.claimed_value,
                    f.is_misattribution || f.segment_qualifier
                )
            })
            .collect();
        let rebote_key = format!("{:?}|{:?}|{:?}", nums, money, attribution_key);
        if attempt > 1 && prev_rebote_key.as_deref() == Some(rebote_key.as_str()) {
            last_hallucinations = nums;
            last_money_hallucinations = money;
            last_attribution = attribution_findings;
            break;
        }
        prev_rebote_key = Some(rebote_key);

        if verification.is_valid && attribution_valid {
            let latency_ms = overall_start.elapsed().as_secs_f64() * 1000.0;
            return Ok(GuardrailResponse {
                raw_response: last_raw.clone(),
                final_response: last_response,
                is_valid: true,
                attempts: attempt,
                hallucinated_numbers: vec![],
                hallucinated_money: vec![],
                corrections: vec![],
                attempt_log,
                latency_ms,
                guardrail_latency_micros: total_guardrail_micros,
                auto_corrected: attempt > 1,
                attribution: attribution_findings,
            });
        }

        last_hallucinations = nums.clone();
        last_money_hallucinations = money.clone();
        last_attribution = attribution_findings;

        // Correcciones grounded para el siguiente intento.
        final_corrections = find_corrections(
            &nums,
            &verification.hallucinated_money,
            &verification.hallucinated_percent,
            corpus,
            context,
        );

        if attempt < retries {
            auto_corrected = true;
            let correction_prompt = build_correction_prompt(&final_corrections, attempt, retries);
            current_user_prompt.push_str(&correction_prompt);
            // Fase 4: líneas de atribución en el prompt de reintento.
            for f in &last_attribution {
                if f.is_misattribution {
                    current_user_prompt.push_str(&format!(
                        "\n  - ⚠ ATRIBUCIÓN: verifica la atribución del indicador '{}' en el emisor '{}': la cifra '{} {}' no aparece en el documento para ese indicador{}",
                        f.indicator,
                        f.issuer,
                        f.claimed_currency,
                        f.claimed_value,
                        match &f.correct_value {
                            Some(v) => format!(". El único valor en el corpus es '{}'", v),
                            None => " (hay varios valores posibles, no inventes el correcto)".to_string(),
                        }
                    ));
                }
                if strict_attribution && f.segment_qualifier {
                    current_user_prompt.push_str(&format!(
                        "\n  - ⚠ SEGMENTO: la cifra '{} {}' de '{}' en '{}' pertenece a un segmento de negocio (ej. gran minería), NO a los totales. No la reportes como total.",
                        f.claimed_currency, f.claimed_value, f.indicator, f.issuer
                    ));
                }
            }
            attempt_log.push(AttemptInfo {
                attempt,
                is_valid: false,
                hallucinated_numbers: nums.clone(),
                hallucinated_money: money.clone(),
                corrections_sent: final_corrections
                    .iter()
                    .map(|c| c.hallucinated.clone())
                    .collect(),
            });
        }
    }

    // Sanitización final.
    let hall_set: HashSet<String> = last_hallucinations.iter().cloned().collect();
    let money_hall: Vec<MoneyPhrase> = final_corrections
        .iter()
        .filter_map(|c| {
            if matches!(c.kind, CorrectionKind::Moneda | CorrectionKind::Representacion) {
                Some(MoneyPhrase {
                    currency: String::new(),
                    base_digits: String::new(),
                    value: c.hallucinated.clone(),
                    raw_text: c.snippet.clone().unwrap_or_default(),
                })
            } else {
                None
            }
        })
        .collect();
    let sanitized_final = sanitize_hallucinations_full(&last_response, &hall_set, &money_hall, &[]);
    let latency_ms = overall_start.elapsed().as_secs_f64() * 1000.0;

    Ok(GuardrailResponse {
        raw_response: last_raw,
        final_response: sanitized_final,
        is_valid: false,
        attempts: retries,
        hallucinated_numbers: last_hallucinations,
        hallucinated_money: last_money_hallucinations,
        corrections: final_corrections,
        attempt_log,
        latency_ms,
        guardrail_latency_micros: total_guardrail_micros,
        auto_corrected,
        attribution: last_attribution,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_valid_grounded() {
        let context = "Ferreycorp reportó ventas por 59,968 unidades y un margen de 14.8% en 2025.";
        let question = "¿Cuáles fueron las ventas en 2025?";
        let response = "En 2025, Ferreycorp reportó 59,968 unidades vendidas con un margen de 14.8%.";

        let _ = verify_numeric_grounding("1", "1", "1");
        let result = verify_numeric_grounding(response, context, question);
        assert!(result.is_valid);
        assert!(result.hallucinated_numbers.is_empty());
        assert!(result.latency_micros < 5000);
    }

    #[test]
    fn test_verify_hallucination_raw() {
        let context = "Ferreycorp reportó ventas por 59,968 unidades en 2025.";
        let question = "¿Cuáles fueron las ventas?";
        let response = "Ferreycorp vendió 99,999 unidades y obtuvo una ganancia de 750 millones.";

        let result = verify_numeric_grounding(response, context, question);
        assert!(!result.is_valid);
        assert!(result.hallucinated_numbers.contains("99999"));
        assert!(result.hallucinated_numbers.contains("750"));
    }

    #[test]
    fn test_p1_partial_token_not_grounded() {
        // 32.2% en contexto NO permite un 32 suelto en la respuesta.
        let context = "La tasa de pobreza fue 32.2% en 2024.";
        let question = "¿Cuál fue la tasa de pobreza?";
        let response = "La tasa fue de 32.";
        let result = verify_numeric_grounding(response, context, question);
        assert!(!result.is_valid, "32 no debe estar respaldado por 32.2");
        assert!(result.hallucinated_numbers.contains("32"));
    }

    #[test]
    fn test_p2_money_currency_mixup_detected() {
        // Contexto: patrimonio en soles; respuesta lo pone en dólares.
        let context = "El patrimonio superó S/. 405 millones y las ventas fueron US$ 2,177 millones.";
        let question = "¿Cuál fue el patrimonio?";
        let response = "El patrimonio superó US$ 405 millones.";
        let result = verify_numeric_grounding(response, context, question);
        assert!(!result.is_valid, "US$ 405 millones debe marcarse (mix-up de moneda)");
        assert!(!result.hallucinated_money.is_empty());
    }

    #[test]
    fn test_p3_money_alias_equivalent() {
        let context = "Ferreycorp reportó US$ 2,177 millones.";
        let question = "¿Ventas en dólares?";
        let response = "Las ventas fueron $2,177 millones.";
        let result = verify_numeric_grounding(response, context, question);
        assert!(result.is_valid, "$2,177 ≡ US$ 2,177");
    }

    #[test]
    fn test_p4_scale_variant_equivalent() {
        let context = "El patrimonio superó S/. 405 millones.";
        let question = "¿Patrimonio?";
        let response = "El patrimonio superó S/. 405,000,000.";
        let result = verify_numeric_grounding(response, context, question);
        assert!(result.is_valid, "S/. 405,000,000 ≡ S/. 405 millones");
    }

    #[test]
    fn test_percent_textual_equivalent() {
        let context = "GDP growth is expected to be 2.7 percent in 2024.";
        let question = "¿Crecimiento del PBI en 2024?";
        let response = "El PBI creció 2.7% en 2024.";
        let result = verify_numeric_grounding(response, context, question);
        assert!(result.is_valid, "2.7 percent ≡ 2.7%");
    }

    #[test]
    fn test_sanitize_by_phrase() {
        let text = "El patrimonio de US$ 32 mil millones fue reportado.";
        let mut bad = HashSet::new();
        bad.insert("32".to_string());
        let money_hall = vec![MoneyPhrase {
            currency: "usd".to_string(),
            base_digits: "32".to_string(),
            value: "32000000000".to_string(),
            raw_text: "US$ 32 mil millones".to_string(),
        }];
        let sanitized = sanitize_hallucinations_full(text, &bad, &money_hall, &[]);
        assert!(
            sanitized.contains("~~US$ 32 mil millones~~"),
            "la frase completa debe tacharse: {sanitized}"
        );
        // El dígito suelto dentro de la frase no debe tacharse dos veces.
        assert_eq!(sanitized.matches("~~32~~").count(), 0);
    }

    #[test]
    fn test_find_corrections_moneda_mixup() {
        use crate::ingest::{InvertedNumberIndex, SourceLocation};

        let mut index: InvertedNumberIndex = HashMap::new();
        index.insert(
            "405".to_string(),
            vec![SourceLocation {
                document: "Memoria-Anual-2025-Financiera-Efectiva.md".to_string(),
                page: 3,
                section_snippet: "patrimonio que superó los S/. 405 millones de soles".to_string(),
            }],
        );
        let corpus = Corpus {
            number_index: index,
            ..Corpus::default()
        };

        // El modelo alucina "US$ 405 millones" (moneda incorrecta).
        let m = MoneyPhrase {
            currency: "usd".to_string(),
            base_digits: "405".to_string(),
            value: "405000000".to_string(),
            raw_text: "US$ 405 millones".to_string(),
        };
        let corrections = find_corrections(&[], &[m], &[], Some(&corpus), "");
        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].kind, CorrectionKind::Moneda);
        assert_eq!(corrections[0].suggested_value.as_deref(), Some("pen|405000000"));
        assert!(corrections[0].source.as_deref().unwrap_or("").contains("Financiera"));
    }

    #[test]
    fn test_find_corrections_ausencia() {
        // Número que no existe en ningún lado → NoEspecificado.
        let m = MoneyPhrase {
            currency: "usd".to_string(),
            base_digits: "32".to_string(),
            value: "32000000000".to_string(),
            raw_text: "US$ 32 mil millones".to_string(),
        };
        let corrections = find_corrections(&[], &[m], &[], None, "");
        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].kind, CorrectionKind::NoEspecificado);
    }

    #[test]
    fn test_rebote_key_stable() {
        // El rebote_key debe ser determinista entre intentos idénticos.
        let ctx = "Ventas S/. 450 millones en 2025.";
        let r1 = verify_numeric_grounding("Ventas de 999 millones.", ctx, "¿Ventas?");
        let r2 = verify_numeric_grounding("Ventas de 999 millones.", ctx, "¿Ventas?");
        let mut n1: Vec<String> = r1.hallucinated_numbers.iter().cloned().collect();
        let mut n2: Vec<String> = r2.hallucinated_numbers.iter().cloned().collect();
        n1.sort();
        n2.sort();
        assert_eq!(n1, n2);
    }
}
