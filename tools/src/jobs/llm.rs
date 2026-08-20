//! LLM client for job extraction.
//!
//! Two providers:
//! - `gemini` (default): Google Generative Language API
//!   `POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent`
//!   key from `GEMINI_API_KEY`, or the `google` service in the opencode auth store.
//! - `zen`: OpenCode Zen OpenAI-compatible endpoint
//!   `POST https://opencode.ai/zen/v1/chat/completions`
//!   key from `ZEN_API_KEY`, or the `opencode` service in the auth store.
use crate::Result;
use crate::utils::http::Http;
use log::{debug, info, warn};
use serde_json as json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

pub const ZEN_ENDPOINT: &str = "https://opencode.ai/zen/v1/chat/completions";
pub const GEMINI_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/models";
pub const DEFAULT_MODEL: &str = "gemini-3.5-flash-lite";

/// Maximum concurrent LLM requests (the bottleneck of the crawl).
pub const LLM_CONCURRENCY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Gemini,
    Zen,
}

impl Provider {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "gemini" => Ok(Self::Gemini),
            "zen" => Ok(Self::Zen),
            other => Err(format!("unknown provider '{other}' (expected 'gemini' or 'zen')").into()),
        }
    }
}

pub struct Llm {
    http: Arc<Http>,
    provider: Provider,
    endpoint: String,
    api_key: String,
    model: String,
    semaphore: Arc<Semaphore>,
    /// Last LLM call start time; enforces a minimum interval between calls
    /// so bursts don't blow the provider's per-minute rate limit.
    pace: std::sync::Mutex<Instant>,
    min_interval: Duration,
}

/// Minimum spacing between LLM request starts. With the free tier at
/// ~10 requests/minute, 7s keeps us safely under the limit while finishing
/// ~10 batched calls per minute (all sites in well under 5 minutes).
const LLM_INTERVAL: Duration = Duration::from_secs(7);

#[derive(serde::Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(serde::Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

#[derive(serde::Deserialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(serde::Deserialize)]
struct GeminiPart {
    text: Option<String>,
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
    /// The model name used for calls (also a cache key component).
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn new(provider: Provider, model: &str, http: Arc<Http>) -> Result<Self> {
        let (endpoint, api_key) = match provider {
            Provider::Gemini => (GEMINI_ENDPOINT.to_string(), gemini_api_key()?),
            Provider::Zen => (ZEN_ENDPOINT.to_string(), zen_api_key()?),
        };
        info!("LLM: {model} via {provider:?}");
        Ok(Self {
            http,
            provider,
            endpoint,
            api_key,
            model: model.to_string(),
            semaphore: Arc::new(Semaphore::new(LLM_CONCURRENCY)),
            pace: std::sync::Mutex::new(Instant::now()),
            min_interval: LLM_INTERVAL,
        })
    }

    /// Extract structured JSON from `markdown`. The schema is embedded in
    /// the system prompt and the response is forced to JSON mode.
    pub async fn extract_json(
        &self,
        system: &str,
        user: &str,
        json_schema: &json::Value,
    ) -> Result<json::Value> {
        let started = Instant::now();
        let schema_pretty = json::to_string_pretty(json_schema)?;

        let content = match self.provider {
            Provider::Gemini => {
                let response = self
                    .send_with_retry(
                        &format!("{}/{}:generateContent", self.endpoint, self.model),
                        &json::json!({
                            "systemInstruction": { "parts": [{ "text": format!("{system}\n\nRespond with a single JSON object that conforms to this JSON schema:\n{schema_pretty}") }] },
                            "contents": [{ "parts": [{ "text": user }] }],
                            "generationConfig": {
                                "temperature": 0.0,
                                "responseMimeType": "application/json",
                            },
                        }),
                        |status| status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
                    )
                    .await?;

                let status = response.status();
                if !status.is_success() {
                    let text = response.text().await.unwrap_or_default();
                    return Err(format!("gemini error {status}: {}", truncate(&text, 300)).into());
                }

                let payload: GeminiResponse = response.json().await?;
                payload
                    .candidates
                    .into_iter()
                    .next()
                    .and_then(|c| c.content.parts.into_iter().filter_map(|p| p.text).next())
                    .ok_or_else(|| "gemini returned no content".to_string())?
            }
            Provider::Zen => {
                let response = self
                    .send_with_retry(
                        &self.endpoint,
                        &json::json!({
                            "model": self.model,
                            "temperature": 0.0,
                            "response_format": { "type": "json_object" },
                            "messages": [
                                {
                                    "role": "system",
                                    "content": format!("{system}\n\nRespond with a single JSON object that conforms to this JSON schema:\n{schema_pretty}"),
                                },
                                { "role": "user", "content": user },
                            ],
                        }),
                        |status| status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
                    )
                    .await?;

                let status = response.status();
                if !status.is_success() {
                    let text = response.text().await.unwrap_or_default();
                    return Err(format!("zen LLM error {status}: {}", truncate(&text, 300)).into());
                }

                let payload: ChatResponse = response.json().await?;
                payload
                    .choices
                    .into_iter()
                    .next()
                    .and_then(|c| c.message.content)
                    .ok_or_else(|| "zen LLM returned no content".to_string())?
            }
        };

        let value = json::from_str::<json::Value>(&strip_code_fence(&content)).map_err(|err| {
            format!("LLM returned invalid JSON: {err}\n{}", truncate(&content, 400))
        })?;

        debug!("LLM call took {:.1}s", started.elapsed().as_secs_f64());
        Ok(value)
    }

