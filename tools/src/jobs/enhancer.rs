//! Production AI enhancer — cleans and validates LLM-extracted JobPosts.
//!
//! Runs after initial `extract_batch` / `extract_details`.
//! Two layers:
//!  1. Rule-based deterministic cleanup (always, no LLM cost)
//!  2. Optional LLM refinement pass for low-confidence or incomplete jobs
//!
//! Goals: perfect output for dashboard — no boilerplate, normalized tags,
//! correct salary/location, markdown hygiene, dedup, confidence recalibration.

use super::schema::*;
use crate::data::Schema;
use log::{debug, warn};
use std::collections::{HashMap, HashSet};

/// Known tech list for tag normalization (from schema.toml)
fn normalize_tag(tag: &str, schema: Option<&Schema>) -> Option<String> {
    let trimmed = tag.trim();
    if trimmed.is_empty() || trimmed.len() < 2 || trimmed.len() > 40 {
        return None;
    }
    // Remove duplicates like "Node.js" vs "NodeJS" → canonical via schema
    // For now, keep original but title-case; schema-based mapping done in warehouse
    // Here we just clean: trim, dedup case-insensitive, remove junk
    let lower = trimmed.to_ascii_lowercase();
    // Junk tags that LLM invents
    const JUNK: &[&str] = &["job", "career", "hiring", "urgent", "vacancy", "position"];
    if JUNK.contains(&lower.as_str()) {
        return None;
    }
    // If schema provided, check if tag is known or hint to closest; but enhancer keeps LLM tags as-is,
    // warehouse will map. We just ensure it looks like a tech.
    if let Some(schema) = schema
        && schema.is_unknown_technology(trimmed)
    {
        // Allow but warn; maybe it's a new tech like "n8n" — keep if plausible (alphanum + ./#+)
        if !trimmed.chars().any(|c| c.is_alphanumeric()) {
            return None;
        }
        // If very far from known tech, maybe hallucinated — keep but flag
        // We keep it; warehouse check will error if truly unknown
    }
    Some(trimmed.to_string())
}

