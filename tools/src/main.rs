#[cfg(feature = "extra")]
mod info;
#[cfg(feature = "crawler")]
mod jobs;
mod update;

mod data;
mod error;
mod repos;
mod utils;

use clap::Parser;
use std::{fs, path::PathBuf, process};

use data::{Companies, Schema};
use repos::subtree;
use utils::{logger::Logger, text_file::TextFile};

pub type DynError = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T = (), E = DynError> = std::result::Result<T, E>;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Tools for maintaining the Awesome Software Companies in Bangladesh repo."
)]
struct Cli {
    #[arg(default_value = ".")]
    dir: PathBuf,

    #[arg(long)]
    backup: bool,

    /// Pull updates from upstream repos
    #[arg(long)]
    pull: bool,

    #[arg(long)]
    update: bool,

    /// Format data
    #[arg(long)]
    fmt: bool,

    #[arg(long, short)]
    docs: bool,

    // /// Crawl jobs with the specified LLM model

    // crawl: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    #[cfg(feature = "extra")]
    /// Fetch websites and extract `schema.org` data.
    Fetch {
        /// Re-fetch even if cached.
        #[arg(long)]
        force: bool,
    },
    #[cfg(feature = "crawler")]
    /// Extract job posting information.
    Index {
        /// Re-fetch even if cached.
        #[arg(long, short)]
        force: bool,

        /// LLM provider: `gemini` or `zen`.
        #[arg(
            long,
            default_value = "gemini",
            value_name = "PROVIDER"
        )]
        provider: String,

        /// LLM model to use.
        #[arg(
            long,
            short,
            default_value = jobs::llm::DEFAULT_MODEL,
            value_name = "MODEL"
        )]
        model: String,
        /// Generate `job-postings.pages.md` file
        #[arg(long, short)]
        log_file: bool,

        /// Maximum number of concurrent jobs.
        #[arg(long, short, default_value_t = 8, value_name = "N")]
        concurrent: u8,
    },
}

fn main() {
    Logger::init();

    if let Err(error) = cli() {
        log::error!("{error}");
    }

    let warnings = Logger::count_warnings();
    if warnings > 0 {
        eprintln!("::warning:: Found {warnings} warnings");
    }

    if Logger::has_error() {
        process::exit(1);
    }
}

fn cli() -> Result {
    let Cli {
        dir,
        backup,
        pull,
        mut update,
        mut fmt,
        mut docs,
        #[allow(unused)]
        command,
    } = Cli::parse();

    let schema_file = TextFile::read(dir.join("./data/schema.toml"))?;
    let schema = Schema::parse(&schema_file.text)?;

    let companies_file = TextFile::read(dir.join("./data/companies.toml"))?;
    let mut companies = Companies::parse(&companies_file.text)?;

    companies.check_known_company_type(&schema);
    companies.check_no_redundant_technologies(&schema);

    if Logger::has_error() {
        return Ok(());
    }

    if backup {
        fs::create_dir_all(dir.join("./backup"))?;
        fs::write(dir.join("./backup/companies.toml"), companies.to_toml()?)?;
    }

    if pull {
        subtree::pull_repos(&dir)?;
        update = true;
    }

    if update {
        update::repos(&schema, &mut companies, &dir)?;
        fmt = true;
        docs = true;
    }

    #[cfg(feature = "extra")]
    if let Some(Command::Fetch { force }) = command {
        if force {
            log::info!("Clearing cache...");
            utils::fetch::clear_cache()?;
        }
        info::fetch_info(&companies, &dir)?;
    }

    #[cfg(feature = "crawler")]
    if let Some(Command::Index {
        model,
        log_file,
        force,
        concurrent,
        provider,
    }) = command
    {
        if force {
            log::info!("Clearing index cache...");
            jobs::clear_cache()?;
        }
        let provider = jobs::llm::Provider::parse(&provider)?;
        log::info!("Concurrent: {concurrent}; LLM: {model}; Provider: {provider:?}");
        jobs::run(provider, model, &dir, &companies, log_file, concurrent)?;
    }

    if fmt {
        companies_file.write(companies.to_toml()?)?;
    }

    if docs {
        let count_jobs_link = companies
            .iter()
            .filter(|(_, c)| c.links.job.is_some())
            .count();

        TextFile::read(dir.join("./README.md"))?.write(format!(
            "{README_HEADER}, From `{count_jobs_link}` companies.\n\n{companies}",
        ))?;
        #[cfg(feature = "crawler")]
        jobs::schema::gen_readme(dir)?;
    }

    Ok(())
}

static README_HEADER: &str = "<!-- AUTO-GENERATED FILE — DO NOT EDIT. -->
<!-- To update entries, go to the `/data` directory. -->

## 💼 Jobs

Visit [Jobs](https://github.com/nurmohammed840/software-companies-in-bangladesh/blob/main/jobs.md) to see currently open positions";
