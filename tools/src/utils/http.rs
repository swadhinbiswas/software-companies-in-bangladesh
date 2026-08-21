//! Shared async HTTP client for the fast crawler.
//!
//! - Single reused connection pool (no per-request client construction).
//! - Retries transient failures with exponential backoff.
//! - Global concurrency cap via a semaphore.
//! - Per-host politeness: jittered minimum delay between requests to the
//!   same host so sites are not hammered.
#[cfg(feature = "crawler")]
use crate::Result;
use log::{debug, warn};
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use url::Url;

const MAX_RETRIES: u32 = 1;
const BASE_BACKOFF_MS: u64 = 300;
const HOST_DELAY_RANGE: (u64, u64) = (60, 180);

pub struct Http {
    client: Client,
    semaphore: Arc<Semaphore>,
    last_request: Mutex<HashMap<String, Instant>>,
    tls_failed: Mutex<HashSet<String>>,
}

impl Http {
    pub fn new(max_concurrent: usize) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36",
            ),
        );

        let client = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(12))
            .tcp_keepalive(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .zstd(true)
            .build()?;

        Ok(Self {
            client,
            semaphore: Arc::new(Semaphore::new(max_concurrent.max(1))),
            last_request: Mutex::new(HashMap::new()),
            tls_failed: Mutex::new(HashSet::new()),
        })
    }

    /// Fetch a URL as text with retries and per-host politeness.
    /// Falls back to the alternate `www.`/apex host when the server's
    /// certificate doesn't cover the requested hostname.
    pub async fn get(&self, url: &Url) -> Result<String> {
        let host = url.host_str().unwrap_or("").to_string();
        // Skip hosts with known-bad TLS (expired, wrong issuer, no useful
        // www↔bare alternate). Prevents 12+ redundant retries during
        // discovery when a host's certificate is broken site-wide.
        if self.tls_failed.lock().unwrap().contains(&host) {
            return Err(format!("TLS: skipping {host} (previously failed)").into());
        }
        match self.fetch(url).await {
            Ok(response) => response
                .error_for_status()?
                .text()
                .await
                .map_err(Into::into),
            Err(err) if is_tls_error_msg(&err.to_string()) => match alternate_www_host(url) {
                Some(alt) if alt.host_str() != Some(&host) => {
                    warn!(
                        "[TLS] {url}: certificate rejected; retrying via {}",
                        alt.host_str().unwrap_or("")
                    );
                    let result = self.fetch(&alt).await;
                    match result {
                        Ok(response) => response
                            .error_for_status()?
                            .text()
                            .await
                            .map_err(Into::into),
                        Err(_) => {
                            // Alternate also failed — the cert is broken
                            // site-wide (expired / unknown issuer), not just
                            // a www mismatch. Remember it.
                            self.tls_failed.lock().unwrap().insert(host.clone());
                            warn!("[TLS] {host}: both hosts failed — marking as broken");
                            Err(err)
                        }
                    }
                }
                _ => {
                    // No useful alternate (multi-level subdomain, or both
                    // www and bare share the same broken cert).
                    if !alternate_www_exists(url) {
                        self.tls_failed.lock().unwrap().insert(host.clone());
                        warn!("[TLS] {host}: no viable alternate — marking as broken");
                    }
                    Err(err)
                }
            },
            Err(err) => Err(err),
        }
    }

    /// Fetch a URL and parse the response as JSON.
    pub async fn get_json<T: DeserializeOwned>(&self, url: &Url) -> Result<T> {
        let response = self.fetch(url).await?.error_for_status()?;
        let body = response.bytes().await?;
        Ok(serde_json::from_slice(&body)?)
    }

    /// Access to the underlying reqwest client (for custom requests).
    pub fn raw_client(&self) -> &Client {
        &self.client
    }

    /// Cheap reachability probe with the same TLS fallback as `get`.
    pub async fn head_ok(&self, url: &Url) -> bool {
        let host = url.host_str().unwrap_or("").to_string();
        if self.tls_failed.lock().unwrap().contains(&host) {
            return false;
        }
        use reqwest::Method;
        let send = |u: Url| {
            self.raw_client()
                .request(Method::HEAD, u)
                .timeout(Duration::from_secs(5))
                .send()
        };
        match send(url.clone()).await {
            Ok(r) => r.status().is_success(),
            Err(err) if is_tls_error_msg(&err.to_string()) => {
                self.tls_failed.lock().unwrap().insert(host);
                match alternate_www_host(url) {
                    Some(alt) => matches!(send(alt).await, Ok(r) if r.status().is_success()),
                    None => false,
                }
            }
            Err(_) => false,
        }
    }

    async fn fetch(&self, url: &Url) -> Result<reqwest::Response> {
        self.polite_delay(url).await;

        let _permit = self.semaphore.acquire().await?;

        let mut attempt = 0;
        loop {
            match self.client.get(url.clone()).send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    {
                        let retryable = attempt < MAX_RETRIES;
                        warn!(
                            "[RETRY {attempt}] {} -> {status}",
                            if retryable { "will retry" } else { "giving up" }
                        );
                        if !retryable {
                            return response.error_for_status().map_err(Into::into);
                        }
                    } else {
                        return Ok(response);
                    }
                }
                Err(err) if attempt < MAX_RETRIES && err.is_timeout() => {
                    warn!("[RETRY {attempt}] timeout: {url}")
                }
                Err(err) => return Err(err.into()),
            }

            attempt += 1;
            tokio::time::sleep(Duration::from_millis(
                BASE_BACKOFF_MS * (1 << (attempt - 1)),
            ))
            .await;
        }
    }

    /// Wait until the minimum inter-request delay for `url`'s host elapses.
    async fn polite_delay(&self, url: &Url) {
        let Some(host) = url.host_str() else { return };

        let (min, max) = HOST_DELAY_RANGE;
        let jitter = min + fastrandish() % (max - min + 1);

        let next_allowed = {
            let mut last = self.last_request.lock().unwrap();
            let previous = last.insert(host.to_string(), Instant::now());
            previous.map(|at| at + Duration::from_millis(jitter))
        };

        if let Some(next) = next_allowed {
            let wait = next.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
    }
}

