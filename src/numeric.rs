//! Extracción numérica de alta fidelidad para el Guardrail v3.
//!
//! Porta a Rust las correcciones de validación numérica del harness Python:
//! - Tokenización completa (`32.2` → `322`, nunca `32` suelto).
//! - Frases monetarias `(moneda_canónica, valor_canónico)` con alias
//!   (`$`/`US$`/`USD` → `usd`; `S/`/`S./`/`PEN` → `pen`).
//! - Canonicalización con multiplicadores (`405 millones` → `405000000`),
//!   unidireccional: expande solo cuando la palabra multiplicadora existe.
//! - Porcentajes en forma simbólica (`2.7%`) y textual (`2.7 percent`,
//!   `2.7 por ciento`).
//! - Soporte del apóstrofe tipográfico (U+2019 y ASCII) como separador de
//!   miles, presente en el corpus peruano (`S/ 1'938,305 miles`).

use std::collections::HashSet;
use std::sync::LazyLock;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Token numérico completo: `32.2`, `1,000.50`, `1'938,305`, `2,177`, `3.5x`, `14.8%`.
static NUM_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d[\d.,'\u{2019}]*[xXmMkK%]?\b").unwrap()
});

/// Frase monetaria: moneda + número + multiplicador opcional.
static MONEY_PHRASE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(us\s*\$|usd|pen|s\s*/\.?|\$)\s*\.?\s*(\d[\d.,'\u{2019}]*)(?:\s*(mil millones|billones|billón|billon|millones|millón|millon|million|miles|mil|billion))?",
    )
    .unwrap()
});

/// Porcentaje: símbolo o forma textual.
static PCT_PHRASE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\d[\d.,'\u{2019}]*)\s*(?:%|percent|por ciento)").unwrap()
});

