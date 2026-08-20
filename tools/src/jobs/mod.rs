//! Fast job crawler.
//!
//! Pipeline (no browser, no long waits, minimal LLM calls):
//!   1. Crawl phase (HTTP only, parallel):
//!      - ATS platform detected?  -> public JSON API -> exact JobPost[] (no LLM)
//!      - schema.org JSON-LD?     -> parse directly  -> JobPost[]        (no LLM)
//!      - generic page            -> instant HTML GET + smart pagination -> clean Markdown
//!   2. LLM phase (batched): several sites per call with `<!-- SITE -->`
//!      markers, returns per-site JobPost[]. Paced to stay under rate limits.
//!   3. Detail phase (batched): `needsFetch` pages fetched in parallel,
//!      extracted in batches.
//!
//! Errors never abort the run; output is persisted incrementally and merged
//! with previous runs, so a re-run only fills the gaps (24h disk cache).
mod input_normalizer;
pub mod ats;
pub mod enhancer;
pub mod llm;
pub mod schema;

use ats::Ats;
use input_normalizer::{cap_input, normalize_markdown_from};
use llm::Llm;
use schema::*;

use std::collections::{BTreeMap as Map, HashSet, VecDeque};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

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

const LLM_INPUT: &str = r###"You are a production information extraction engine for a Bangladeshi job board.
Input is Markdown for ONE OR MORE career sites. Each site is wrapped:
<!-- SITE: <company name> -->
...clean markdown (may contain <!-- PAGE: url --> markers, ignore them)...
<!-- END SITE -->

GOAL: Return perfect JSON matching the provided schema. Production quality — no hallucination.

Output: {"sites": [{"name": "<exact company name from SITE marker>", "jobs": [ ...JobPost ]}, ...]}
- One entry per SITE marker, in same order. If a site has no jobs, return {"name": ..., "jobs": []}. Never omit a SITE.
- Never wrap in extra keys, never output code fences.

Extraction rules (strict):
- Title: keep exactly as on page (trim only). Don't normalize or translate.
- Description: clean Markdown, keep all facts, remove repeated nav/footer boilerplate, fix headings/lists, don't add inventions. Keep as full as possible (up to ~5000 chars).
- needsFetch: true ONLY if listing is summary (missing description, deadline, or apply link) and you saw a `source` URL that must be fetched for details. Otherwise false.
- Never guess: if a field not present, use null/empty per schema. Don't infer salary, deadline, postedAt.
- Bangladesh filter: exclude on-site/Hybrid jobs proven outside Bangladesh (contains city/country outside BD). Remote is always allowed (even US/EU).
- Salary: extract only explicit numbers. If text says "30k-50k BDT" then min 30000 max 50000 currency BDT. If "Negotiable" then null. Never invent. Ensure min <= max. Currency ISO if present else BDT for BD jobs.
- Tags: 3-12 real tech/skills that actually appear in description. Dedup case-insensitive. No junk: "hiring", "job", "career" forbidden. Prefer canonical (React not React.js, NodeJS not Node.js) but keep as seen if unsure. Never invent.
- source/apply: preserve exactly as in markdown (may be relative "/careers/123" or absolute). Never resolve to absolute, never modify. `source` is the job detail URL; `apply` Website is apply link.
- EmploymentType: map literally (Full-time -> FullTime, Internship -> Internship, etc). Don't guess.
- Location: exact city/area string or Remote. Keep "Hybrid (Dhaka)" style if hybrid.
- Confidence 0.5-1.0: high if title+description+location+apply present, low if sparse. Below 0.5 must be omitted (do not output).
- Be deterministic: temperature 0.0, same input -> same JSON.

