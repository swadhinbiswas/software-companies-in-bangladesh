use super::*;
use crate::utils::date::parse_date;
use chrono::Utc;
use std::{
    fmt::{self, Write},
    path::PathBuf,
};

pub type Jobs = Map<String, Entry>;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub source: Url,
    pub jobs: Vec<JobPost>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JobPost {
    pub title: String,

    /// Job description formatted as Markdown.
    pub description: String,

    pub employment_type: Option<EmploymentType>,
    /// Job role or seniority.
    pub role: Option<String>,

    pub posted_at: Option<PostedAt>,

    pub category: Option<Category>,

    /// Application deadline. Use `Expired` only if the posting explicitly states
    /// that applications are closed or expired.
    pub deadline: Option<Deadline>,

    pub location: Option<JobLocation>,

    /// Required or preferred experience.
    pub experience: Option<String>,

    pub salary: Option<Salary>,

    /// Number of open positions.
    pub vacancies: Option<u32>,

    /// Relevant technologies, skills, tools.
    ///
    /// LLMs may extract relevant tags from the `description` when available.
    /// Do not invent or add duplicate tags.
    pub tags: Vec<String>,

    /// Ways to apply.
    pub apply: Vec<ApplicationMethod>,

    /// Original job posting link.
    /// Include only when found.
    /// Preserve exactly as provided, whether relative or absolute. Never guess or alter it.
    /// Never resolve to an absolute URL.
    pub source: Option<String>,

    /// `true` if `source` should be fetched for complete job details.
    pub needs_fetch: bool,

    #[schemars(range(min = 0.0, max = 1.0))]
    pub confidence: f32,
}

/// Broad job category or professional domain.
/// Use `Technology` for software, IT, cybersecurity, DevOps, ... roles.
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
pub enum Category {
    Technology,
    Sales,
    Marketing,
    Other(String)
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Salary {
    /// Minimum salary.
    pub min: Option<u32>,

    /// Maximum salary.
    pub max: Option<u32>,

    /// ISO 4217 currency code (e.g. "USD", "BDT", "EUR").
    pub currency: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub enum Deadline {
    /// Application deadline.
    Date(PostedAt),

    /// Applications are closed.
    Expired,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub enum PostedAt {
    Absolute(String),
    /// Relative time, e.g. "2 days ago".
    Relative(String),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
pub enum ApplicationMethod {
    Email(String),
    /// Preserve exactly as provided, whether relative or absolute. Never guess or alter it.
    /// Never resolve to an absolute URL.
    Website(String),
}

impl ApplicationMethod {
    #[allow(dead_code)]
    pub fn website(&self) -> Option<&str> {
        match self {
            ApplicationMethod::Email(_) => None,
            ApplicationMethod::Website(url) => Some(url),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub enum EmploymentType {
    FullTime,
    PartTime,
    Contract,
    Temporary,
    Internship,
    Freelance,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub enum JobLocation {
    ///  All remote jobs are allowed, including those outside Bangladesh.
    Remote,
    /// Hybrid job located in Bangladesh.
    Hybrid(String),
    /// On-site job located in Bangladesh.
    OnSite(String),
}

// =============================================================================

impl fmt::Debug for JobLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Remote => write!(f, "Remote"),
            Self::Hybrid(s) => f.write_str(&format!("{s} (Hybrid)")),
            Self::OnSite(s) => f.write_str(s),
        }
    }
}

impl fmt::Debug for EmploymentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FullTime => write!(f, "Full Time"),
            Self::PartTime => write!(f, "Part Time"),
            Self::Contract => write!(f, "Contract"),
            Self::Temporary => write!(f, "Temporary"),
            Self::Internship => write!(f, "Internship"),
            Self::Freelance => write!(f, "Freelance"),
        }
    }
}

impl fmt::Debug for PostedAt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PostedAt::Absolute(s) => f.write_str(s),
            PostedAt::Relative(s) => f.write_str(s),
        }
    }
}

impl fmt::Debug for Deadline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Date(s) => s.fmt(f),
            Self::Expired => write!(f, "Expired"),
        }
    }
}

impl Deadline {
    pub fn is_expired(&self) -> bool {
        match self {
            Self::Expired => true,
            Self::Date(PostedAt::Relative(_)) => false,
            Self::Date(PostedAt::Absolute(date)) => match parse_date(date) {
                Some(date) => date < Utc::now().date_naive(),
                None => {
                    error!("Could not parse deadline: {date}");
                    false
                }
            },
        }
    }
}

