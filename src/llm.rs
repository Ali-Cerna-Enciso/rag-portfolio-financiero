use std::sync::LazyLock;
use std::time::Duration;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};

static THINK_BLOCK_RES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?is)<think>.*?</think\s*>").unwrap(),
        Regex::new(r"(?is)<thought>.*?</thought\s*>").unwrap(),
        Regex::new(r"(?is)<reasoning>.*?</reasoning\s*>").unwrap(),
    ]
});

static STRAY_THINK_CLOSER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)^.*?</think\s*>").unwrap()
});

pub fn strip_think_blocks(text: &str) -> String {
    let mut cleaned = text.to_string();
    for re in THINK_BLOCK_RES.iter() {
        cleaned = re.replace_all(&cleaned, "").to_string();
    }
    cleaned = STRAY_THINK_CLOSER.replace(&cleaned, "").to_string();
    cleaned.trim().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: Option<usize>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: Option<String>,
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub api_base: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: usize,
    pub timeout_secs: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_base: "http://127.0.0.1:8080".to_string(),
            model: "qwen2.5-3b-instruct-q4_k_m.gguf".to_string(),
            temperature: 0.1,
            max_tokens: 1024,
            timeout_secs: 120,
        }
    }
}

pub struct LlmClient {
    pub config: LlmConfig,
    client: Client,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { config, client }
    }

    pub fn resolve_model_alias(alias: &str) -> String {
        let low = alias.to_lowercase();
        if low.contains("2.5") || low.contains("3b") {
            "qwen2.5-3b-instruct-q4_k_m.gguf".to_string()
        } else if low.contains("3.8-2b") || (low.contains("2b") && !low.contains("3b")) {
            "Qwen3.8-2B-Q4_K_M.gguf".to_string()
        } else if low.contains("3.8-4b") || low.contains("4b") {
            "Qwen3.8-4B-Q4_K_M.gguf".to_string()
        } else {
            alias.to_string()
        }
    }

    pub async fn check_health(&self) -> bool {
        let url = format!("{}/health", self.config.api_base.trim_end_matches('/'));
        if let Ok(resp) = self.client.get(&url).send().await {
            resp.status().is_success()
        } else {
            // Check fallback /v1/models
            let models_url = format!("{}/v1/models", self.config.api_base.trim_end_matches('/'));
            self.client.get(&models_url).send().await.map_or(false, |r| r.status().is_success())
        }
    }

    pub async fn chat_complete(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Result<String, String> {
        let endpoint = format!("{}/v1/chat/completions", self.config.api_base.trim_end_matches('/'));

        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages,
            temperature: self.config.temperature,
            max_tokens: Some(self.config.max_tokens),
            stream: false,
        };

        let res = self.client.post(&endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Fallo de conexión con llama-server ({}): {}", endpoint, e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            return Err(format!("Error del servidor LLM (HTTP {}): {}", status, err_body));
        }

        let resp: ChatCompletionResponse = res.json()
            .await
            .map_err(|e| format!("Error decodificando respuesta JSON del LLM: {}", e))?;

        let raw_content = resp.choices.first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        let cleaned = strip_think_blocks(&raw_content);
        Ok(cleaned)
    }

    pub async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            },
        ];
        self.chat_complete(messages).await
    }
}

pub fn build_system_prompt() -> &'static str {
    "Eres un analista financiero sénior especializado en memorias anuales y reportes estadísticos del mercado peruano.\n\
    Tu misión es responder con máxima fidelidad, precisión ejecutiva y estricto apego a las fuentes.\n\n\
    REGLAS INVIOLABLES:\n\
    1. IDENTIFICACIÓN Y GROUNDING POR MÉTRICA: Para cada métrica que pide la pregunta, identifica su cifra por el CALIFICADOR (entidad, periodo, moneda/columna) y cítala con su fuente. Si el contexto contiene la misma métrica para OTRA entidad, año, moneda o columna (p.ej. 'mora de Efectiva' vs 'mora del Sistema Financiero'; 'liquidez 2025' vs 'liquidez 2024'), cita la que corresponde a la pregunta, nunca la otra. Si una métrica pedida no figura o no se distingue cuál corresponde, indica '[Dato no especificado en los documentos]' SOLO para esa métrica y continúa con las demás.\n\
    2. CERO ALUCINACIONES NUMÉRICAS: NUNCA deduzcas, inventes, redondees, conviertas de moneda ni aproximes números, porcentajes o monedas que no estén textualmente en el contexto.\n\
    3. CITAS PRECISAS: Cada cifra relevante debe citar el documento y la página correspondiente entre corchetes, ej: [Ferreycorp_Memoria_2025 pág. 39].\n\
    4. IDIOMA: Responde siempre en español formal y directo."
}

pub fn build_context_prompt(
    context_blocks: &[String],
    question: &str,
) -> String {
    let mut prompt = String::new();
    prompt.push_str("--- DOCUMENTOS Y CONTEXTO FUENTE ---\n\n");
    for (i, block) in context_blocks.iter().enumerate() {
        prompt.push_str(&format!("### FUENTE [{}]\n{}\n\n", i + 1, block));
    }
    prompt.push_str("------------------------------------\n\n");
    prompt.push_str(&format!("PREGUNTA DEL ANALISTA: {}\n\n", question));
    prompt.push_str("RESPUESTA EJECUTIVA (con citas y cifras verificadas):");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_think_blocks() {
        let raw = "<think>Aquí está mi razonamiento interno sobre el ROE</think>El ROE de Ferreycorp en 2025 fue de 16.5%.";
        let cleaned = strip_think_blocks(raw);
        assert_eq!(cleaned, "El ROE de Ferreycorp en 2025 fue de 16.5%.");
    }

    #[test]
    fn test_stray_think_closer() {
        let raw = "razonamiento previo colgado </think>La utilidad neta reportada fue S/. 150 millones.";
        let cleaned = strip_think_blocks(raw);
        assert_eq!(cleaned, "La utilidad neta reportada fue S/. 150 millones.");
    }

    #[test]
    fn test_model_alias_resolution() {
        assert_eq!(LlmClient::resolve_model_alias("qwen2.5-3b"), "qwen2.5-3b-instruct-q4_k_m.gguf");
        assert_eq!(LlmClient::resolve_model_alias("qwen3.8-2b"), "Qwen3.8-2B-Q4_K_M.gguf");
        assert_eq!(LlmClient::resolve_model_alias("qwen3.8-4b"), "Qwen3.8-4B-Q4_K_M.gguf");
    }
}