    /// POST with retries, bounded concurrency, generous timeout, pacing and
    /// exponential backoff (429s need real cooldown, ideally the server's
    /// `Retry-After` / `retryDelay`).
    async fn send_with_retry(
        &self,
        url: &str,
        body: &json::Value,
        retryable_status: impl Fn(reqwest::StatusCode) -> bool,
    ) -> Result<reqwest::Response> {
        const MAX_ATTEMPTS: u32 = 6;
        const BACKOFF_SECS: u64 = 5;

        let _permit = self.semaphore.acquire().await?;

        let mut attempt = 0u32;
        loop {
            // Pace: no request starts sooner than `min_interval` after the
            // previous one, so bursts never trip the per-minute rate limit.
            {
                let mut last = self.pace.lock().unwrap();
                let since = last.elapsed();
                if since < self.min_interval {
                    tokio::time::sleep(self.min_interval - since).await;
                }
                *last = Instant::now();
            }

            // Gemini authenticates via `?key=` query parameter, Zen via Bearer header.
            let auth_url = match self.provider {
                Provider::Gemini => {
                    reqwest::Url::parse_with_params(url, &[("key", &self.api_key)])
                        .map_err(|err| format!("invalid LLM url {url}: {err}"))?
                }
                Provider::Zen => reqwest::Url::parse(url)
                    .map_err(|err| format!("invalid LLM url {url}: {err}"))?,
            };

            let mut request = self
                .http
                .raw_client()
                .post(auth_url)
                .json(body)
                .timeout(Duration::from_secs(60));

            if self.provider == Provider::Zen {
                request = request.bearer_auth(&self.api_key);
            }

            let backoff = match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    if !retryable_status(status) {
                        return Ok(response);
                    }

                    let header_delay = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let text = response.text().await.unwrap_or_default();
                    let (body_delay, daily_quota) = gemini_error_info(&text);

                    // Daily free-tier quota is exhausted; retrying is futile.
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS && daily_quota {
                        return Err(format!(
                            "gemini daily quota exhausted: {}",
                            truncate(&text, 300)
                        )
                        .into());
                    }

                    if attempt + 1 < MAX_ATTEMPTS {
                        warn!(
                            "[LLM-RETRY {attempt}] {status} (retry-after: {}s)",
                            header_delay.max(body_delay)
                        );
                    } else {
                        return Err(format!(
                            "LLM error {status}: {}",
                            truncate(&text, 300)
                        )
                        .into());
                    }

                    Duration::from_secs(header_delay.max(body_delay).max(1))
                }
                Err(err) if attempt + 1 < MAX_ATTEMPTS => {
                    warn!("[LLM-RETRY {attempt}] {err}");
                    Duration::from_secs(BACKOFF_SECS << attempt)
                }
                Err(err) => return Err(err.into()),
            };

            attempt += 1;
            tokio::time::sleep(backoff).await;
        }
    }
}

/// Parse Gemini's structured error body for retry guidance.
/// Returns `(retry_delay_secs, is_daily_quota_exhaustion)`.
fn gemini_error_info(body: &str) -> (u64, bool) {
    let Ok(value) = json::from_str::<json::Value>(body) else {
        return (0, false);
    };
    let Some(error) = value.get("error") else {
        return (0, false);
    };

    let mut delay = 0u64;
    let mut daily = false;

    if let Some(details) = error.get("details").and_then(json::Value::as_array) {
        for detail in details {
            if let Some(retry_delay) = detail
                .get("retryInfo")
                .and_then(|r| r.get("retryDelay"))
                .and_then(json::Value::as_str)
                .and_then(parse_delay)
            {
                delay = delay.max(retry_delay);
            }
            if let Some(violations) = detail
                .get("quotaFailure")
                .and_then(|q| q.get("violations"))
                .and_then(json::Value::as_array)
            {
                for violation in violations {
                    if violation
                        .get("quotaId")
                        .and_then(json::Value::as_str)
                        .is_some_and(|id| id.contains("PerDay"))
                    {
                        daily = true;
                    }
                }
            }
        }
    }

    (delay, daily)
}

/// Parse a duration string like `"54s"` or `"1.5s"` into whole seconds.
fn parse_delay(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches('s');
    let secs: f64 = s.parse().ok()?;
    Some(secs.ceil() as u64)
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

/// Resolve the Gemini API key: `GEMINI_API_KEY` (also `GEMINIAPIKEY` alias) env first, then the `google`
/// service in the opencode auth store.
fn gemini_api_key() -> Result<String> {
    for env in ["GEMINI_API_KEY", "GEMINIAPIKEY", "GOOGLE_API_KEY", "GOOGLE_GENAI_API_KEY"] {
        if let Some(key) = env_key(env) {
            return Ok(key);
        }
    }
    auth_service_key("google")
}

/// Resolve the Zen API key: `ZEN_API_KEY` env first, then the `opencode`
/// service in the opencode auth store.
fn zen_api_key() -> Result<String> {
    for env in ["ZEN_API_KEY", "OPENCODE_ZEN_API_KEY"] {
        if let Some(key) = env_key(env) {
            return Ok(key);
        }
    }
    auth_service_key("opencode")
}

fn env_key(env: &str) -> Option<String> {
    std::env::var(env).ok().filter(|k| !k.is_empty())
}

fn auth_service_key(service: &str) -> Result<String> {
    let auth = auth_json_path()?;
    let text = std::fs::read_to_string(&auth)
        .map_err(|err| format!("no API key env set and failed to read {}: {err}", auth.display()))?;
    let value: json::Value = json::from_str(&text)
        .map_err(|err| format!("failed to parse {}: {err}", auth.display()))?;

    value
        .get(service)
        .and_then(|v| v.get("key"))
        .and_then(json::Value::as_str)
        .map(|key| key.to_string())
        .ok_or_else(|| {
            format!(
                "no `{service}` key found in {}; set the provider API key env var",
                auth.display()
            )
            .into()
        })
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
