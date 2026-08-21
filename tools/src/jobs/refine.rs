//! Phase 4: batched AI data refinement.
//!
//! After collection, every job is sent to the LLM in large batches (~20 per
//! call) as plain JSON with stable integer ids, and returned cleaned:
//! markdown hygiene, canonical tech tags, normalized salary/location/deadline,
//! and a recalibrated confidence. Ids keep parsing trivial and lossless —
//! the response is a dict-shaped `{"jobs": [...]}` aligned by `id`, so no
//! fuzzy matching against prose ever happens.

use super::schema::*;
use super::{input_normalizer::cap_input, llm::Llm};
use crate::{Result, utils::cache::Cache};
use log::{debug, info, warn};
use serde::Deserialize;
use serde_json as json;
use std::time::Duration;

/// Jobs per refinement call. Large enough to amortize latency and cost,
/// small enough that one malformed answer can't wipe the whole dataset.
pub const REFINE_BATCH: usize = 20;

const CACHE_PATH: &str = "job-postings-cache";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Hard cap for one refinement call (retries included).
const CALL_CAP_SECS: u64 = 240;

pub const REFINE_PROMPT: &str = r#"You are a production data-cleaning engine for a Bangladeshi job-postings dataset consumed by analysts and applications downstream.

Input: a JSON object {"jobs":[ ... ]}. Each job has a stable integer "id" plus raw extracted fields.
Output: a JSON object {"jobs":[ ... ]} containing EVERY input id EXACTLY ONCE, with cleaned fields. Never omit an id. Never add ids. Never add extra keys. Output raw JSON only — no markdown fences, no commentary.

Cleaning rules per job:
- title: trim and fix spacing/casing ONLY. Never invent, translate, or reword.
- description: clean Markdown, max ~4000 chars. Remove navigation/footer boilerplate, repeated headers, duplicate blank lines; fix broken headings and lists; keep every factual detail (requirements, responsibilities, benefits, contact). NEVER invent content.
- location: null, or exactly one of: "Remote", "Hybrid: <area>", "On-site: <area>". <area> is a Bangladesh city/area unless the posting clearly states otherwise; remote jobs abroad are still "Remote".
- employmentType: null or exactly one of FullTime, PartTime, Contract, Temporary, Internship, Freelance. Map literally from the text; do not guess.
- salaryMin / salaryMax / salaryCurrency: only explicit numbers from the text. Convert "30k" to 30000. Monthly amounts assumed BDT for Bangladesh jobs ("salaryCurrency":"BDT") unless another currency is stated. Negotiable/unstated -> all three null. Ensure min <= max. NEVER invent numbers.
- deadline: null or ISO "YYYY-MM-DD" parsed from the text. If the posting says applications are closed, use "EXPIRED".
- postedAt: null or ISO "YYYY-MM-DD" if the text states when the job was posted.
- tags: 3-12 canonical technology/skill names that literally appear in the description (examples: React, NodeJS, TypeScript, PostgreSQL, Docker, Flutter, Figma). Deduplicate case-insensitively. No junk words ("job", "hiring", "career", "team", "communication"). Never invent tags not supported by the description.
- confidence: number 0.5-1.0 scoring overall completeness and trustworthiness (title + real description + location + way to apply = high). Entries that are spam, duplicates-within-batch, or empty shells must get confidence below 0.5 so downstream drops them."#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefinedJob {
    id: u32,
    title: Option<String>,
    description: Option<String>,
    location: Option<String>,
    employment_type: Option<String>,
    salary_min: Option<u32>,
    salary_max: Option<u32>,
    salary_currency: Option<String>,
    deadline: Option<String>,
    posted_at: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    confidence: Option<f32>,
}

fn refine_schema() -> json::Value {
    json::json!({
        "type": "object",
        "properties": {
            "jobs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "title": { "type": ["string", "null"] },
                        "description": { "type": ["string", "null"] },
                        "location": { "type": ["string", "null"] },
                        "employmentType": { "type": ["string", "null"] },
                        "salaryMin": { "type": ["number", "null"] },
                        "salaryMax": { "type": ["number", "null"] },
                        "salaryCurrency": { "type": ["string", "null"] },
                        "deadline": { "type": ["string", "null"] },
                        "postedAt": { "type": ["string", "null"] },
                        "tags": { "type": ["array", "null"], "items": { "type": "string" } },
                        "confidence": { "type": ["number", "null"] }
                    },
                    "required": ["id"]
                }
            }
        },
        "required": ["jobs"]
    })
}

