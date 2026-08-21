//! Platform-specific structured extraction ("smart crawler").
//!
//! Instead of rendering every page in a browser and paying an LLM call,
//! detect the job-board platform from the URL and hit its public JSON API
//! directly. Jobs returned this way are exact: no confidence score, no
//! `needs_fetch` re-crawl. Schema.org `JobPosting` JSON-LD embedded in
//! generic pages is handled here as well.
#![allow(non_snake_case)] // vendor API JSON uses camelCase field names
use super::schema::*;
use crate::Result;
use crate::utils::http::Http;
use log::warn;
use serde::Deserialize;
use serde_json as json;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub enum Ats {
    Greenhouse(String),
    Lever(String),
    Workable(String),
    Ashby(String),
    Recruitee(String),
    SmartRecruiters(String),
}

impl Ats {
    /// Detect the platform from a careers URL. `None` means "generic page".
    pub fn detect(url: &Url) -> Option<Self> {
        let host = url.host_str()?.to_ascii_lowercase();
        let path = |i: usize| {
            url.path_segments()?
                .nth(i)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        let sub = || {
            host.split('.')
                .next()
                .filter(|s| !s.is_empty() && !s.contains("www"))
                .map(String::from)
        };

        Some(match host.as_str() {
            "boards.greenhouse.io" => Self::Greenhouse(path(0)?),
            "jobs.lever.co" => Self::Lever(path(0)?),
            "apply.workable.com" => Self::Workable(path(0)?),
            "jobs.ashbyhq.com" => Self::Ashby(path(0)?),
            "jobs.smartrecruiters.com" | "careers.smartrecruiters.com" => {
                Self::SmartRecruiters(path(0)?)
            }
            _ if host.ends_with(".greenhouse.io") => Self::Greenhouse(sub()?),
            _ if host.ends_with(".lever.co") => Self::Lever(sub()?),
            _ if host.ends_with(".recruitee.com") => Self::Recruitee(sub()?),
            _ => return None,
        })
    }

    /// Fetch jobs from the platform's public API. `Ok(None)` when the API
    /// is unreachable or returns no data (caller falls back to HTML).
    pub async fn fetch_jobs(&self, http: &Http) -> Result<Option<Vec<JobPost>>> {
        let jobs = match self {
            Self::Greenhouse(org) => {
                let url =
                    format!("https://boards-api.greenhouse.io/v1/boards/{org}/jobs?content=true");
                match http.get_json::<GreenhouseBoard>(&parse(&url)?).await {
                    Ok(board) => board.jobs.into_iter().map(Into::into).collect(),
                    Err(err) => return err_no_data(err),
                }
            }
            Self::Lever(org) => {
                let url = format!("https://api.lever.co/v0/postings/{org}?mode=json");
                match http.get_json::<Vec<LeverPosting>>(&parse(&url)?).await {
                    Ok(posts) => posts.into_iter().map(Into::into).collect(),
                    Err(err) => return err_no_data(err),
                }
            }
            Self::Workable(org) => {
                let url = format!("https://apply.workable.com/api/v1/widget/accounts/{org}");
                match http.get_json::<WorkableBoard>(&parse(&url)?).await {
                    Ok(board) => board.jobs.into_iter().map(Into::into).collect(),
                    Err(err) => return err_no_data(err),
                }
            }
            Self::Ashby(org) => {
                let url = format!(
                    "https://api.ashbyhq.com/posting-api/job-board/{org}?includeCompensation=true"
                );
                match http.get_json::<AshbyBoard>(&parse(&url)?).await {
                    Ok(board) => board.jobs.into_iter().map(Into::into).collect(),
                    Err(err) => return err_no_data(err),
                }
            }
            Self::Recruitee(org) => {
                let url = format!("https://{org}.recruitee.com/api/offers/");
                match http.get_json::<RecruiteeBoard>(&parse(&url)?).await {
                    Ok(board) => board.offers.into_iter().map(Into::into).collect(),
                    Err(err) => return err_no_data(err),
                }
            }
            Self::SmartRecruiters(org) => {
                let url = format!(
                    "https://api.smartrecruiters.com/v1/companies/{org}/postings?limit=100"
                );
                match http.get_json::<SmartRecruitersBoard>(&parse(&url)?).await {
                    Ok(board) => board.content.into_iter().map(Into::into).collect(),
                    Err(err) => return err_no_data(err),
                }
            }
        };
        Ok(Some(jobs))
    }
}

fn err_no_data<T>(err: crate::DynError) -> Result<Option<T>> {
    warn!("ATS API failed ({err}); falling back to HTML");
    Ok(None)
}

fn parse(url: &str) -> Result<Url> {
    Ok(Url::parse(url)?)
}

// ===========================================================================
// Greenhouse: https://boards-api.greenhouse.io/v1/boards/{org}/jobs
// ===========================================================================

#[derive(Deserialize)]
struct GreenhouseBoard {
    jobs: Vec<GreenhouseJob>,
}

#[derive(Deserialize)]
struct GreenhouseJob {
    title: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    location: Option<GreenhouseLocation>,
    #[serde(default)]
    departments: Vec<GreenhouseNamed>,
    #[serde(default)]
    offices: Vec<GreenhouseNamed>,
    #[serde(default)]
    absolute_url: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

#[derive(Deserialize)]
struct GreenhouseLocation {
    name: Option<String>,
}

#[derive(Deserialize)]
struct GreenhouseNamed {
    name: Option<String>,
}

impl From<GreenhouseJob> for JobPost {
    fn from(job: GreenhouseJob) -> Self {
        JobPost {
            title: job.title,
            description: html_to_markdown(job.content.as_deref().unwrap_or("")),
            employment_type: None,
            role: None,
            posted_at: job.updated_at.map(PostedAt::Absolute),
            category: department(&job.departments).or(department(&job.offices)),
            deadline: None,
            location: location(&job.location.and_then(|l| l.name), None),
            experience: None,
            salary: None,
            vacancies: None,
            tags: vec![],
            apply: job
                .absolute_url
                .as_deref()
                .map(apply_website)
                .unwrap_or_default(),
            source: job.absolute_url,
            needs_fetch: false,
            confidence: 1.0,
            last_seen: None,
        }
    }
}

fn department(list: &[GreenhouseNamed]) -> Option<Category> {
    let names: Vec<_> = list.iter().filter_map(|d| d.name.as_deref()).collect();
    names.first().map(|name| department_other(name))
}

// ===========================================================================
// Lever: https://api.lever.co/v0/postings/{org}?mode=json
// ===========================================================================

#[derive(Deserialize)]
struct LeverPosting {
    title: String,
    #[serde(default)]
    descriptionPlain: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    workplaceType: Option<String>,
    #[serde(default)]
    salaryRange: Option<LeverSalary>,
    #[serde(default)]
    applyUrl: Option<String>,
    #[serde(default)]
    hostedUrl: Option<String>,
    #[serde(default)]
    publishedAt: Option<i64>,
    #[serde(default)]
    department: Option<String>,
    #[serde(default)]
    employmentType: Option<String>,
}

#[derive(Deserialize)]
struct LeverSalary {
    min: Option<u64>,
    max: Option<u64>,
    currency: Option<String>,
}

impl From<LeverPosting> for JobPost {
    fn from(job: LeverPosting) -> Self {
        JobPost {
            title: job.title,
            description: job.descriptionPlain.or(job.description).unwrap_or_default(),
            employment_type: job.employmentType.as_deref().and_then(parse_employment),
            role: None,
            posted_at: job.publishedAt.map(|secs| {
                use chrono::{TimeZone, Utc};
                PostedAt::Absolute(
                    Utc.timestamp_opt(secs, 0)
                        .single()
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default(),
                )
            }),
            category: job.department.as_deref().map(department_other),
            deadline: None,
            location: location(&job.location, job.workplaceType.as_deref()),
            experience: None,
            salary: job.salaryRange.map(|s| Salary {
                min: s.min.map(|v| v as u32),
                max: s.max.map(|v| v as u32),
                currency: s.currency,
            }),
            vacancies: None,
            tags: vec![],
            apply: job
                .applyUrl
                .as_deref()
                .map(apply_website)
                .unwrap_or_default(),
            source: job.hostedUrl.or(job.applyUrl),
            needs_fetch: false,
            confidence: 1.0,
            last_seen: None,
        }
    }
}

// ===========================================================================
// Workable: https://apply.workable.com/api/v1/widget/accounts/{org}
// ===========================================================================

#[derive(Deserialize)]
struct WorkableBoard {
    #[serde(default)]
    jobs: Vec<WorkableJob>,
}

#[derive(Deserialize)]
struct WorkableJob {
    title: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    location: Option<WorkableLocation>,
    #[serde(default)]
    remote: bool,
    #[serde(default)]
    employment_type: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    department: Option<String>,
}

#[derive(Deserialize)]
struct WorkableLocation {
    country: Option<String>,
    city: Option<String>,
}

impl From<WorkableJob> for JobPost {
    fn from(job: WorkableJob) -> Self {
        JobPost {
            title: job.title,
            description: String::new(),
            employment_type: job.employment_type.as_deref().and_then(parse_employment),
            role: None,
            posted_at: job.published_at.map(PostedAt::Absolute),
            category: job.department.as_deref().map(department_other),
            deadline: None,
            location: location(
                &Some(format!(
                    "{} {}",
                    job.location
                        .as_ref()
                        .and_then(|l| l.city.clone())
                        .unwrap_or_default(),
                    job.location
                        .as_ref()
                        .and_then(|l| l.country.clone())
                        .unwrap_or_default()
                )),
                Some(if job.remote { "remote" } else { "on_site" }),
            ),
            experience: None,
            salary: None,
            vacancies: None,
            tags: vec![],
            apply: job.url.as_deref().map(apply_website).unwrap_or_default(),
            source: job.url,
            needs_fetch: false,
            confidence: 1.0,
            last_seen: None,
        }
    }
}

// ===========================================================================
// Ashby: https://api.ashbyhq.com/posting-api/job-board/{org}
// ===========================================================================

#[derive(Deserialize)]
struct AshbyBoard {
    #[serde(default)]
    jobs: Vec<AshbyJob>,
}

#[derive(Deserialize)]
struct AshbyJob {
    title: String,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    isRemote: bool,
    #[serde(default)]
    employmentType: Option<String>,
    #[serde(default)]
    publishedAt: Option<String>,
    #[serde(default)]
    jobUrl: Option<String>,
    #[serde(default)]
    department: Option<String>,
    #[serde(default)]
    descriptionHtml: Option<String>,
    #[serde(default)]
    compensation: Option<AshbyCompensation>,
}

#[derive(Deserialize)]
struct AshbyCompensation {
    #[serde(default)]
    compensationTierSummary: Option<AshbyTier>,
}

#[derive(Deserialize)]
struct AshbyTier {
    minimum: Option<f64>,
    maximum: Option<f64>,
    currency: Option<String>,
}

impl From<AshbyJob> for JobPost {
    fn from(job: AshbyJob) -> Self {
        let salary = job
            .compensation
            .and_then(|c| c.compensationTierSummary)
            .map(|t| Salary {
                min: t.minimum.map(|v| v as u32),
                max: t.maximum.map(|v| v as u32),
                currency: t.currency,
            });
        JobPost {
            title: job.title,
            description: html_to_markdown(job.descriptionHtml.as_deref().unwrap_or("")),
            employment_type: job.employmentType.as_deref().and_then(parse_employment),
            role: None,
            posted_at: job.publishedAt.map(PostedAt::Absolute),
            category: job.department.as_deref().map(department_other),
            deadline: None,
            location: location(
                &job.location,
                Some(if job.isRemote { "remote" } else { "on_site" }),
            ),
            experience: None,
            salary,
            vacancies: None,
            tags: vec![],
            apply: job.jobUrl.as_deref().map(apply_website).unwrap_or_default(),
            source: job.jobUrl,
            needs_fetch: false,
            confidence: 1.0,
            last_seen: None,
        }
    }
}

// ===========================================================================
// Recruitee: https://{org}.recruitee.com/api/offers/
// ===========================================================================

#[derive(Deserialize)]
struct RecruiteeBoard {
    #[serde(default)]
    offers: Vec<RecruiteeOffer>,
}

#[derive(Deserialize)]
struct RecruiteeOffer {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    remote: bool,
    #[serde(default)]
    employment_type: Option<String>,
    #[serde(default)]
    careers_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
}

impl From<RecruiteeOffer> for JobPost {
    fn from(offer: RecruiteeOffer) -> Self {
        JobPost {
            title: offer.title,
            description: offer.description.unwrap_or_default(),
            employment_type: offer.employment_type.as_deref().and_then(parse_employment),
            role: None,
            posted_at: offer.published_at.map(PostedAt::Absolute),
            category: None,
            deadline: None,
            location: location(
                &offer.location,
                Some(if offer.remote { "remote" } else { "on_site" }),
            ),
            experience: None,
            salary: None,
            vacancies: None,
            tags: vec![],
            apply: offer
                .careers_url
                .as_deref()
                .map(apply_website)
                .unwrap_or_default(),
            source: offer.careers_url,
            needs_fetch: false,
            confidence: 1.0,
            last_seen: None,
        }
    }
}

// ===========================================================================
// SmartRecruiters: https://api.smartrecruiters.com/v1/companies/{org}/postings
// ===========================================================================

#[derive(Deserialize)]
struct SmartRecruitersBoard {
    #[serde(default)]
    content: Vec<SmartRecruitersPosting>,
}

#[derive(Deserialize)]
struct SmartRecruitersPosting {
    name: String,
    #[serde(default)]
    releasedDate: Option<String>,
    #[serde(default)]
    location: Option<SmartLocation>,
    #[serde(default)]
    employmentType: Option<String>,
    #[serde(default)]
    applyUrl: Option<String>,
}

#[derive(Deserialize)]
struct SmartLocation {
    city: Option<String>,
    country: Option<String>,
    remote: Option<bool>,
}

impl From<SmartRecruitersPosting> for JobPost {
    fn from(post: SmartRecruitersPosting) -> Self {
        let remote = post
            .location
            .as_ref()
            .and_then(|l| l.remote)
            .unwrap_or(false);
        JobPost {
            title: post.name,
            description: String::new(),
            employment_type: post.employmentType.as_deref().and_then(parse_employment),
            role: None,
            posted_at: post.releasedDate.map(PostedAt::Absolute),
            category: None,
            deadline: None,
            location: location(
                &Some(format!(
                    "{} {}",
                    post.location
                        .as_ref()
                        .and_then(|l| l.city.clone())
                        .unwrap_or_default(),
                    post.location
                        .as_ref()
                        .and_then(|l| l.country.clone())
                        .unwrap_or_default()
                )),
                Some(if remote { "remote" } else { "on_site" }),
            ),
            experience: None,
            salary: None,
            vacancies: None,
            tags: vec![],
            apply: post
                .applyUrl
                .as_deref()
                .map(apply_website)
                .unwrap_or_default(),
            source: post.applyUrl,
            needs_fetch: false,
            confidence: 1.0,
            last_seen: None,
        }
    }
}

// ===========================================================================
// Shared mapping helpers
// ===========================================================================

fn apply_website(url: &str) -> Vec<ApplicationMethod> {
    vec![ApplicationMethod::Website(url.to_string())]
}

fn department_other(name: &str) -> Category {
    let lower = name.to_ascii_lowercase();
    if lower.contains("engin")
        || lower.contains("tech")
        || lower.contains("software")
        || lower.contains("data")
        || lower.contains("dev")
    {
        Category::Technology
    } else if lower.contains("sales") {
        Category::Sales
    } else if lower.contains("market") {
        Category::Marketing
    } else {
        Category::Other(name.to_string())
    }
}

fn location(raw: &Option<String>, kind: Option<&str>) -> Option<JobLocation> {
    let text = raw.as_deref().unwrap_or("").trim();

    let is_remote = kind.map(|k| k.contains("remote")).unwrap_or(false)
        || text.to_ascii_lowercase().contains("remote");

    if is_remote && text.trim().is_empty() {
        return Some(JobLocation::Remote);
    }

    if text.is_empty() {
        return None;
    }

    if is_remote {
        Some(JobLocation::Hybrid(text.to_string()))
    } else {
        Some(JobLocation::OnSite(text.to_string()))
    }
}

fn parse_employment(raw: &str) -> Option<EmploymentType> {
    let raw = raw.to_ascii_lowercase();
    if raw.contains("full") || raw == "f" {
        Some(EmploymentType::FullTime)
    } else if raw.contains("part") {
        Some(EmploymentType::PartTime)
    } else if raw.contains("contract") {
        Some(EmploymentType::Contract)
    } else if raw.contains("temp") {
        Some(EmploymentType::Temporary)
    } else if raw.contains("intern") {
        Some(EmploymentType::Internship)
    } else if raw.contains("freelance") {
        Some(EmploymentType::Freelance)
    } else {
        None
    }
}

/// Convert an HTML description to Markdown, keeping it small for the LLM.
fn html_to_markdown(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }
    match super::input_normalizer::normalize_markdown_from(html) {
        Ok(markdown) => truncate(&markdown, 4_000),
        Err(err) => {
            warn!("markdown conversion failed ({err}); using raw text");
            truncate(html, 4_000)
        }
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        text.chars().take(max_chars).collect()
    }
}

// ===========================================================================
// Schema.org `JobPosting` JSON-LD embedded in generic pages
// ===========================================================================

/// Extract job postings from all `application/ld+json` blocks in `html`.
/// Returns empty when none are found (caller falls back to LLM extraction).
pub fn from_json_ld(html: &str) -> Vec<JobPost> {
    use scraper::{Html, Selector};

    let Ok(selector) = Selector::parse(r#"script[type="application/ld+json"]"#) else {
        return vec![];
    };

    Html::parse_document(html)
        .select(&selector)
        .map(|script| script.text().collect::<String>())
        .filter_map(|raw| json::from_str::<json::Value>(&raw).ok())
        .filter_map(extract_jobposting)
        .collect()
}

fn extract_jobposting(value: json::Value) -> Option<JobPost> {
    let type_ = value
        .pointer("/@type")
        .or_else(|| value.pointer("/@graph/0/@type"));
    let is_job = match type_ {
        Some(json::Value::String(t)) => t.eq_ignore_ascii_case("JobPosting"),
        Some(json::Value::Array(types)) => types.iter().any(|t| {
            t.as_str()
                .is_some_and(|t| t.eq_ignore_ascii_case("JobPosting"))
        }),
        _ => false,
    };
    if !is_job {
        return None;
    }

    let title = value
        .pointer("/title")
        .and_then(json::Value::as_str)
        .unwrap_or_default()
        .to_string();

    if title.is_empty() {
        return None;
    }

    let description = value
        .pointer("/description")
        .and_then(json::Value::as_str)
        .map(html_to_markdown)
        .unwrap_or_default();

    let url = value
        .pointer("/url")
        .and_then(json::Value::as_str)
        .map(String::from);

    let location = value
        .pointer("/jobLocation/address")
        .map(|addr| {
            let city = addr
                .pointer("/addressLocality")
                .and_then(json::Value::as_str);
            let country = addr
                .pointer("/addressCountry")
                .and_then(json::Value::as_str);
            format!(
                "{} {}",
                city.unwrap_or_default().trim(),
                country
                    .as_ref()
                    .map(|c| if c.len() == 2 { "" } else { c })
                    .unwrap_or_default()
            )
            .trim()
            .to_string()
        })
        .filter(|s| !s.is_empty())
        .map(JobLocation::OnSite);

    let remote = value
        .pointer("/jobLocationType")
        .and_then(json::Value::as_str)
        .is_some_and(|t| t.to_ascii_lowercase().contains("remote"));

    let location = location
        .map(|loc| {
            if remote {
                JobLocation::Hybrid(match loc {
                    JobLocation::OnSite(s) | JobLocation::Hybrid(s) => s,
                    JobLocation::Remote => String::new(),
                })
            } else {
                loc
            }
        })
        .or_else(|| remote.then_some(JobLocation::Remote));

    let salary = value.pointer("/baseSalary").and_then(|s| {
        let (min, max, currency) = match s {
            json::Value::Object(map) => {
                let value = map.get("value").and_then(json::Value::as_number);
                let min = s
                    .pointer("/value/minValue")
                    .or(s.pointer("/value/minimum"))
                    .and_then(json::Value::as_number)
                    .or(value)
                    .map(|n| n.as_f64().unwrap_or_default() as u32);
                let max = s
                    .pointer("/value/maxValue")
                    .or(s.pointer("/value/maximum"))
                    .and_then(json::Value::as_number)
                    .or(value)
                    .map(|n| n.as_f64().unwrap_or_default() as u32);
                let currency = s
                    .pointer("/value/currency")
                    .or(s.pointer("/currency"))
                    .and_then(json::Value::as_str)
                    .map(String::from);
                (min, max, currency)
            }
            _ => (None, None, None),
        };
        if min.is_none() && max.is_none() {
            None
        } else {
            Some(Salary { min, max, currency })
        }
    });

    let deadline = value
        .pointer("/validThrough")
        .and_then(json::Value::as_str)
        .map(|d| Deadline::Date(PostedAt::Absolute(d.to_string())));

    let employment_type = value
        .pointer("/employmentType")
        .and_then(json::Value::as_str)
        .and_then(parse_employment);

    let posted_at = value
        .pointer("/datePosted")
        .and_then(json::Value::as_str)
        .map(|d| PostedAt::Absolute(d.to_string()));

    Some(JobPost {
        title,
        description,
        employment_type,
        role: None,
        posted_at,
        category: None,
        deadline,
        location,
        experience: None,
        salary,
        vacancies: None,
        tags: vec![],
        apply: url.as_deref().map(apply_website).unwrap_or_default(),
        source: url,
        needs_fetch: false,
        confidence: 1.0,
        last_seen: None,
    })
}
