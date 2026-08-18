use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use regex::Regex;

static WORD_BOUNDARY_CACHE: LazyLock<HashMap<&'static str, Regex>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for &alias in ALL_ALIASES.iter() {
        let escaped = regex::escape(alias);
        if let Ok(re) = Regex::new(&format!(r"(?i)\b{}\b", escaped)) {
            map.insert(alias, re);
        }
    }
    map
});

pub const ALL_ALIASES: &[&str] = &[
    "roe", "retorno sobre patrimonio", "retorno sobre el patrimonio", "return on equity", "rentabilidad sobre el patrimonio",
    "roa", "retorno sobre activos", "retorno sobre los activos", "return on assets", "rentabilidad sobre activos",
    "roic", "retorno sobre capital invertido", "retorno sobre el capital invertido",
    "deuda ebitda", "deuda / ebitda", "deuda/ebitda", "deuda neta / ebitda", "deuda neta/ebitda", "leverage", "ratio de apalancamiento", "deuda financiera / ebitda",
    "ebitda", "ebitda ajustado", "ebitda consolidado", "ebitda acumulado",
    "utilidad neta", "ganancia neta", "resultado neto", "net income", "utilidad del ejercicio", "beneficio neto",
    "utilidad operativa", "resultado operativo", "ganancia operativa", "operating income", "ebit",
    "apalancamiento", "ratio de deuda", "endeudamiento", "cobertura de intereses", "solvencia", "ratio de apalancamiento financiero",
    "liquidez", "ratio corriente", "prueba acida", "capital de trabajo", "liquidez corriente", "liquidez general",
    "flujo de caja", "flujo de efectivo", "cash flow", "flujo libre de caja", "fcf", "flujo operativo",
    "margen bruto", "gross margin", "margen operativo", "operating margin", "margen ebitda", "margen neto",
    "ventas", "ventas netas", "ingresos", "sales", "revenue", "ingresos totales", "ingresos de actividades ordinarias",
    "morosidad", "cartera atrasada", "cartera pesada", "npl", "provisiones",
    "riesgos", "riesgo de credito", "riesgo cambiario", "riesgo de tasa", "riesgo de liquidez",
    "pobreza", "poverty", "macroeconomico", "macroeconomic", "gdp", "pbi",
];

pub struct IndicatorSpec {
    pub canon: &'static str,
    pub aliases: &'static [&'static str],
}

pub const FINANCIAL_INDICATORS: &[IndicatorSpec] = &[
    IndicatorSpec {
        canon: "ROE",
        aliases: &["roe", "retorno sobre patrimonio", "retorno sobre el patrimonio", "return on equity", "rentabilidad sobre el patrimonio"],
    },
    IndicatorSpec {
        canon: "ROA",
        aliases: &["roa", "retorno sobre activos", "retorno sobre los activos", "return on assets", "rentabilidad sobre activos"],
    },
    IndicatorSpec {
        canon: "ROIC",
        aliases: &["roic", "retorno sobre capital invertido", "retorno sobre el capital invertido"],
    },
    IndicatorSpec {
        canon: "deuda_ebitda",
        aliases: &["deuda ebitda", "deuda / ebitda", "deuda/ebitda", "deuda neta / ebitda", "deuda neta/ebitda", "leverage", "ratio de apalancamiento", "deuda financiera / ebitda"],
    },
    IndicatorSpec {
        canon: "EBITDA",
        aliases: &["ebitda", "ebitda ajustado", "ebitda consolidado", "ebitda acumulado"],
    },
    IndicatorSpec {
        canon: "utilidad_neta",
        aliases: &["utilidad neta", "ganancia neta", "resultado neto", "net income", "utilidad del ejercicio", "beneficio neto"],
    },
    IndicatorSpec {
        canon: "utilidad_operativa",
        aliases: &["utilidad operativa", "resultado operativo", "ganancia operativa", "operating income", "ebit"],
    },
    IndicatorSpec {
        canon: "apalancamiento",
        aliases: &["apalancamiento", "ratio de deuda", "endeudamiento", "cobertura de intereses", "solvencia", "ratio de apalancamiento financiero"],
    },
    IndicatorSpec {
        canon: "liquidez",
        aliases: &["liquidez", "ratio corriente", "prueba acida", "capital de trabajo", "liquidez corriente", "liquidez general"],
    },
    IndicatorSpec {
        canon: "flujo_caja",
        aliases: &["flujo de caja", "flujo de efectivo", "cash flow", "flujo libre de caja", "fcf", "flujo operativo"],
    },
    IndicatorSpec {
        canon: "margen_bruto",
        aliases: &["margen bruto", "gross margin"],
    },
    IndicatorSpec {
        canon: "margen_operativo",
        aliases: &["margen operativo", "operating margin"],
    },
    IndicatorSpec {
        canon: "ventas",
        aliases: &["ventas", "ventas netas", "ingresos", "sales", "revenue", "ingresos totales", "ingresos de actividades ordinarias"],
    },
    IndicatorSpec {
        canon: "morosidad",
        aliases: &["morosidad", "cartera atrasada", "cartera pesada", "npl", "provisiones"],
    },
    IndicatorSpec {
        canon: "riesgos",
        aliases: &["riesgos", "riesgo de credito", "riesgo cambiario", "riesgo de tasa", "riesgo de liquidez"],
    },
];