/// Cheap deterministic-enough jitter without pulling in a rand dependency.
/// xorshift64* seeded from the host string is fine for staggering requests.
fn fastrandish() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    let mut x = *SEED.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs() << 17))
            .unwrap_or(0x9e37_79b9_7f4a_7c15)
    });

    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

static GLOBAL: OnceLock<Arc<Http>> = OnceLock::new();

/// True when a request failed inside TLS verification (e.g. a certificate
/// that doesn't cover the requested `www.` hostname).
fn is_tls_error_msg(msg: &str) -> bool {
    msg.contains("failed to verify TLS certificate")
        || msg.contains("invalid peer certificate")
        || (msg.contains("certificate") && (msg.contains("connect") || msg.contains("request")))
}

/// The same URL served by the alternate host: `www.example.com` ↔
/// `example.com`. Used as a one-shot fallback for certificate mismatches.
fn alternate_www_host(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let alt: Option<String> = if let Some(rest) = host.strip_prefix("www.") {
        Some(rest.to_string())
    } else if host.split('.').count() == 2 {
        Some(format!("www.{host}"))
    } else {
        None
    };
    let mut out = url.clone();
    out.set_host(alt.as_deref()).ok()?;
    Some(out)
}

/// True when there IS a viable www↔bare alternate for this URL (i.e. the
/// host is a simple two-part domain or has a `www.` prefix we can strip).
fn alternate_www_exists(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host.strip_prefix("www.").is_some() || host.split('.').count() == 2
}

/// The process-wide shared client, created on first use.
pub fn global() -> &'static Arc<Http> {
    GLOBAL.get_or_init(|| Arc::new(Http::new(8).expect("failed to build http client")))
}

/// Replace the global client (used to set the concurrency cap from CLI).
pub fn init_global(max_concurrent: usize) -> Result<()> {
    if GLOBAL.get().is_none() {
        let http = Arc::new(Http::new(max_concurrent)?);
        let _ = GLOBAL.set(http);
        debug!("http client initialized with concurrency {max_concurrent}");
    }
    Ok(())
}