fn parse_employment(s: &str) -> Option<EmploymentType> {
    match s.to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
        "fulltime" => Some(EmploymentType::FullTime),
        "parttime" => Some(EmploymentType::PartTime),
        "contract" => Some(EmploymentType::Contract),
        "temporary" => Some(EmploymentType::Temporary),
        "internship" => Some(EmploymentType::Internship),
        "freelance" => Some(EmploymentType::Freelance),
        _ => None,
    }
}

fn parse_location(s: &str) -> Option<JobLocation> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let low = s.to_ascii_lowercase();
    if low == "remote" {
        return Some(JobLocation::Remote);
    }
    if let Some(rest) = low.strip_prefix("hybrid:") {
        let area = s[s.len() - rest.len()..].trim();
        return Some(JobLocation::Hybrid(area.to_string()));
    }
    if let Some(rest) = low.strip_prefix("on-site:") {
        let area = s[s.len() - rest.len()..].trim();
        return Some(JobLocation::OnSite(area.to_string()));
    }
    Some(JobLocation::OnSite(s.to_string()))
}

/// Refine every job currently in `output`, in place.
/// Jobs are addressed by their position (`gid`) in a stable snapshot of
/// (company, job-index) pairs, so responses align by plain integer lookup.
pub async fn run(
    llm: &Llm,
    output: &mut super::Jobs,
    started: std::time::Instant,
    deadline_secs: u64,
) -> Result {
    let mut index: Vec<(String, usize)> = Vec::new();
    for (name, entry) in output.iter() {
        for i in 0..entry.jobs.len() {
            index.push((name.clone(), i));
        }
    }
    let total = index.len();
    if total == 0 {
        return Ok(());
    }

    info!("[REFINE] cleaning {total} job(s) in batches of {REFINE_BATCH}...");

    for chunk_start in (0..total).step_by(REFINE_BATCH) {
        if started.elapsed().as_secs() >= deadline_secs {
            warn!("[REFINE] out of time budget; remaining jobs kept un-refined");
            break;
        }
        let chunk_end = (chunk_start + REFINE_BATCH).min(total);

        let mut payload_jobs = Vec::with_capacity(chunk_end - chunk_start);
        for (gid, (name, i)) in index
            .iter()
            .enumerate()
            .skip(chunk_start)
            .take(chunk_end - chunk_start)
        {
            let job = &output[name].jobs[*i];
            payload_jobs.push(json::json!({
                "id": gid,
                "company": name,
                "title": job.title,
                "description": cap_input(&job.description, 1500),
                "employmentType": job.employment_type.as_ref().map(format_employment),
                "location": job.location.as_ref().map(format_location),
                "salary": job.salary.as_ref().map(|s| json::json!({
                    "min": s.min, "max": s.max, "currency": s.currency
                })),
                "deadline": job.deadline.as_ref().map(format_deadline),
                "postedAt": job.posted_at.as_ref().map(|p| match p {
                    PostedAt::Absolute(s) | PostedAt::Relative(s) => s.clone(),
                }),
                "tags": job.tags,
                "applyCount": job.apply.len(),
                "hasSource": job.source.is_some(),
                "confidence": job.confidence,
            }));
        }

        let payload = json::to_string(&json::json!({ "jobs": payload_jobs }))?;
        let cache_key = format!("llm|{}|refine|{:x}", llm.model(), fnv1a(payload.as_bytes()));
        let cache = Cache::open_with_ttl(CACHE_PATH, &cache_key, Some(CACHE_TTL))?;

        let value = match cache.get()? {
            Some(cached) => json::from_str::<json::Value>(&cached).unwrap_or_default(),
            None => {
                let user =
                    format!("Clean these job postings. Return every id exactly once.\n\n{payload}");
                let budget = Duration::from_secs(
                    deadline_secs
                        .saturating_sub(started.elapsed().as_secs())
                        .min(CALL_CAP_SECS),
                );
                match tokio::time::timeout(
                    budget,
                    llm.extract_json(REFINE_PROMPT, &user, &refine_schema()),
                )
                .await
                {
                    Ok(Ok(v)) => {
                        cache.set(&json::to_string(&v)?)?;
                        v
                    }
                    Ok(Err(err)) => {
                        warn!("[REFINE-BATCH-ERROR] {err}");
                        continue;
                    }
                    Err(_) => {
                        warn!("[REFINE-TIMEOUT] batch exceeded its budget; skipping");
                        continue;
                    }
                }
            }
        };

        apply_refined(&value, &index, output);
    }

    Ok(())
}