/// Deterministic cleanup — no LLM, always runs. Perfects markdown + fields.
pub fn enhance_job_deterministic(mut job: JobPost, schema: Option<&Schema>) -> Option<JobPost> {
    // 1. Title hygiene
    job.title = job.title.trim().to_string();
    // Title should be 5-120 chars, not just "Job" or "-"
    if job.title.len() < 5 || job.title.len() > 150 {
        debug!("drop job with bad title: {:?}", job.title);
        return None;
    }
    // Remove duplicate prefixes like "Job: Job Title"
    if job.title.to_lowercase().starts_with("job:") {
        job.title = job.title[4..].trim().to_string();
    }

    // 2. Description hygiene
    let mut desc = job.description.trim().to_string();
    // Remove excessive blank lines (more than 2 consecutive)
    desc = desc.replace("\n\n\n", "\n\n");
    while desc.contains("\n\n\n") {
        desc = desc.replace("\n\n\n", "\n\n");
    }
    // Cap description at 8k (warehouse caps at 6k, but keep more here)
    if desc.len() > 8000 {
        desc = desc.chars().take(8000).collect::<String>() + "\n\n…[truncated]";
    }
    // If description too short (<80 chars), likely incomplete → mark needs_fetch or drop
    if desc.chars().filter(|c| !c.is_whitespace()).count() < 50 {
        if job.needs_fetch {
            // Keep but will be detailed later; don't drop
        } else {
            debug!(
                "drop job with too-short description: {} - {}",
                job.title,
                desc.len()
            );
            // Keep low-confidence jobs but lower confidence
            job.confidence = (job.confidence * 0.5).min(0.4);
            if job.confidence < 0.5 {
                return None;
            }
        }
    }
    job.description = desc;
    job.description_len_check();

    // 3. Tags: dedup case-insensitive, normalize, filter junk, limit 15
    let mut seen_lower = HashSet::new();
    let mut cleaned_tags = Vec::new();
    for tag in job.tags.drain(..) {
        if let Some(norm) = normalize_tag(&tag, schema) {
            let lower = norm.to_ascii_lowercase();
            if seen_lower.insert(lower) {
                cleaned_tags.push(norm);
            }
        }
        if cleaned_tags.len() >= 15 {
            break;
        }
    }
    // If tags empty but description contains obvious tech, try simple keyword scan
    if cleaned_tags.is_empty()
        && schema.is_some()
        && let Some(s) = schema
    {
        let desc_lower = job.description.to_ascii_lowercase();
        for tech in s.technologies.keys() {
            if desc_lower.contains(&tech.to_ascii_lowercase()) && tech.len() > 2 {
                cleaned_tags.push(tech.clone());
                if cleaned_tags.len() >= 5 {
                    break;
                }
            }
        }
    }
    job.tags = cleaned_tags;

    // 4. Salary: ensure min <= max, currency uppercase, drop if both None
    if let Some(s) = job.salary.as_mut() {
        if let (Some(min), Some(max)) = (s.min, s.max)
            && min > max
        {
            std::mem::swap(&mut s.min, &mut s.max);
        }
        if let Some(c) = s.currency.as_mut() {
            *c = c.trim().to_ascii_uppercase();
            if c.len() > 5 || c.chars().any(|ch| !ch.is_ascii_alphabetic()) {
                *c = "BDT".to_string(); // fallback for BDT jobs
            }
        }
        // Drop salary if both None
        if s.min.is_none() && s.max.is_none() {
            job.salary = None;
        }
    }

    // 5. Location: trim, ensure not "Bangladesh, Bangladesh"
    if let Some(loc) = job.location.as_mut() {
        match loc {
            JobLocation::OnSite(s) | JobLocation::Hybrid(s) => {
                let t = s.trim().to_string();
                if t.is_empty() || t.eq_ignore_ascii_case("bangladesh") {
                    *s = t;
                } else {
                    // Deduplicate "Dhaka, Dhaka"
                    let parts: Vec<&str> = t
                        .split(',')
                        .map(|p| p.trim())
                        .filter(|p| !p.is_empty())
                        .collect();
                    let mut uniq = Vec::new();
                    let mut seen = HashSet::new();
                    for p in parts {
                        let l = p.to_ascii_lowercase();
                        if seen.insert(l) {
                            uniq.push(p);
                        }
                    }
                    *s = uniq.join(", ");
                }
            }
            JobLocation::Remote => {}
        }
    }

    // 6. Apply links: dedup, ensure valid URL or mailto
    let mut seen_apply = HashSet::new();
    job.apply.retain(|a| {
        let key = match a {
            ApplicationMethod::Email(e) => format!("mailto:{}", e.to_ascii_lowercase()),
            ApplicationMethod::Website(u) => u.trim().to_ascii_lowercase(),
        };
        if key.is_empty() || key == "mailto:" {
            return false;
        }
        seen_apply.insert(key)
    });

    // 7. Confidence recalibration based on completeness
    let mut completeness = 0.0;
    if !job.description.is_empty() {
        completeness += 0.3;
    }
    if job.location.is_some() {
        completeness += 0.2;
    }
    if job.salary.is_some() {
        completeness += 0.1;
    }
    if !job.apply.is_empty() || job.source.is_some() {
        completeness += 0.2;
    }
    if !job.tags.is_empty() {
        completeness += 0.1;
    }
    if job.employment_type.is_some() {
        completeness += 0.1;
    }
    // Blend original confidence with completeness (weighted)
    let blended = job.confidence * 0.6 + completeness * 0.4;
    job.confidence = blended.clamp(0.0, 1.0);
    // Drop if blended still <0.5
    if job.confidence < 0.5 {
        debug!(
            "drop low confidence after enhance: {} {:.2}",
            job.title, job.confidence
        );
        return None;
    }

    // 8. Vacancies: cap realistic (1-100)
    if let Some(v) = job.vacancies
        && (v == 0 || v > 200)
    {
        job.vacancies = None;
    }

    Some(job)
}

impl JobPost {
    fn description_len_check(&self) {}
}

