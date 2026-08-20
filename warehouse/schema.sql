-- warehouse/schema.sql — Medallion: bronze → silver → gold
-- Run in DuckDB. All tables dropped/recreated for full rebuild; incremental uses MERGE.

-- ── DROP in dependency order (child → parent) ──────────────────────────────
DROP VIEW IF EXISTS v_jobs_per_company;
DROP VIEW IF EXISTS v_tech_demand;
DROP VIEW IF EXISTS v_company_tech;
DROP VIEW IF EXISTS v_location_heatmap;
DROP VIEW IF EXISTS v_salary_stats;
DROP VIEW IF EXISTS v_posting_trend;
DROP VIEW IF EXISTS v_employment_breakdown;
DROP TABLE IF EXISTS bridge_job_tag;
DROP TABLE IF EXISTS fact_job;
DROP TABLE IF EXISTS job_snapshot;
DROP TABLE IF EXISTS dim_company;

-- ── DIM_COMPANY (from data/companies.toml + data/schema.toml) ──────────────
CREATE TABLE dim_company (
  company_id      INTEGER PRIMARY KEY,  -- fnv hash of host
  name            TEXT NOT NULL UNIQUE,
  website         TEXT NOT NULL,
  job_url         TEXT,
  host            TEXT,                 -- url_host(website)
  linkedin        TEXT,
  github          TEXT,
  twitter         TEXT,
  facebook        TEXT,
  youtube         TEXT,
  tech            TEXT[],               -- raw tags from TOML
  company_type    TEXT[],
  has_job_page    BOOLEAN,
  tech_count      INTEGER,
  updated_at      TIMESTAMP DEFAULT now()
);

-- ── FACT_JOB (from data/job-posts.json) ────────────────────────────────────
CREATE TABLE fact_job (
  job_id          TEXT PRIMARY KEY,     -- content_hash: sha256(company|title|source)
  company_id      INTEGER REFERENCES dim_company(company_id),
  company_name    TEXT NOT NULL,
  source_company_url TEXT,              -- Entry.source (company job page)
  title           TEXT NOT NULL,
  description_md  TEXT,
  description_len INTEGER,
  employment_type TEXT,                 -- FullTime | Internship | Contract ...
  role            TEXT,
  category        TEXT,                 -- Technology | Sales | Other
  posted_at_raw   TEXT,                 -- PostedAt::Absolute|Relative string
  deadline_raw    TEXT,                 -- JSON string of Deadline
  deadline_date   DATE,                 -- parsed deadline_date for filtering
  is_expired      BOOLEAN,
  is_open         BOOLEAN,
  location_raw    JSON,
  location_text   TEXT,                 -- normalized OnSite/Hybrid/Remote text
  location_type   TEXT,                 -- Remote | Hybrid | OnSite | Unknown
  experience      TEXT,
  salary_min      INTEGER,
  salary_max      INTEGER,
  salary_currency TEXT,
  vacancies       INTEGER,
  tags            TEXT[],
  apply_json      JSON,                  -- ApplicationMethod[]
  apply_links     TEXT[],               -- flattened Website URLs
  source_url      TEXT,                 -- JobPost.source resolved absolute
  confidence      DOUBLE,
  needs_fetch     BOOLEAN,
  first_seen_at   TIMESTAMP,
  last_seen_at    TIMESTAMP,
  crawl_id        TEXT
);

-- ── BRIDGE_JOB_TAG (normalized for analytics) ──────────────────────────────
CREATE TABLE bridge_job_tag (
  job_id  TEXT REFERENCES fact_job(job_id),
  tag     TEXT NOT NULL,
  PRIMARY KEY (job_id, tag)
);

-- ── SNAPSHOT (SCD2-lite) ───────────────────────────────────────────────────
CREATE TABLE job_snapshot (
  job_id      TEXT PRIMARY KEY,
  company_id  INTEGER,
  title       TEXT,
  first_seen_at TIMESTAMP,
  last_seen_at  TIMESTAMP,
  is_current    BOOLEAN,
  valid_from    TIMESTAMP,
  valid_to      TIMESTAMP
);

-- ── GOLD VIEWS ─────────────────────────────────────────────────────────────
CREATE VIEW v_jobs_per_company AS
  SELECT c.name, c.host, c.tech, j.company_id, COUNT(*) AS open_jobs, COUNT(*) FILTER (WHERE j.is_open) AS open_current
  FROM fact_job j JOIN dim_company c USING(company_id)
  WHERE j.is_open = true
  GROUP BY 1,2,3,4 ORDER BY open_jobs DESC;

CREATE VIEW v_tech_demand AS
  SELECT tag, COUNT(*) AS jobs, COUNT(DISTINCT company_id) AS companies
  FROM bridge_job_tag b JOIN fact_job j USING(job_id) WHERE j.is_open
  GROUP BY 1 ORDER BY 2 DESC;

DROP VIEW IF EXISTS v_company_tech;
CREATE VIEW v_company_tech AS
  SELECT t.tech AS tech, COUNT(*) AS companies
  FROM dim_company, UNNEST(dim_company.tech) AS t(tech)
  GROUP BY 1 ORDER BY 2 DESC;

CREATE VIEW v_location_heatmap AS
  SELECT location_type, location_text, COUNT(*) AS jobs
  FROM fact_job WHERE is_open GROUP BY 1,2 ORDER BY 3 DESC;

CREATE VIEW v_salary_stats AS
  SELECT salary_currency, COUNT(*) AS n, MEDIAN(salary_min) AS median_min, MEDIAN(salary_max) AS median_max, MIN(salary_min) AS min_min, MAX(salary_max) AS max_max
  FROM fact_job WHERE salary_min IS NOT NULL AND is_open GROUP BY 1;

CREATE VIEW v_posting_trend AS
  SELECT date_trunc('day', last_seen_at)::DATE AS day, COUNT(*) AS jobs_seen
  FROM fact_job GROUP BY 1 ORDER BY 1;

CREATE VIEW v_employment_breakdown AS
  SELECT employment_type, COUNT(*) AS jobs FROM fact_job WHERE is_open GROUP BY 1 ORDER BY 2 DESC;