fn apply_refined(value: &json::Value, index: &[(String, usize)], output: &mut super::Jobs) {
    let Some(jobs) = value.get("jobs").and_then(json::Value::as_array) else {
        return;
    };
    let mut applied = 0usize;
    for rj in jobs {
        let Ok(r) = json::from_value::<RefinedJob>(rj.clone()) else {
            continue;
        };
        let Some((company, i)) = index.get(r.id as usize) else {
            continue;
        };
        let Some(entry) = output.get_mut(company) else {
            continue;
        };
        let Some(job) = entry.jobs.get_mut(*i) else {
            continue;
        };

        if let Some(t) = r.title.as_deref() {
            let t = t.trim();
            if t.len() >= 5 && t.len() <= 150 {
                job.title = t.to_string();
            }
        }
        if let Some(d) = r.description
            && d.chars().count() > 80
        {
            job.description = d;
        }
        if let Some(loc) = r.location.as_deref().and_then(parse_location) {
            job.location = Some(loc);
        }
        if let Some(et) = r.employment_type.as_deref().and_then(parse_employment) {
            job.employment_type = Some(et);
        }
        if r.salary_min.is_some() || r.salary_max.is_some() {
            let (min, max) = match (r.salary_min, r.salary_max) {
                (Some(a), Some(b)) if a > b => (Some(b), Some(a)),
                (a, b) => (a, b),
            };
            job.salary = Some(Salary {
                min,
                max,
                currency: r.salary_currency.clone(),
            });
        }
        match r.deadline.as_deref() {
            Some("EXPIRED") => job.deadline = Some(Deadline::Expired),
            Some(date) if is_iso_date(date) => {
                job.deadline = Some(Deadline::Date(PostedAt::Absolute(date.to_string())))
            }
            _ => {}
        }
        if let Some(p) = r.posted_at.filter(|p| is_iso_date(p)) {
            job.posted_at = Some(PostedAt::Absolute(p));
        }
        if !r.tags.is_empty() {
            job.tags = r.tags.clone();
        }
        if let Some(c) = r.confidence {
            job.confidence = c.clamp(0.0, 1.0);
        }
        applied += 1;
    }
    debug!("[REFINE] applied {applied} refined job(s)");
}

// --- tiny helpers -----------------------------------------------------------

/// Strict `YYYY-MM-DD` check — the LLM sometimes returns prose dates
/// ("August 2025") that would poison downstream ISO parsing.
fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn format_employment(e: &EmploymentType) -> String {
    match e {
        EmploymentType::FullTime => "FullTime".into(),
        EmploymentType::PartTime => "PartTime".into(),
        EmploymentType::Contract => "Contract".into(),
        EmploymentType::Temporary => "Temporary".into(),
        EmploymentType::Internship => "Internship".into(),
        EmploymentType::Freelance => "Freelance".into(),
    }
}

fn format_location(l: &JobLocation) -> String {
    match l {
        JobLocation::Remote => "Remote".into(),
        JobLocation::Hybrid(s) => format!("Hybrid: {s}"),
        JobLocation::OnSite(s) => format!("On-site: {s}"),
    }
}

fn format_deadline(d: &Deadline) -> String {
    match d {
        Deadline::Expired => "EXPIRED".into(),
        Deadline::Date(PostedAt::Absolute(s)) | Deadline::Date(PostedAt::Relative(s)) => s.clone(),
    }
}

/// FNV-1a — cheap stable hash for cache keys.
fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