/// Año de 4 dígitos (19xx–21xx), categoría tolerada siempre.
static YEAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:19|20|21)\d{2}\b").unwrap()
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoneyPhrase {
    /// Moneda canónica: `usd` | `pen` | otra literal.
    pub currency: String,
    /// Dígitos base sin multiplicador (para fallback crudo).
    pub base_digits: String,
    /// Valor canónico completo (base × multiplicador).
    pub value: String,
    /// Texto crudo coincidente (para sanitización por frase).
    pub raw_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PercentPhrase {
    /// Clave canónica: `%|<dígitos>`.
    pub canon: String,
    /// Dígitos base.
    pub base_digits: String,
    /// Texto crudo coincidente.
    pub raw_text: String,
}

fn multiplier_value(word: &str) -> u128 {
    match word.to_lowercase().as_str() {
        "mil millones" => 1_000_000_000,
        "billones" | "billón" | "billon" => 1_000_000_000_000,
        "millones" | "millón" | "millon" | "million" => 1_000_000,
        "miles" | "mil" => 1_000,
        "billion" => 1_000_000_000,
        _ => 1,
    }
}

fn digits_only(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
}

fn normalize_currency(raw: &str) -> String {
    let low = raw.to_lowercase();
    let tight: String = low.chars().filter(|c| !c.is_whitespace()).collect();
    match tight.as_str() {
        "$" | "us$" | "usd" | "us" => "usd".to_string(),
        "s/" | "s/." | "s" | "pen" => "pen".to_string(),
        _ => tight,
    }
}

/// Extrae frases monetarias de un texto.
pub fn extract_money_phrases(text: &str) -> Vec<MoneyPhrase> {
    let mut out = Vec::new();
    for cap in MONEY_PHRASE_RE.captures_iter(text) {
        let Some(cur_raw) = cap.get(1) else { continue };
        let Some(num_raw) = cap.get(2) else { continue };
        let mult_raw = cap.get(3).map(|m| m.as_str());

        let digits = digits_only(num_raw.as_str());
        if digits.is_empty() {
            continue;
        }
        let base: u128 = match digits.parse::<u128>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let value = base
            .saturating_mul(mult_raw.map(multiplier_value).unwrap_or(1))
            .to_string();

        out.push(MoneyPhrase {
            currency: normalize_currency(cur_raw.as_str()),
            base_digits: digits,
            value,
            raw_text: cap.get(0).map(|m| m.as_str()).unwrap_or_default().to_string(),
        });
    }
    out
}

/// Extrae frases de porcentaje (símbolo o forma textual).
pub fn extract_percent_phrases(text: &str) -> Vec<PercentPhrase> {
    let mut out = Vec::new();
    for cap in PCT_PHRASE_RE.captures_iter(text) {
        let Some(num_raw) = cap.get(1) else { continue };
        let digits = digits_only(num_raw.as_str());
        if digits.is_empty() {
            continue;
        }
        out.push(PercentPhrase {
            canon: format!("%|{digits}"),
            base_digits: digits,
            raw_text: cap.get(0).map(|m| m.as_str()).unwrap_or_default().to_string(),
        });
    }
    out
}

/// Números crudos normalizados a dígitos (`32.2` → `322`).
pub fn extract_raw_numbers(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for mat in NUM_TOKEN_RE.find_iter(text) {
        let digits = digits_only(mat.as_str());
        if !digits.is_empty() {
            out.insert(digits);
        }
    }
    out
}

/// Años de 4 dígitos presentes en el texto.
pub fn extract_years(text: &str) -> HashSet<String> {
    YEAR_RE
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// ¿El token es un año plausible (1900–2199)?
pub fn is_year(digits: &str) -> bool {
    matches!(digits.len(), 4) && matches!(&digits[..2], "19" | "20" | "21")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn money_key(p: &MoneyPhrase) -> String {
        format!("{}|{}", p.currency, p.value)
    }

    #[test]
    fn test_token_complete_not_partial() {
        let nums = extract_raw_numbers("tasa de pobreza 32.2%");
        assert!(!nums.contains("32"), "32 no debe derivarse de 32.2");
        assert!(nums.contains("322"));
    }

    #[test]
    fn test_apostrophe_thousands() {
        let nums = extract_raw_numbers("S/ 1'938,305 miles de activos");
        assert!(nums.contains("1938305"), "apóstrofe U+2019 debe separar miles");
        assert!(!nums.contains("1"));
    }

    #[test]
    fn test_money_alias_usd() {
        let ctx = "Ferreycorp reportó US$ 2,177 millones y también $2,177 millones.";
        let phrases = extract_money_phrases(ctx);
        let keys: HashSet<String> = phrases.iter().map(money_key).collect();
        assert!(keys.contains("usd|2177000000"));
        assert_eq!(phrases.len(), 2, "US$ y $ deben normalizar al mismo par");
    }

    #[test]
    fn test_money_alias_pen() {
        let ctx = "El patrimonio superó S/. 405 millones y S/ 405,000,000.";
        let phrases = extract_money_phrases(ctx);
        let keys: HashSet<String> = phrases.iter().map(money_key).collect();
        assert!(keys.contains("pen|405000000"));
        assert_eq!(phrases.len(), 2);
    }

    #[test]
    fn test_money_scale_variant_equivalent() {
        let ctx = "Las ventas fueron S/ 7,798 millones (US$ 2,177 millones).";
        let phrases = extract_money_phrases(ctx);
        let keys: HashSet<String> = phrases.iter().map(money_key).collect();
        assert!(keys.contains("pen|7798000000"));
        assert!(keys.contains("usd|2177000000"));
    }

    #[test]
    fn test_percent_symbol_and_text_equivalent() {
        let a = extract_percent_phrases("el PBI creció 2.7%");
        let b = extract_percent_phrases("GDP growth is expected to be 2.7 percent");
        assert_eq!(a[0].canon, b[0].canon, "%|27");
        let c = extract_percent_phrases("el crecimiento fue de 2.7 por ciento");
        assert_eq!(a[0].canon, c[0].canon);
    }

    #[test]
    fn test_years() {
        assert!(is_year("2025"));
        assert!(is_year("1998"));
        assert!(!is_year("405"));
        assert!(!is_year("999"));
        let ys = extract_years("Entre 2024 y 2025 el PBI creció; en 1998 hubo crisis.");
        assert_eq!(ys.len(), 3);
    }
}
