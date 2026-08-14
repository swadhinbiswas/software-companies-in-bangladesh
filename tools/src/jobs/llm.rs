//! OpenCode Zen LLM client.
//!
//! Calls `https://opencode.ai/zen/v1/chat/completions` (OpenAI-compatible)
//! with the free `deepseek-v4-flash-free` model. The API key is read from
//! `ZEN_API_KEY`, `OPENCODE_ZEN_API_KEY`, or the local opencode auth store
//! (`~/.local/share/opencode/auth.json`, service `opencode`).
use crate::Result;
use crate::utils::http::Http;
use log::{debug, info};
use serde_json as json;
use std::path::PathBuf;
use std::time::Instant;

pub const ZEN_ENDPOINT: &str = "https://opencode.ai/zen/v1/chat/completions";
pub const DEFAULT_MODEL: &str = "deepseek-v4-flash-free";

pub struct Llm {
    http: std::sync::Arc<Http>,
    endpoint: String,
    api_key: String,
    model: String,
}

#[derive(serde::Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(serde::Deserialize)]
struct Choice {
    message: Message,
}

#[derive(serde::Deserialize)]
struct Message {
    content: Option<String>,
}

impl Llm {
    pub fn new(model: &str, http: std::sync::Arc<Http>) -> Result<Self> {
        let api_key = zen_api_key()?;
        info!("LLM: {model} via {ZEN_ENDPOINT}");
        Ok(Self {
            http,
            endpoint: ZEN_ENDPOINT.to_string(),
            api_key,
            model: model.to_string(),
        })
    }

    /// Extract structured JSON from `markdown`. The schema is embedded in
    /// the system prompt and the response is forced to JSON object mode.
    pub async fn extract_json(
        &self,
        system: &str,
        user: &str,
        json_schema: &json::Value,
    ) -> Result<json::Value> {
        let started = Instant::now();

        let body = json::json!({
            "model": self.model,
            "temperature": 0.0,
            "response_format": { "type": "json_object" },
            "messages": [
                {
                    "role": "system",
                    "content": format!("{system}\n\nRespond with a single JSON object that conforms to this JSON schema:\n{schema_pretty}", schema_pretty = json_schema),
                },
                { "role": "user", "content": user },
            ],
        });

        let response = self
            .http
            .raw_client()
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(format!("zen LLM error {status}: {}", truncate(&text, 300)).into());
        }

        let payload: ChatResponse = response.json().await?;
        let content = payload
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| "zen LLM returned no content".to_string())?;

        let value = strip_code_fence(&content);
        let value = json::from_str::<json::Value>(&value)
            .map_err(|err| format!("LLM returned invalid JSON: {err}\n{}", truncate(&content, 400)))?;

        debug!("LLM call took {:.1}s", started.elapsed().as_secs_f64());
        Ok(value)
    }
}

fn strip_code_fence(content: &str) -> String {
    let content = content.trim();
    content
        .strip_prefix("```json")
        .or_else(|| content.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| content.to_string())
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        format!("{}…", &text[..max])
    }
}

/// Resolve the Zen API key: env vars first, then the opencode auth store.
fn zen_api_key() -> Result<String> {
    for env in ["ZEN_API_KEY", "OPENCODE_ZEN_API_KEY", "OPENAUTH_OPCODE_API_KEY"] {
        if let Ok(key) = std::env::var(env) {
            if !key.is_empty() {
                return Ok(key);
            }
        }
    }

    let auth = auth_json_path()?;
    let text = std::fs::read_to_string(&auth)
        .map_err(|err| format!("no ZEN_API_KEY and failed to read {auth}: {err}"))?;
    let value: json::Value = json::from_str(&text)
        .map_err(|err| format!("failed to parse {auth}: {err}"))?;

    value
        .get("opencode")
        .and_then(|v| v.get("key"))
        .and_then(json::Value::as_str)
        .map(|key| key.to_string())
        .ok_or_else(|| format!("no `opencode` key found in {auth}; set ZEN_API_KEY").into())
}

fn auth_json_path() -> Result<PathBuf> {
    if let Ok(data) = std::env::var("XDG_DATA_HOME") {
        let path = PathBuf::from(data).join("opencode/auth.json");
        if path.exists() {
            return Ok(path);
        }
    }
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    Ok(PathBuf::from(home).join(".local/share/opencode/auth.json"))
}

impl Llm {
    /// Raw access to the shared HTTP client (used by callers for streaming).
    pub fn http(&self) -> &Http {
        &self.http
    }
}