Examples:
- Good: {"title":"Sr. Backend Engineer","description":"Responsibilities: Build APIs...","employmentType":"FullTime","location":{"OnSite":"Dhaka"},"tags":["Go","PostgreSQL","Docker"],"confidence":0.92}
- Bad (don't do): inventing salary when not present, adding tags not in description, translating title.

Ignore <!-- PAGE: ... --> markers inside markdown; they are pagination hints, not content.
"###;

const CACHE_PATH: &str = "job-postings-cache";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_LISTING_PAGES: usize = 4;
const MAX_LLM_CHARS: usize = 12_000;
/// Sites per LLM call in the listing phase.
const BATCH_SITES: usize = 12;
/// Detail pages per LLM call in the detail phase.
const BATCH_DETAILS: usize = 8;
/// Soft deadline: stop starting new work at this point; persisted results
/// plus the 24h cache let a re-run finish in seconds.
const SOFT_DEADLINE_SECS: u64 = 270;

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
    // Merge with previous results: failed companies never lose data.
    let mut output: Jobs = json::from_str(&output_file.text).unwrap_or_default();

    let started = std::time::Instant::now();
    info!("Crawling {} companies...", companies.len());

    // ── Phase 1: crawl everything (HTTP only, no LLM). ────────────────────
    let mut pending: Vec<(String, Url, String)> = Vec::new();
    let mut stream = stream::iter(companies.iter())
        .map(|(name, company)| {
            let me = engine.clone();
            let name = name.clone();
            async move {
                let Some(url) = company.links.job.as_ref() else {
                    return (name, None);
                };
                let Ok(url) = normalize_url(url) else {
                    return (name, None);
                };
                let prepared = me.prepare_site(&url).await;
                (name, Some((url, prepared)))
            }
        })
        .buffer_unordered(concurrent.into());

    while let Some((name, result)) = stream.next().await {
        match result {
            Some((url, Ok(SitePrepare::Exact(jobs)))) if !jobs.is_empty() => {
                // Production: normalize ATS jobs too (salary caps, tag hygiene, dedup)
                let jobs = enhancer::enhance_batch(jobs, None);
                if !jobs.is_empty() {
                    output.insert(name, Entry { source: url, jobs });
                }
            }
            Some((_, Ok(SitePrepare::Exact(_)))) => {}
            Some((url, Ok(SitePrepare::Markdown(md)))) => {
                pending.push((name, url, md));
            }
            Some((_, Ok(SitePrepare::Empty))) => {}
            Some((_, Err(err))) => error!("[ERROR] {name}: {err}"),
            None => {}
        }

        if started.elapsed().as_secs() >= SOFT_DEADLINE_SECS {
            warn!("Soft deadline in crawl phase; saving partial results");
            break;
        }
    }
    drop(stream);

    // ── Phase 2: batched LLM extraction of pending sites. ────────────────
    info!("Extracting {} site(s) via LLM...", pending.len());
    let mut detail_queue: Vec<(String, Url, JobPost)> = Vec::new();

    let mut retry_batches: Vec<Vec<(String, String)>> = Vec::new();
    let mut batch = 0usize;
    while batch < pending.len() && started.elapsed().as_secs() < SOFT_DEADLINE_SECS {
        let chunk: Vec<_> = pending[batch..(batch + BATCH_SITES).min(pending.len())]
            .iter()
            .map(|(name, _, md)| (name.as_str(), md.as_str()))
            .collect();

        match engine.extract_batch(&chunk).await {
            Ok(assignments) => {
                let mut matched = std::collections::HashSet::new();
                for (name, jobs) in assignments {
                    matched.insert(name.clone());
                    if let Some((_, url, _)) = pending.iter().find(|(n, _, _)| *n == name) {
                        // Production enhancer: clean tags, markdown, confidence recalibration, dedup
                        let jobs = enhancer::enhance_batch(jobs, None);
                        for job in jobs {
                            if job.needs_fetch {
                                detail_queue.push((name.clone(), url.clone(), job));
                            } else if let Some(entry) = output.get_mut(&name) {
                                entry.jobs.push(job);
                            } else {
                                // Company not yet in output (first time) — create entry
                                output.insert(name.clone(), Entry { source: url.clone(), jobs: vec![job] });
                            }
                        }
                    }
                }
                // Sites the LLM skipped get one more chance in the sweep.
                let missed: Vec<_> = chunk
                    .iter()
                    .filter(|(n, _)| !matched.contains(*n))
                    .map(|(n, md)| (n.to_string(), md.to_string()))
                    .collect();
                if !missed.is_empty() {
                    retry_batches.push(missed);
                }
            }
            Err(err) => {
                error!("[BATCH-ERROR] {err}");
                if err.to_string().contains("quota exhausted") {
                    error!("LLM daily quota exhausted; skipping remaining LLM extraction");
                    break;
                }
                retry_batches.push(
                    chunk
                        .iter()
                        .map(|(n, md)| (n.to_string(), md.to_string()))
                        .collect(),
                );
            }
        }

        batch += BATCH_SITES;
    }

    // Retry sweep for rate-limited / LLM-skipped sites.
    for sweep in retry_batches {
        if started.elapsed().as_secs() >= SOFT_DEADLINE_SECS {
            break;
        }
        info!("[LLM-SWEEP] retrying {} site(s)", sweep.len());
        let chunk: Vec<(&str, &str)> = sweep.iter().map(|(n, md)| (n.as_str(), md.as_str())).collect();
        match engine.extract_batch(&chunk).await {
            Ok(assignments) => {
                for (name, jobs) in assignments {
                    if let Some((_, url, _)) = pending.iter().find(|(n, _, _)| *n == name) {
                        let jobs = enhancer::enhance_batch(jobs, None);
                        for job in jobs {
                            if job.needs_fetch {
                                detail_queue.push((name.clone(), url.clone(), job));
                            } else if let Some(entry) = output.get_mut(&name) {
                                entry.jobs.push(job);
                            } else {
                                output.insert(name.clone(), Entry { source: url.clone(), jobs: vec![job] });
                            }
                        }
                    }
                }
            }
            Err(err) => {
                error!("[SWEEP-ERROR] {err}");
                if err.to_string().contains("quota exhausted") {
                    error!("LLM daily quota exhausted; skipping detail extraction");
                    detail_queue.clear();
                    break;
                }
            }
        }
    }

    // ── Phase 3: batched detail resolution for `needsFetch` jobs. ─────────
    if !detail_queue.is_empty() {
        info!("Resolving {} detail page(s)...", detail_queue.len());

        // Fetch all detail pages in parallel (HTTP only).
        let mut fetched: Vec<(String, Url, JobPost, Result<String>)> = stream::iter(detail_queue)
            .map(|(name, url, job)| {
                let me = engine.clone();
                let url = url.clone();
                async move {
                    let md = me.detail_markdown(&url).await;
                    (name, url, job, md)
                }
            })
            .buffer_unordered(concurrent.into())
            .collect()
            .await;

        // Extract in batches.
        let mut i = 0;
        while i < fetched.len() {
            let chunk: Vec<_> = fetched[i..(i + BATCH_DETAILS).min(fetched.len())]
                .iter()
                .map(|(_, url, job, md)| {
                    (url.as_str(), job.title.as_str(), md.as_ref().ok().map(|s| s.as_str()))
                })
                .collect();

            match engine.extract_details(&chunk).await {
                Ok(posts) => {
                    for ((_, url, job, _), post) in fetched[i..].iter_mut().zip(posts) {
                        if let Some(mut post) = post {
                            if post.source.is_none() && post.apply.is_empty() {
                                post.source = Some(url.to_string());
                                post.apply = vec![ApplicationMethod::Website(url.to_string())];
                            }
                            // Production AI hygiene for detail page extraction
                            if let Some(e) = enhancer::enhance_job_deterministic(post, None) {
                                *job = e;
                            } else {
                                job.title = String::new(); // mark for drop
                            }
                        }
                    }
                }
                Err(err) => error!("[DETAIL-BATCH-ERROR] {err}"),
            }
            i += BATCH_DETAILS;
        }

        // Assign resolved posts back into the output (with final validation).
        for (name, _, job, _) in fetched {
            if job.title.is_empty() || job.confidence < 0.5 { continue; }
            if let Some(entry) = output.get_mut(&name) {
                if job.source.is_some() || !job.apply.is_empty() {
                    entry.jobs.push(job);
                }
            } else if job.source.is_some() || !job.apply.is_empty() {
                let src = job.source.clone().and_then(|s| Url::parse(&s).ok()).unwrap_or_else(|| Url::parse("https://example.com").unwrap());
                output.insert(name.clone(), Entry { source: src, jobs: vec![job] });
            }
        }
    }

    // Production dedup per company (same title+location → best confidence) — always, not just detail queue
    for entry in output.values_mut() {
        let jobs = std::mem::take(&mut entry.jobs);
        entry.jobs = enhancer::dedup_jobs(jobs);
    }
    // Final sanity: drop companies with zero open jobs after enhancer filtering
    output.retain(|_, e| !e.jobs.is_empty());

    save(&output_file, &output)?;

    info!(
        "From {} companies; Found {} Jobs in {:.1}s (production AI-enhanced)",
        output.len(),
        output.values().map(|f| f.jobs.len()).sum::<usize>(),
        started.elapsed().as_secs_f64()
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

/// What phase 1 produced for a site.
enum SitePrepare {
    /// Structured data, no LLM needed.
    Exact(Vec<JobPost>),
    /// Cleaned Markdown awaiting LLM extraction.
    Markdown(String),
    /// Nothing scrapable (JS-rendered page, empty listing, ...).
    Empty,
}

struct Crawler {
    http: Arc<Http>,
    llm: Llm,
    log_file: Option<Mutex<LogFile>>,
}

impl Crawler {
    fn new(http: Arc<Http>, llm: Llm, log_file: Option<std::path::PathBuf>) -> Result<Self> {
        let log_file = match log_file {
            Some(path) => Some(Mutex::new(open_log_file(path)?)),
            None => None,
        };
        Ok(Self { http, llm, log_file })
    }

    /// Phase 1: fastest path to structured or clean data for one site.
    async fn prepare_site(&self, url: &Url) -> Result<SitePrepare> {
        // 1. Known ATS platform → public JSON API (exact, no LLM).
        if let Some(ats) = Ats::detect(url)
            && let Some(jobs) = ats.fetch_jobs(&self.http).await?
                && !jobs.is_empty() {
                return Ok(SitePrepare::Exact(jobs));
            }

        // 2. Schema.org JobPosting JSON-LD on the page (no LLM).
        let html = self.fetch_html(url).await?;
        let from_ld = ats::from_json_ld(&html);
        if !from_ld.is_empty() {
            return Ok(SitePrepare::Exact(from_ld));
        }

        // 3. Generic: crawl listing pages (pagination-aware) → clean Markdown.
        let markdown = self.crawl_listing(url).await?;

        if markdown.trim().is_empty() {
            warn!("[EMPTY] {url}");
            return Ok(SitePrepare::Empty);
        }

        Ok(SitePrepare::Markdown(markdown))
    }

    /// One LLM call covering `sites` (name + markdown). Returns per-site jobs.
    async fn extract_batch(&self, sites: &[(&str, &str)]) -> Result<Vec<(String, Vec<JobPost>)>> {
        let cache_key = {
            let mut names: Vec<_> = sites.iter().map(|(n, _)| n.to_string()).collect();
            names.sort();
            format!("llm|{}|batch|{}", self.llm.model(), names.join("|"))
        };
        let cache = Cache::open_with_ttl(CACHE_PATH, &cache_key, Some(CACHE_TTL))?;
        if let Some(json) = cache.get()? {
            let value: json::Value = json::from_str(&json).unwrap_or_default();
            return Ok(parse_batch(&value));
        }

        let mut markdown = String::new();
        for (name, md) in sites {
            markdown.push_str(&format!("<!-- SITE: {name} -->\n\n{}\n\n<!-- END SITE -->\n\n", cap_input(md, MAX_LLM_CHARS)));
        }

        self.log_page("batch", &markdown);

        info!(
            "[LLM-CALL] batch of {} site(s) ({} chars)",
            sites.len(),
            markdown.len()
        );

        let schema = batch_schema();
        let value = self
            .llm
            .extract_json(LLM_INPUT, &format!("Extract from this markdown and return the job postings as json.\n\n{markdown}"), &schema)
            .await?;

        let assignments = parse_batch(&value);
        cache.set(&json::to_string(&value)?)?;
        Ok(assignments)
    }

    /// One LLM call for a batch of detail pages (in given order).
    /// Returns one `JobPost` per page (aligned by index), `None` for empty.
    async fn extract_details(&self, pages: &[(&str, &str, Option<&str>)]) -> Result<Vec<Option<JobPost>>> {
        let cache_key = {
            let mut urls: Vec<_> = pages.iter().map(|(url, _, _)| url.to_string()).collect();
            urls.sort();
            format!("llm|{}|details|{}", self.llm.model(), urls.join("|"))
        };
        let cache = Cache::open_with_ttl(CACHE_PATH, &cache_key, Some(CACHE_TTL))?;
        if let Some(json) = cache.get()? {
            let value: json::Value = json::from_str(&json).unwrap_or_default();
            return Ok(parse_details(&value));
        }

        let mut markdown = String::new();
        for (index, (url, title, md)) in pages.iter().enumerate() {
            let md = md.unwrap_or_default();
            markdown.push_str(&format!("<!-- JOB {index}: {url} (expected title: {title}) -->\n\n{}\n\n", cap_input(md, MAX_LLM_CHARS)));
        }

        self.log_page("details", &markdown);

        info!("[LLM-CALL] batch of {} detail page(s)", pages.len());

        let schema = details_schema();
        let value = self
            .llm
            .extract_json(
                r#"Extract job postings from this markdown. The input contains one job posting per page, marked with <!-- JOB index: url -->. Output a JSON object {"jobs": [ ... ]} — one job per page, IN THE SAME ORDER as the pages. If a page has no job posting, output null for that slot. Never invent jobs; do not include on-site jobs outside Bangladesh unless remote. Preserve `source` exactly as provided. Keep `title` unchanged. Confidence below 0.5 → null slot."#,
                &format!("Extract from this markdown and return the job postings as json.\n\n{markdown}"),
                &schema,
            )
            .await?;

        let posts = parse_details(&value);
        cache.set(&json::to_string(&value)?)?;
        Ok(posts)
    }

    fn log_page(&self, url: &str, markdown: &str) {
        if let Some(file) = &self.log_file
            && let Ok(file) = file.lock() {
                let _ = writeln!(file.as_ref(), "---\n{url}\n{}", cap_input(markdown, 12_000));
            }
    }

    /// Instant HTML fetch with a 24h cache. No browser, no waits.
    async fn fetch_html(&self, url: &Url) -> Result<String> {
        let url = normalize_url(url)?;
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
                                if !visited.contains(&key)
                                    && !queue.iter().any(|u| u.as_str() == key)
                                {
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

    /// Fetch one detail page and convert to markdown (JSON-LD short-circuits).
    async fn detail_markdown(&self, url: &Url) -> Result<String> {
        let html = self.fetch_html(url).await?;

        let from_ld = ats::from_json_ld(&html);
        if !from_ld.is_empty() {
            return Ok(format!("<!-- JSON-LD -->\n{}", serde_json::to_string(&from_ld)?));
        }

        normalize_markdown_from(&html)
    }
}

fn batch_schema() -> json::Value {
    json::json!({
        "type": "object",
        "properties": {
            "sites": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "jobs": { "type": "array", "items": json::to_value(schema_for!(JobPost)).unwrap() },
                    },
                    "required": ["name", "jobs"],
                }
            }
        },
        "required": ["sites"],
    })
}

fn details_schema() -> json::Value {
    json::json!({
        "type": "object",
        "properties": {
            "jobs": {
                "type": "array",
                "items": {
                    "anyOf": [
                        json::to_value(schema_for!(JobPost)).unwrap(),
                        { "type": "null" },
                    ]
                }
            }
        },
        "required": ["jobs"],
    })
}

fn parse_batch(value: &json::Value) -> Vec<(String, Vec<JobPost>)> {
    value
        .get("sites")
        .and_then(json::Value::as_array)
        .map(|sites| {
            sites
                .iter()
                .filter_map(|site| {
                    let name = site.get("name")?.as_str()?.to_string();
                    let jobs: Vec<JobPost> = site
                        .get("jobs")
                        .and_then(json::Value::as_array)
                        .map(|jobs| jobs.iter().filter_map(|j| json::from_value(j.clone()).ok()).collect())
                        .unwrap_or_default();
                    Some((name, jobs))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_details(value: &json::Value) -> Vec<Option<JobPost>> {
    value
        .get("jobs")
        .and_then(json::Value::as_array)
        .map(|jobs| {
            jobs.iter()
                .map(|j| {
                    if j.is_null() {
                        None
                    } else {
                        json::from_value(j.clone()).ok()
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Find pagination links on a listing page: `<link rel="next">`, `?page=N`
/// query links, and next/› anchors. Same-origin, same path.
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

use crate::utils::resolve_url;

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