impl JobPost {
    fn is_open(&self) -> bool {
        self.deadline
            .as_ref()
            .is_none_or(|deadline| !deadline.is_expired())
    }
}

fn fmt_salary(amount: u32) -> u32 {
    amount / 1000
}

fn print_post(o: &mut String, source: &str, job: &JobPost) -> Result {
    writeln!(o, "| Field | Information |")?;
    writeln!(o, "| ----- | ----------- |")?;

    if let Some(ty) = &job.employment_type {
        writeln!(o, "| **Employment** | {ty:?} |")?;
    }
    if let Some(salary) = &job.salary {
        match (salary.min, salary.max) {
            (Some(num), None) | (None, Some(num)) => {
                writeln!(o, "| **Salary** | {}K |", fmt_salary(num))?
            }
            (Some(min), Some(max)) if min == max => {
                writeln!(o, "| **Salary** | {}K |", fmt_salary(max))?
            }
            (Some(min), Some(max)) => writeln!(
                o,
                "| **Salary** | {}K - {}K |",
                fmt_salary(min),
                fmt_salary(max)
            )?,
            _ => {}
        }
    }
    if let Some(at) = &job.posted_at {
        writeln!(o, "| **Posted** | {at:?} |")?;
    }
    if let Some(deadline) = &job.deadline {
        writeln!(o, "| **Deadline** | {deadline:?} |")?;
    }
    if let Some(loc) = &job.location {
        writeln!(o, "| **Location** | {loc:?} |")?;
    }
    if let Some(role) = &job.role {
        writeln!(o, "| **Role** | {role} |")?;
    }
    if let Some(count) = &job.vacancies {
        writeln!(o, "| **Vacancies** | {count} |")?;
    }

    if !job.tags.is_empty() {
        let tags: String = job.tags.iter().map(|tag| format!("`{tag}` ")).collect();
        writeln!(o, "\n**🛠️ Tags**: {tags}\n")?;
    }

    writeln!(o, "## 📝 [Description]({source})\n\n{}\n", job.description)?;

    if !job.apply.is_empty() {
        writeln!(o, "---")?;

        for method in &job.apply {
            match method {
                ApplicationMethod::Email(email) => {
                    writeln!(o, "* 📧 [Send Resume via Email](mailto:{email})")?
                }
                ApplicationMethod::Website(web) => writeln!(o, "* 🌐 [Apply on Website]({web})")?,
            }
        }
    }

    writeln!(o, "---\n")?;

    Ok(())
}

pub fn gen_readme(dir: PathBuf) -> Result {
    let file = TextFile::read(dir.join("data/job-posts.json"))?;
    let jobs: Jobs = json::from_str(&file.text)?;

    let mut o = String::new();

    let open = jobs
        .values()
        .flat_map(|f| &f.jobs)
        .filter(|job| job.is_open())
        .count();

    let total = jobs.values().map(|f| f.jobs.len()).sum::<usize>();

    writeln!(
        o,
        "# Jobs\n\n**🟢 {open} open** · **📋 {total} total** · **🏢 {} companies**\n",
        jobs.len()
    )?;

    for (name, Entry { source, mut jobs }) in jobs {
        jobs.sort_by_key(|job| job.title.clone());

        let jobs: Vec<_> = jobs.into_iter().filter(|job| job.is_open()).collect();

        if jobs.is_empty() {
            continue;
        }

        writeln!(o, "## 🏢 {name}\n")?;
        writeln!(o, "> Career Page: <{source}>\n")?;

        for mut job in jobs {
            let src = match &job.source {
                Some(url) => resolve_url(&source, url)?.into(),
                None => source.to_string(),
            };

            for method in job.apply.iter_mut() {
                let ApplicationMethod::Website(link) = method else {
                    continue;
                };
                *link = resolve_url(&source, link)?.into()
            }

            writeln!(o, "<details>")?;
            writeln!(
                o,
                "<summary> <strong style=\"font-size: 1.3em;\">💼 {}</strong> </summary>\n",
                job.title
            )?;

            print_post(&mut o, &src, &job)?;
            writeln!(o, "</details>\n")?;
        }
        // writeln!(o, "---\n")?;
    }

    TextFile::read(dir.join("jobs.md"))?.write(o)?;

    Ok(())
}