pub const QUERY_ASPECTS: &[(&str, &str)] = &[
    ("rentabilidad", "margen bruto, margen operativo, ROE, ROA y utilidad neta"),
    ("liquidez", "capital de trabajo, razon corriente y flujo de caja operativo"),
    ("solvencia", "deuda total, deuda neta sobre EBITDA y apalancamiento financiero"),
    ("ingresos", "ventas netas, ingresos por linea de negocio y volumen de operaciones"),
    ("calidad_activos", "morosidad, provisiones para colocaciones y ratio de cobertura"),
    ("eficiencia", "gastos de administracion, ratio de eficiencia y costo de ventas"),
    ("riesgos", "riesgo de tipo de cambio, tasa de interes y exposicion macroeconomica"),
    ("inversiones", "capex, adquisicion de activos fijos y proyectos de expansion"),
    ("pobreza", "poverty rate Gini index macroeconomic conditions GDP growth"),
    ("macroecon", "macroeconomic conditions GDP growth poverty reduction inflation"),
];

pub const CROSS_LINGUAL_EXPANSIONS: &[(&str, &str)] = &[
    ("pobreza", "poverty"),
    ("macroeconómicos", "macroeconomic"),
    ("macroeconomicos", "macroeconomic"),
    ("crecimiento", "GDP growth"),
    ("inflación", "inflation"),
    ("inflacion", "inflation"),
    ("empleo", "employment"),
    ("desempleo", "unemployment"),
    ("inversión", "investment"),
    ("inversion", "investment"),
    ("banco mundial", "World Bank Macro Poverty Outlook"),
];

pub fn get_indicator_synonyms(canon_name: &str) -> Vec<String> {
    for spec in FINANCIAL_INDICATORS {
        if spec.canon.eq_ignore_ascii_case(canon_name) {
            return spec.aliases.iter().map(|&s| s.to_string()).collect();
        }
    }
    vec![]
}

pub fn all_indicator_aliases() -> Vec<String> {
    ALL_ALIASES.iter().map(|&s| s.to_string()).collect()
}

pub fn contains_alias(text_low: &str, alias_low: &str) -> bool {
    if let Some(re) = WORD_BOUNDARY_CACHE.get(alias_low) {
        re.is_match(text_low)
    } else {
        let pattern = format!(r"(?i)\b{}\b", regex::escape(alias_low));
        if let Ok(re) = Regex::new(&pattern) {
            re.is_match(text_low)
        } else {
            text_low.contains(alias_low)
        }
    }
}

pub fn find_matched_indicators(text: &str) -> Vec<String> {
    let text_low = text.to_lowercase();
    let mut matched = Vec::new();
    let mut seen = HashSet::new();

    for spec in FINANCIAL_INDICATORS {
        for &alias in spec.aliases {
            if contains_alias(&text_low, alias) {
                if seen.insert(spec.canon) {
                    matched.push(spec.canon.to_string());
                }
                break;
            }
        }
    }

    matched.sort_by_key(|s| std::cmp::Reverse(s.len()));
    matched.truncate(3);
    matched
}

pub fn match_indicator_name(row_label: &str) -> Option<String> {
    let label_low = row_label.trim().to_lowercase();
    if label_low.is_empty() {
        return None;
    }

    for spec in FINANCIAL_INDICATORS {
        for &alias in spec.aliases {
            if contains_alias(&label_low, alias) || label_low == alias {
                return Some(spec.canon.to_string());
            }
        }
    }
    None
}

pub fn expand_financial_queries(question: &str) -> Vec<String> {
    let q_clean = question.trim();
    if q_clean.is_empty() {
        return vec![];
    }

    let q_low = q_clean.to_lowercase();
    let mut out: Vec<String> = vec![q_clean.to_string()];
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(q_low.clone());

    // 1. Variantes por indicadores específicos detectados
    let matched = find_matched_indicators(q_clean);
    for canon in &matched {
        let syns = get_indicator_synonyms(canon);
        for syn in syns.iter().take(2) {
            let variant = format!("{} ({})", q_clean, syn);
            let v_low = variant.to_lowercase();
            if seen.insert(v_low) {
                out.push(variant);
            }
        }
    }

    // 2. Variantes bilingües / cross-lingual
    let mut english_tokens = Vec::new();
    for &(es_term, en_term) in CROSS_LINGUAL_EXPANSIONS {
        if q_low.contains(es_term) {
            english_tokens.push(en_term);
        }
    }
    if !english_tokens.is_empty() {
        let en_variant = format!("{} {}", q_clean, english_tokens.join(" "));
        let v_low = en_variant.to_lowercase();
        if seen.insert(v_low) {
            out.push(en_variant);
        }
    }

    // 3. Variantes por aspectos financieros amplios
    for &(aspect, aspect_desc) in QUERY_ASPECTS {
        if q_low.contains(aspect) {
            let variant = format!("{}: detalle de {}", q_clean, aspect_desc);
            let v_low = variant.to_lowercase();
            if seen.insert(v_low) {
                out.push(variant);
            }
        }
    }

    out.truncate(5);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indicator_matching() {
        let text = "El ratio deuda neta / EBITDA de la empresa mejoró significativamente en 2025.";
        let matched = find_matched_indicators(text);
        assert!(matched.contains(&"deuda_ebitda".to_string()) || matched.contains(&"EBITDA".to_string()));
    }

    #[test]
    fn test_row_matching() {
        assert_eq!(match_indicator_name("Utilidad Neta del Ejercicio"), Some("utilidad_neta".to_string()));
        assert_eq!(match_indicator_name("ROE"), Some("ROE".to_string()));
        assert_eq!(match_indicator_name("Total Activos"), None);
    }

    #[test]
    fn test_expand_queries() {
        let expanded = expand_financial_queries("¿Cuáles fueron los principales factores macroeconómicos y de pobreza según el Banco Mundial?");
        assert!(!expanded.is_empty());
        assert!(expanded.iter().any(|v| v.contains("poverty") || v.contains("macroeconomic")));
    }
}
