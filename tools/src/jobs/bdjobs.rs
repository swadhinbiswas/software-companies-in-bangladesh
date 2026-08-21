//! BDJobs.com board ingestion — Bangladesh's largest job portal.
//!
//! Every employer posts here, including the ~40% of companies whose own
//! career sites are JS-rendered or social-only. Two public JSON APIs
//! (no auth):
//!   - List:  GET gateway.bdjobs.com/recruitment-account-test/api/JobSearch/
//!     GetJobSearch?isPro=1&rpp=50&pg=N&keyword=<kw>
//!   - Detail: GET gateway.bdjobs.com/ActtivejobsTest/api/JobSubsystem/
//!     jobDetails?jobId=<id>
//!
//! IT focus: we fan out over software/IT keywords, dedupe by `Jobid`, then
//! hydrate each hit from the details API (full HTML description, salary,
//! skills, benefits). Jobs are grouped under their employer name so they
//! merge with existing company entries in `job-posts.json`.

use super::input_normalizer;
use super::schema::*;
use crate::Result;
use crate::utils::http::Http;
use futures::StreamExt;
use log::{debug, info, warn};
use serde::Deserialize;
use serde_json as json;
use url::Url;

/// IT/software keyword seeds. Each hits the search API; results are merged
/// and deduped, so overlap between keywords is harmless.
const KEYWORDS: &[&str] = &[
    "software",
    "software engineer",
    "developer",
    "programmer",
    "web developer",
    "backend",
    "frontend",
    "full stack",
    "devops",
    "python",
    "java",
    "react",
    "node",
    ".net",
    "php",
    "laravel",
    "android",
    "flutter",
    "react native",
    "ios developer",
    "qa",
    "quality assurance",
    "data engineer",
    "data scientist",
    "machine learning",
    "system analyst",
    "cloud",
    "aws",
    "cyber security",
    "network engineer",
    "database",
    "dba",
    "scrum master",
    "product owner",
    "ui ux designer",
    "it executive",
    "it support",
    "sap",
    "oracle",
    "technical lead",
    "software architect",
];

/// Pages fetched per keyword (50 jobs/page). Overlap across keywords is
/// deduped, this just bounds worst-case request volume.
const PAGES_PER_KEYWORD: u32 = 2;
/// Hard cap on hydrated details — bounds runtime and stays polite.
const MAX_DETAILS: usize = 600;
const RPP: u32 = 50;

#[derive(Deserialize)]
struct ListResponse {
    #[serde(default)]
    data: Vec<ListJob>,
    #[serde(rename = "premiumData", default)]
    premium: Vec<ListJob>,
}

#[derive(Deserialize)]
struct ListJob {
    #[serde(rename = "Jobid")]
    id: String,
    #[serde(rename = "jobTitle")]
    #[allow(dead_code)]
    title: String,
}

#[derive(Deserialize)]
struct DetailResponse {
    statuscode: String,
    #[serde(default)]
    data: Vec<DetailJob>,
}