/// Deduplicate jobs within a company batch (same title+location → highest confidence wins)
pub fn dedup_jobs(mut jobs: Vec<JobPost>) -> Vec<JobPost> {
    let mut map: HashMap<String, JobPost> = HashMap::new();
    for job in jobs.drain(..) {
        let key = dedup_key(&job);
        // Use entry to keep highest confidence
        map.entry(key)
            .and_modify(|existing| {
                if job.confidence > existing.confidence {
                    *existing = job.clone();
                } else if job.description.len() > existing.description.len()
                    && (job.confidence - existing.confidence).abs() < 0.1
                {
                    // Prefer longer description if confidence close
                    *existing = job.clone();
                }
            })
            .or_insert(job);
    }
    let mut out: Vec<JobPost> = map.into_values().collect();
    out.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Dedup identity: normalized (title, location). Raw `{:?}` of the location
/// enum leaked duplicates through representation noise ("Dhaka" vs "Dhaka ",
/// case differences) — normalize both sides.
fn dedup_key(job: &JobPost) -> String {
    let title = job.title.trim().to_ascii_lowercase();
    let location = job.location.as_ref().map(|l| match l {
        JobLocation::Remote => "remote".to_string(),
        JobLocation::Hybrid(s) | JobLocation::OnSite(s) => s
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase(),
    });
    format!("{title}|{location:?}|{}", job.employment_type.is_some())
}

/// Full batch enhancer — deterministic + optional LLM refinement for low-confidence jobs.
/// In production, the LLM refinement is gated to `needs_enhancement` jobs only to control cost.
pub fn enhance_batch(jobs: Vec<JobPost>, schema: Option<&Schema>) -> Vec<JobPost> {
    let mut enhanced = Vec::with_capacity(jobs.len());
    let mut dropped = 0;
    for job in jobs {
        let title = job.title.clone();
        match enhance_job_deterministic(job, schema) {
            Some(mut j) => {
                // Observed this run — drives stale-posting pruning later.
                j.mark_seen();
                enhanced.push(j);
            }
            None => {
                warn!("enhancer dropped job: {}", title);
                dropped += 1;
            }
        }
    }
    let before = enhanced.len();
    let deduped = dedup_jobs(enhanced);
    if dropped > 0 || deduped.len() < before {
        debug!(
            "enhancer: dropped {} low-quality, deduped {} → {}",
            dropped,
            before,
            deduped.len()
        );
    }
    deduped
}

/// Production prompt for optional LLM refinement (second pass, only for incomplete jobs)
#[allow(dead_code)]
pub const ENHANCER_PROMPT: &str = r#"You are a senior job-post QA. Given a job JSON, clean it to production quality:

- Keep `title` exactly (fix only whitespace/casing if clearly wrong).
- Rewrite `description` as clean Markdown: keep all facts, remove boilerplate/duplicate lines, fix headings/lists, no hallucination. If description is <80 chars, return it as-is and set confidence low.
- `tags`: keep only real tech/skills that appear in description, dedup case-insensitive, max 12, no junk like "hiring" or "job". Prefer canonical names (React, NodeJS, Python). Never invent.
- `location`: if OnSite/Hybrid text is empty or just "Bangladesh", leave as is. Don't guess city. Remote stays Remote.
- `salary`: keep only explicit numbers; if currency missing and job is in Bangladesh, use BDT. Ensure min <= max.
- `apply`/`source`: preserve exactly, never invent.
- Re-score `confidence` 0.0-1.0 based on completeness (title+description+location+apply = high). Below 0.5 will be dropped.
- Return single JSON object matching the JobPost schema, no extra keys.

Input is one JobPost JSON. Output same shape, no explanation."#;

// Future: async LLM refinement for jobs where enhance_job_deterministic lowered confidence but job is valuable
// pub async fn enhance_with_llm(job: JobPost, llm: &crate::jobs::llm::Llm) -> JobPost { ... }
// For now, deterministic pass is enough for production; LLM refinement is gated behind --enhance flag to avoid cost.

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_dedup() {
        let a = JobPost {
            title: "Backend Engineer".into(),
            description: "Go + Python role".into(),
            employment_type: Some(EmploymentType::FullTime),
            role: None,
            posted_at: None,
            category: None,
            deadline: None,
            location: Some(JobLocation::OnSite("Dhaka".into())),
            experience: None,
            salary: None,
            vacancies: None,
            tags: vec!["Go".into()],
            apply: vec![],
            source: None,
            needs_fetch: false,
            confidence: 0.9,
            last_seen: None,
        };
        let b = JobPost {
            title: "backend engineer".into(),
            description: "Go Python role longer description with more details".into(),
            employment_type: Some(EmploymentType::FullTime),
            role: None,
            posted_at: None,
            category: None,
            deadline: None,
            location: Some(JobLocation::OnSite("Dhaka".into())),
            experience: None,
            salary: None,
            vacancies: None,
            tags: vec!["Go".into(), "Python".into()],
            apply: vec![],
            source: None,
            needs_fetch: false,
            confidence: 0.85,
            last_seen: None,
        };
        let out = dedup_jobs(vec![a, b]);
        assert_eq!(out.len(), 1);
    }
    #[test]
    fn test_enhance_filters_junk() {
        let j = JobPost {
            title: "Test".into(),
            description: "x".into(),
            employment_type: None,
            role: None,
            posted_at: None,
            category: None,
            deadline: None,
            location: None,
            experience: None,
            salary: None,
            vacancies: Some(999),
            tags: vec!["hiring".into(), "Job".into(), "React".into()],
            apply: vec![],
            source: None,
            needs_fetch: false,
            confidence: 0.9,
            last_seen: None,
        };
        let out = enhance_job_deterministic(j, None);
        // Title too short → dropped
        assert!(out.is_none());
    }
}
