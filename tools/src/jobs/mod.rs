//! Fast job crawler.
//!
//! Pipeline (no browser, no long waits):
//!   job URL
//!     ├─ ATS platform detected?  ──► public JSON API ──► exact JobPost[] (no LLM)
//!     ├─ schema.org JSON-LD?     ──► parse directly   ──► JobPost[]        (no LLM)
//!     └─ generic page            ──► instant HTML GET → paginate (smart) → clean Markdown
//!                                        └─► one LLM call per site ──► JobPost[]
//!   `needsFetch` jobs → detail pages fetched in parallel (memoized/deduped) → LLM
//!
//! Errors never abort the run: a failing company is logged and skipped, and
//! output is persisted incrementally.
mod input_normalizer;
pub mod ats;
pub mod llm;
pub mod schema;

use ats::Ats;
use input_normalizer::{cap_input, normalize_markdown_from};
use llm::Llm;
use schema::*;

use std::collections::{BTreeMap as Map, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use crate::data::Company;
use crate::utils::resolve_url;
use crate::utils::cache::Cache;
use crate::utils::http;
use crate::utils::http::Http;
use crate::utils::{normalize_url, text_file::*};
use crate::{Result, data::Companies};
use futures::{StreamExt, stream};
use log::{error, info, warn};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json as json;
use url::Url;

const LLM_INPUT: &str = r#"You are an information extraction engine.
Extract job postings from the provided Markdown (which may span several pages of a careers site).

Rules:
- Output a JSON object of the form {"jobs": [ ... ]} matching the provided JSON schema. Never wrap in extra keys.
- Extract only actual job postings. If no job posting is found, return {"jobs": []}.
- Set `needsFetch` to true ONLY if the listing lacks full details (description, deadline, apply link) and the job's `source` URL must be fetched to complete it.
- Never guess. Use schema defaults when required.
- Keep `title` unchanged.
- Exclude on-site or location-specific jobs confirmed to be outside Bangladesh.
- Remote jobs may be included even outside Bangladesh.
- Format `description` as Markdown. Reorganize if needed, but do not add or remove information.
- Extract relevant `tags` from the `description` when possible; do not invent or duplicate tags.
- Preserve `source` exactly as provided; never resolve, normalize, or modify it.
- Never guess salary information. Only extract salary explicitly present in the source.
- Include a confidence score (0.0–1.0) for each extracted job.
- Do not include job postings with confidence below 0.5.
- You may see `<!-- PAGE: url -->` markers between pages; ignore them.
"#;

const CACHE_PATH: &str = "job-postings-cache";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_LISTING_PAGES: usize = 4;
const MAX_LLM_CHARS: usize = 40_000;

#[tokio::main]
pub async fn run(
    provider: llm::Provider,
    model: String,
    dir: &Path,
    companies: &Companies<'_>,
    log_file: bool,
    concurrent: u8,
) -> Result {
    http::init_global(concurrent as usize)?;
    let http = http::global().clone();
    let llm = Llm::new(provider, &model, http.clone())?;

    let engine = Arc::new(Crawler::new(
        http,
        llm,
        log_file.then(|| dir.join("job-postings.pages.md")),
    )?);

    let output_file = TextFile::read(dir.join("./data/job-posts.json"))?;

    // Merge with previous results so a failed company never loses data:
    // re-runs only fill the gaps.
    let mut output: Jobs = json::from_str(&output_file.text).unwrap_or_default();

    let started = std::time::Instant::now();
    // `buffer_unordered`: a slow site must never stall the stream — results
    // are yielded as they complete (order is irrelevant for the output map).
    let mut stream = stream::iter(companies.iter())
        .map(|(name, company)| engine.fetch_jobs_from(name, company))
        .buffer_unordered(concurrent.into());

    let mut processed = 0usize;
    let mut failed: Vec<String> = Vec::new();
    while let Some(result) = stream.next().await {
        processed += 1;
        match result {
            Ok(Some((name, source, jobs))) => {
                output.insert(name, Entry { source, jobs });
            }
            Ok(None) => {}
            Err(name) => failed.push(name),
        }

        let elapsed = started.elapsed().as_secs();
        if processed.is_multiple_of(20) || elapsed >= 240 {
            save(&output_file, &output)?;
            info!(
                "Progress: {processed}/{} companies ({} failed) in {elapsed}s",
                companies.len(),
                failed.len()
            );
        }
        if elapsed >= 285 {
            warn!("Soft deadline reached; saving partial results. Re-run resumes via cache.");
            break;
        }
    }

    drop(stream);

    // Retry sweep for rate-limited / transient failures.
    let mut sweep = 0;
    while !failed.is_empty() && sweep < 2 && started.elapsed().as_secs() < 270 {
        sweep += 1;
        info!("Retry sweep #{sweep}: {} companies", failed.len());
        let current = std::mem::take(&mut failed);
        let pending: Vec<(&str, &Company)> = current
            .iter()
            .filter_map(|name| companies.get(name).map(|company| (name.as_str(), company)))
            .collect();
        let mut retries = stream::iter(pending)
            .map(|(name, company)| engine.fetch_jobs_from(name, company))
            .buffer_unordered(concurrent.into());

        while let Some(result) = retries.next().await {
            match result {
                Ok(Some((name, source, jobs))) => {
                    output.insert(name, Entry { source, jobs });
                }
                Ok(None) => {}
                Err(name) => failed.push(name),
            }
        }
    }

    save(&output_file, &output)?;

    info!(
        "From {} companies; Found {} Jobs in {:.1}s ({} still failed)",
        output.len(),
        output.values().map(|f| f.jobs.len()).sum::<usize>(),
        started.elapsed().as_secs_f64(),
        failed.len()
    );

    Ok(())
}

fn save(output_file: &TextFile, output: &Jobs) -> Result {
    output_file.write(json::to_string_pretty(output)?)?;
    Ok(())
}

pub fn clear_cache() -> Result {
    Cache::clear(CACHE_PATH)
}

/// Completed extractions, memoized per URL so work is never repeated
/// across companies in one run. (The 24h disk cache handles re-runs.)
type MemoResult = Arc<Result<Option<Vec<JobPost>>>>;

struct Crawler {
    http: Arc<Http>,
    llm: Llm,
    /// URL → completed extraction result.
    memo: Mutex<HashMap<String, MemoResult>>,
    log_file: Option<Mutex<LogFile>>,
}

impl Crawler {
    fn new(http: Arc<Http>, llm: Llm, log_file: Option<std::path::PathBuf>) -> Result<Self> {
        let log_file = match log_file {
            Some(path) => Some(Mutex::new(open_log_file(path)?)),
            None => None,
        };
        Ok(Self {
            http,
            llm,
            memo: Mutex::new(HashMap::new()),
            log_file,
        })
    }

    async fn fetch_jobs_from(
        &self,
        name: &str,
        company: &Company,
    ) -> Result<Option<(String, Url, Vec<JobPost>)>, String> {
        let Some(url) = company.links.job.as_ref() else {
            return Ok(None);
        };
        let url = match normalize_url(url) {
            Ok(url) => url,
            Err(err) => return Err(format!("invalid job URL: {err}")),
        };

        let jobs = match self.fetch_jobs(&url).await {
            Ok(jobs) => jobs,
            Err(err) => {
                error!("[ERROR] {name}: {err}");
                return Err(name.to_string());
            }
        };

        if !jobs.is_empty() {
            info!("[{name}] {} jobs", jobs.len());
        }

        Ok(Some((name.into(), url, jobs)))
    }

    /// Full extraction for one site URL.
    async fn fetch_jobs(&self, url: &Url) -> Result<Vec<JobPost>> {
        let key = format!("site|{url}");

        let result = self.memo_lookup(&key).await?;
        match result.as_ref() {
            Ok(Some(jobs)) => Ok(jobs.clone()),
            Ok(None) => Ok(vec![]),
            Err(err) => Err(format!("{err:?}").into()),
        }
    }

    /// Completed-result memo lookup; computes and stores on miss.
    async fn memo_lookup(&self, key: &str) -> Result<MemoResult> {
        if let Some(result) = self.memo.lock().unwrap().get(key) {
            return Ok(result.clone());
        }

        let url = key.trim_start_matches("site|");
        let url = Url::parse(url)?;
        let result = Arc::new(self.raw_fetch_jobs(url).await);

        self.memo.lock().unwrap().insert(key.to_string(), result.clone());
        Ok(result)
    }

    async fn raw_fetch_jobs(&self, url: Url) -> Result<Option<Vec<JobPost>>> {
        // 1. Known ATS platform → public JSON API (exact, no LLM).
        if let Some(ats) = Ats::detect(&url)
            && let Some(jobs) = ats.fetch_jobs(&self.http).await?
                && !jobs.is_empty() {
                    return Ok(Some(jobs));
                }

        // 2. Schema.org JobPosting JSON-LD on the page (no LLM).
        let html = self.fetch_html(&url).await?;
        let from_ld = ats::from_json_ld(&html);
        if !from_ld.is_empty() {
            return Ok(Some(from_ld));
        }

        // 3. Generic: crawl listing pages (pagination-aware), then one LLM call.
        let markdown = self.crawl_listing(&url).await?;

        if markdown.trim().is_empty() {
            warn!("[EMPTY] {url}");
            return Ok(Some(vec![]));
        }

        let mut jobs = self.extract_jobs(&url, &markdown).await?;
        self.resolve_needs_fetch(&url, &mut jobs).await;

        Ok(Some(jobs))
    }

    /// Resolve `needsFetch` jobs: fetch all detail pages in parallel (HTTP
    /// only, deduped per URL), then run ONE LLM call on the concatenated
    /// pages instead of one call per page.
    async fn resolve_needs_fetch(&self, site: &Url, jobs: &mut [JobPost]) {
        let mut pending: Vec<_> = jobs
            .iter_mut()
            .filter(|job| job.needs_fetch)
            .filter_map(|job| {
                let urls = job
                    .source
                    .as_deref()
                    .into_iter()
                    .chain(job.apply.iter().find_map(|m| m.website()));

                find_resolved_url(site, urls).map(|url| (url, job))
            })
            .collect();

        if pending.is_empty() {
            return;
        }

        let page_count = pending.len();
        info!("[NEED-FETCH] fetching {page_count} detail page(s) in parallel");

        // 1. Parallel HTTP fetch + markdown conversion (no LLM).
        let pages: Vec<(Url, Result<String>)> = stream::iter(pending.iter().map(|(url, _)| url.clone()))
            .map(|url| async move {
                let markdown = self.detail_markdown(&url).await;
                (url, markdown)
            })
            .buffer_unordered(8)
            .collect()
            .await;

        // 2. One LLM call on all pages together.
        let mut combined = String::new();
        let mut expected: Vec<(&Url, String)> = Vec::new(); // (url, title) in page order
        for (index, (url, markdown)) in pages.iter().enumerate() {
            match markdown {
                Ok(markdown) if !markdown.trim().is_empty() => {
                    let title = pending[index].1.title.clone();
                    expected.push((url, title));
                    combined.push_str(&format!("<!-- JOB {index}: {url} -->\n\n{markdown}\n\n"));
                }
                Ok(_) => warn!("[NO-DETAIL] {url}"),
                Err(err) => error!("[DETAIL-ERROR] {url}: {err}"),
            }
        }

        if combined.trim().is_empty() {
            return;
        }

        let extracted = match self.extract_jobs(site, &combined).await {
            Ok(jobs) => jobs,
            Err(err) => {
                error!("[DETAIL-EXTRACT-ERROR] {site}: {err}");
                return;
            }
        };

        // 3. Map results back: same order and count is the common case;
        //    otherwise match by title so a partial extraction still lands.
        if extracted.len() == expected.len() {
            for ((url, _), mut post) in expected.iter().zip(extracted) {
                if post.source.is_none() && post.apply.is_empty() {
                    post.source = Some(url.to_string());
                    post.apply = vec![ApplicationMethod::Website(url.to_string())];
                }
                if let Some(job) = pending.iter_mut().find(|(u, _)| u.as_str() == url.as_str()) {
                    *(job.1) = post;
                }
            }
        } else {
            for mut post in extracted {
                let matched = expected
                    .iter()
                    .find(|(_, title)| titles_match(title, &post.title));
                if let Some(url) = matched.map(|(url, _)| *url) {
                    if post.source.is_none() && post.apply.is_empty() {
                        post.source = Some(url.to_string());
                        post.apply = vec![ApplicationMethod::Website(url.to_string())];
                    }
                    if let Some((pending_url, job)) =
                        pending.iter_mut().find(|(u, _)| u.as_str() == url.as_str())
                    {
                        let _ = pending_url;
                        **job = post;
                    }
                }
            }
        }

        info!("[NEED-FETCH] resolved {page_count} detail page(s)");
    }

    /// Fetch one detail page and convert to markdown (JSON-LD short-circuits).
    async fn detail_markdown(&self, url: &Url) -> Result<String> {
        let html = self.fetch_html(url).await?;

        let from_ld = ats::from_json_ld(&html);
        if !from_ld.is_empty() {
            let jobs = from_ld;
            return Ok(format!("<!-- JSON-LD -->\n{}", serde_json::to_string(&jobs)?));
        }

        normalize_markdown_from(&html)
    }

    /// One LLM call per site (all crawled pages concatenated).
    async fn extract_jobs(&self, url: &Url, markdown: &str) -> Result<Vec<JobPost>> {
        let cache_key = format!("llm|{}|{}", self.llm.model(), url);
        let cache = Cache::open_with_ttl(CACHE_PATH, &cache_key, Some(CACHE_TTL))?;

        if let Some(json) = cache.get()? {
            return Ok(json::from_str(&json)?);
        }

        self.log_page(url, markdown);

        info!("[LLM-CALL] {url}");
        let schema = json::json!({
            "type": "object",
            "properties": {
                "jobs": {
                    "type": "array",
                    "items": json::to_value(schema_for!(JobPost))?,
                }
            },
            "required": ["jobs"],
        });

        let value = self
            .llm
            .extract_json(
                LLM_INPUT,
                &format!("Extract from this markdown and return the job postings as json.\n\n{}", cap_input(markdown, MAX_LLM_CHARS)),
                &schema,
            )
            .await?;

        let jobs: Vec<JobPost> = match value.get("jobs") {
            Some(json::Value::Array(jobs)) => jobs
                .iter()
                .filter_map(|j| json::from_value(j.clone()).ok())
                .collect(),
            _ => vec![],
        };

        cache.set(&json::to_string(&jobs)?)?;
        Ok(jobs)
    }

    fn log_page(&self, url: &Url, markdown: &str) {
        if let Some(file) = &self.log_file
            && let Ok(file) = file.lock() {
                let _ = writeln!(file.as_ref(), "---\n{url}\n{}", cap_input(markdown, 12_000));
            }
    }

    /// Instant HTML fetch with a 24h cache. No browser, no waits.
    async fn fetch_html(&self, url: &Url) -> Result<String> {
        let url = normalize_url(url).map_err(|err| format!("{err}"))?;
        let cache = Cache::open_with_ttl(CACHE_PATH, url.as_str(), Some(CACHE_TTL))?;

        if let Some(html) = cache.get()? {
            return Ok(html);
        }

        let html = self.http.get(&url).await?;
        cache.set(&html)?;
        Ok(html)
    }

    /// Crawl a listing page plus its pagination pages (max `MAX_LISTING_PAGES`).
    /// Each round fetches all queued pages in parallel; next-links are
    /// discovered from the fetched HTML and enqueued for the next round.
    /// Returns concatenated Markdown with `<!-- PAGE -->` markers.
    async fn crawl_listing(&self, base: &Url) -> Result<String> {
        let mut visited = HashSet::new();
        let mut pages: Vec<String> = Vec::new();
        let mut queue: VecDeque<Url> = VecDeque::new();
        queue.push_back(base.clone());

        while !queue.is_empty() && pages.len() < MAX_LISTING_PAGES {
            let round: Vec<Url> = queue
                .drain(..)
                .filter(|url| visited.insert(url.as_str().to_string()))
                .collect();

            let results: Vec<(Url, Result<(String, String)>)> = stream::iter(round)
                .map(|url| {
                    let me = self;
                    Box::pin(async move {
                        let markdown = me.fetch_html_and_markdown(&url).await;
                        (url, markdown)
                    })
                })
                .buffer_unordered(4)
                .collect()
                .await;

            for (url, result) in &results {
                match result {
                    Ok((html, markdown)) if !markdown.trim().is_empty() => {
                        pages.push(format!("<!-- PAGE: {url} -->\n\n{markdown}"));

                        if pages.len() < MAX_LISTING_PAGES {
                            for next in next_page_links(html, url) {
                                let key = next.as_str().to_string();
                                if !visited.contains(&key) && !queue.iter().any(|u| u.as_str() == key) {
                                    queue.push_back(next);
                                }
                            }
                        }
                    }
                    Ok(_) => warn!("[EMPTY-PAGE] {url}"),
                    Err(err) => warn!("[PAGE-ERROR] {url}: {err}"),
                }
            }
        }

        Ok(pages.join("\n\n"))
    }

    /// Fetch a page and convert it to Markdown in one cached step.
    async fn fetch_html_and_markdown(&self, url: &Url) -> Result<(String, String)> {
        let html = self.fetch_html(url).await?;
        let markdown = normalize_markdown_from(&html)?;
        Ok((html, markdown))
    }
}

/// Find pagination links on a listing page: `<link rel="next">`, `?page=N`
/// query links, and next/› anchors. Same-origin, same path, never fragment-only.
fn next_page_links(html: &str, base: &Url) -> Vec<Url> {
    use scraper::{Html, Selector};

    let mut found: Vec<Url> = Vec::new();
    let html = Html::parse_document(html);

    let selectors = [
        r#"link[rel="next"]"#,
        r#"a[href*="page="]"#,
        r#"a[rel="next"]"#,
        r#"a.pagination-next"#,
        r#"li.next a, li.pagination-next a"#,
    ];

    let mut candidates: Vec<(String, bool)> = Vec::new();

    for raw in selectors {
        if let Ok(selector) = Selector::parse(raw) {
            for element in html.select(&selector) {
                if let Some(href) = element.value().attr("href") {
                    let explicit = raw.contains("next");
                    candidates.push((href.to_string(), explicit));
                }
            }
        }
    }

    // Anchors whose visible text looks like "next".
    if let Ok(selector) = Selector::parse("a[href]") {
        for element in html.select(&selector) {
            let text = element.text().collect::<String>();
            let text = text.trim();
            if matches!(text, "next" | "next page" | "Next" | "Next Page" | "›" | "»" | "Next →")
                && let Some(href) = element.value().attr("href") {
                    candidates.push((href.to_string(), true));
                }
        }
    }

    for (href, explicit) in candidates {
        let Ok(resolved) = resolve_url(base, &href) else {
            continue;
        };
        if resolved == *base {
            continue;
        }
        // Only same host + same path (pagination, not navigation).
        if resolved.host_str() != base.host_str() {
            continue;
        }
        if resolved.path() != base.path() && !explicit {
            continue;
        }
        if !found.contains(&resolved) {
            found.push(resolved);
        }
    }

    found
}

fn find_resolved_url<'a>(base: &Url, urls: impl IntoIterator<Item = &'a str>) -> Option<Url> {
    for src in urls {
        let resolved = resolve_url(base, src).ok()?;
        if &resolved != base {
            return Some(resolved);
        }
    }
    None
}

/// Loose title equality: one contains the other (case/punctuation-insensitive).
fn titles_match(expected: &str, actual: &str) -> bool {
    fn normalize(title: &str) -> String {
        title
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }
    let a = normalize(expected);
    let b = normalize(actual);
    (a.len() >= 4 && b.len() >= 4) && (a.contains(&b) || b.contains(&a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_filenames_are_short_and_stable() {
        let long = "https://example.com/careers?utm_source=x&utm_medium=y&utm_campaign=z&page=2&foo=bar&baz=qux&more=stuff&even=more&params=here".repeat(3);
        let a = crate::utils::cache::to_filename(&long);
        let b = crate::utils::cache::to_filename(&long);
        assert_eq!(a, b);
        assert!(a.len() < 64, "filename too long: {}", a.len());
    }
}