/// BDJobs returns JSON `null` for empty fields. `serde(default)` only covers
/// *missing* keys, so every string field needs a null-tolerant deserializer
/// — one unexpected null otherwise poisons the whole response.
fn de_str<'de, D>(d: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// Accepts a JSON number, numeric string, or null.
fn de_num<'de, D>(d: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Option::<json::Value>::deserialize(d)?;
    Ok(match v {
        Some(json::Value::Number(n)) => n.as_u64(),
        Some(json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct DetailJob {
    JobId: String,
    #[serde(default, deserialize_with = "de_str")]
    JobFound: String,
    #[serde(default, deserialize_with = "de_str", rename = "CompnayName")]
    company: String,
    #[serde(default, deserialize_with = "de_str", rename = "JobTitle")]
    title: String,
    #[serde(default, deserialize_with = "de_str")]
    JobDescription: String,
    #[serde(default, deserialize_with = "de_str")]
    JobNature: String,
    #[serde(default, deserialize_with = "de_str")]
    JobLocation: String,
    #[serde(default, deserialize_with = "de_str")]
    JobVacancies: String,
    #[serde(default, deserialize_with = "de_str")]
    DeadlineDB: String,
    #[serde(default, deserialize_with = "de_str")]
    PostedOn: String,
    #[serde(default, deserialize_with = "de_str")]
    EducationRequirements: String,
    #[serde(default, deserialize_with = "de_str")]
    experience: String,
    #[serde(default, deserialize_with = "de_str")]
    AdditionJobRequirements: String,
    #[serde(default, deserialize_with = "de_str")]
    JobOtherBenifits: String,
    #[serde(default, deserialize_with = "de_str")]
    JobKeyPoints: String,
    #[serde(default, deserialize_with = "de_str")]
    SkillsRequired: String,
    #[serde(default, deserialize_with = "de_str")]
    SuggestedSkills: String,
    #[serde(default, deserialize_with = "de_num")]
    JobSalaryMinSalary: Option<u64>,
    #[serde(default, deserialize_with = "de_num")]
    JobSalaryMaxSalary: Option<u64>,
    #[serde(default, deserialize_with = "de_str")]
    ApplyEmail: String,
}

fn detail_url(id: &str) -> String {
    format!("https://jobs.bdjobs.com/jobdetails.asp?id={id}&ln=1")
}

/// Fetch IT/software jobs from the BDJobs board, grouped by employer.
pub async fn fetch(http: &Http, concurrent: usize) -> Result<Vec<(String, Vec<JobPost>)>> {
    // ── 1. Fan out over keywords, collect unique ids. ────────────────────
    let mut seen = std::collections::HashSet::new();
    let mut ids: Vec<(String, String)> = Vec::new(); // (Jobid, title)

    for kw in KEYWORDS {
        for pg in 1..=PAGES_PER_KEYWORD {
            let url = Url::parse(&format!(
                "https://gateway.bdjobs.com/recruitment-account-test/api/JobSearch/GetJobSearch?isPro=1&rpp={RPP}&pg={pg}&keyword={}",
                urlencode(kw)
            ))?;
            let resp: Result<ListResponse> = http.get_json(&url).await;
            match resp {
                Ok(r) => {
                    for job in r.data.into_iter().chain(r.premium) {
                        if seen.insert(job.id.clone()) {
                            ids.push((job.id, job.title));
                        }
                    }
                }
                Err(err) => debug!("[BDJOBS] list {kw} p{pg}: {err}"),
            }
        }
    }
    ids.truncate(MAX_DETAILS);
    info!(
        "[BDJOBS] {} unique IT job(s) listed; hydrating details...",
        ids.len()
    );

    // ── 2. Hydrate details in parallel. ──────────────────────────────────
    let results: Vec<Option<(String, JobPost)>> = futures::stream::iter(ids)
        .map(|(id, _title)| {
            let http = &http;
            async move { hydrate(http, &id).await }
        })
        .buffer_unordered(concurrent.max(1))
        .collect()
        .await;

    // ── 3. Group by employer. ────────────────────────────────────────────
    let mut groups: std::collections::BTreeMap<String, Vec<JobPost>> =
        std::collections::BTreeMap::new();
    let mut dropped = 0usize;
    for (employer, job) in results.into_iter().flatten() {
        let key = clean_employer(&employer);
        if key.is_empty() {
            dropped += 1;
            continue;
        }
        groups.entry(key).or_default().push(job);
    }
    if dropped > 0 {
        warn!("[BDJOBS] dropped {dropped} job(s) with empty employer");
    }

    let total: usize = groups.values().map(|v| v.len()).sum();
    info!(
        "[BDJOBS] ingested {total} job(s) across {} employer(s)",
        groups.len()
    );
    Ok(groups.into_iter().collect())
}

async fn hydrate(http: &Http, id: &str) -> Option<(String, JobPost)> {
    let url = Url::parse(&format!(
        "https://gateway.bdjobs.com/ActtivejobsTest/api/JobSubsystem/jobDetails?jobId={id}"
    ))
    .ok()?;
    let resp: DetailResponse = match http.get_json(&url).await {
        Ok(r) => r,
        Err(err) => {
            debug!("[BDJOBS] detail {id}: {err}");
            return None;
        }
    };
    let d = resp.data.into_iter().next()?;
    if resp.statuscode != "0" || d.JobFound != "True" || d.title.is_empty() {
        return None;
    }

    // Compose full description from all content sections.
    let mut html = String::new();
    for section in [
        &d.JobKeyPoints,
        &d.JobDescription,
        &d.EducationRequirements,
        &d.experience,
        &d.AdditionJobRequirements,
        &d.JobOtherBenifits,
    ] {
        if !section.trim().is_empty() {
            html.push_str(section);
            html.push_str("<hr>");
        }
    }
    let description = html_to_md(&html);

    let mut apply = Vec::new();
    if !d.ApplyEmail.trim().is_empty() {
        // Field often contains prose; extract the first email address.
        if let Some(email) = first_email(&d.ApplyEmail) {
            apply.push(ApplicationMethod::Email(email));
        }
    }
    apply.push(ApplicationMethod::Website(detail_url(&d.JobId)));

    let salary = {
        let min = d.JobSalaryMinSalary.unwrap_or(0);
        let max = d.JobSalaryMaxSalary.unwrap_or(0);
        (min > 0 || max > 0).then_some(Salary {
            min: (min > 0).then_some(min as u32),
            max: (max > 0).then_some(max as u32),
            currency: Some("BDT".into()),
        })
    };

    let employer = clean_employer(&d.company);
    Some((
        employer,
        JobPost {
            title: d.title.trim().to_string(),
            description,
            employment_type: parse_nature(&d.JobNature),
            role: None,
            posted_at: (!d.PostedOn.trim().is_empty())
                .then(|| PostedAt::Absolute(d.PostedOn.trim().to_string())),
            category: Some(Category::Technology),
            deadline: parse_deadline_db(&d.DeadlineDB),
            location: (!d.JobLocation.trim().is_empty())
                .then(|| JobLocation::OnSite(d.JobLocation.trim().to_string())),
            experience: None,
            salary,
            vacancies: d.JobVacancies.trim().parse().ok().filter(|v| *v > 0),
            tags: split_skills(&d.SkillsRequired)
                .into_iter()
                .chain(split_skills(&d.SuggestedSkills))
                .take(15)
                .collect(),
            apply,
            source: Some(detail_url(&d.JobId)),
            needs_fetch: false,
            confidence: 1.0,
            last_seen: None,
        },
    ))
}

// ── helpers ─────────────────────────────────────────────────────────────

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out.trim_end().to_string()
}

fn clean_employer(raw: &str) -> String {
    raw.split('|')
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_end_matches('-')
        .trim()
        .to_string()
}

fn parse_nature(s: &str) -> Option<EmploymentType> {
    let low = s.to_ascii_lowercase();
    if low.contains("full") {
        Some(EmploymentType::FullTime)
    } else if low.contains("part") {
        Some(EmploymentType::PartTime)
    } else if low.contains("contract") {
        Some(EmploymentType::Contract)
    } else if low.contains("intern") {
        Some(EmploymentType::Internship)
    } else if low.contains("freelance") {
        Some(EmploymentType::Freelance)
    } else if low.contains("temp") {
        Some(EmploymentType::Temporary)
    } else {
        None
    }
}

/// `DeadlineDB` is `MM/DD/YYYY HH:MM:SS`.
fn parse_deadline_db(s: &str) -> Option<Deadline> {
    let date = s.trim().split(' ').next()?;
    let mut parts = date.split('/');
    let m: usize = parts.next()?.parse().ok()?;
    let d: usize = parts.next()?.parse().ok()?;
    let y: i32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 2000 {
        return None;
    }
    Some(Deadline::Date(PostedAt::Absolute(format!(
        "{y:04}-{m:02}-{d:02}"
    ))))
}

fn split_skills(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() >= 2 && s.len() <= 40)
        .collect()
}

fn first_email(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i].is_ascii_alphanumeric() {
            let mut end = i;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric()
                    || matches!(bytes[end], b'@' | b'.' | b'-' | b'_'))
            {
                end += 1;
            }
            let candidate = &text[i..end];
            if candidate.contains('@') && candidate.ends_with(|c: char| c.is_ascii_alphanumeric()) {
                return Some(candidate.to_ascii_lowercase());
            }
        }
    }
    None
}

fn html_to_md(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }
    match input_normalizer::normalize_markdown_from(html) {
        Ok(md) => {
            let md = md.replace("\n\n\n", "\n\n");
            if md.chars().count() > 8000 {
                let mut cut: String = md.chars().take(8000).collect();
                cut.push_str("\n\n…[truncated]");
                cut
            } else {
                md
            }
        }
        Err(_) => String::new(),
    }
}
