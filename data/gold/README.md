---
license: cc-by-4.0
task_categories:
- text-retrieval
- feature-extraction
language:
- en
- bn
tags:
- jobs
- bangladesh
- job-market
- tech-jobs
- data-engineering
- duckdb
- parquet
size_categories:
- 1K<n<10K
pretty_name: Bangladeshi Tech Jobs
---

# Bangladeshi Tech Jobs — Open Dataset

Weekly-refreshed, structured dataset of **open software & IT job postings from Bangladeshi tech companies**, built by an automated crawl → LLM-extraction → data-warehouse pipeline. Published as JSON + Parquet + a DuckDB star schema, free for any use with attribution (CC-BY-4.0).

<!-- SNAPSHOT:START -->

## Snapshot (2026-08-21)

> 🤖 **Auto-generated on every build** — these numbers are never edited by hand.

| Metric | Value |
|---|---|
| Registered companies | 234 |
| Companies with verified career page | 140 |
| Hiring right now | 68 |
| Open postings | 478 |
| Total postings tracked | 483 |
| Distinct skills demanded | 643 |
| Remote-friendly postings | 8 |
| Median max salary (explicit figures) | ৳60,000 / month |
| Most active hirer | Next Ventures |
| Most in-demand skill | Python |

<!-- SNAPSHOT:END -->

## Files

```
gold/                      ← recommended entry points (JSON + Parquet, stable schema)
  recent_jobs.*            1 row = 1 posting (open, newest first, max 150)
  companies.*              1 row = 1 company (tech stack, career URL, open jobs)
  tech_demand.*            1 row = tag → jobs, companies (skill demand)
  jobs_per_company.*       1 row = company → hiring velocity
  location_heatmap.*       1 row = location → jobs
  salary_stats.*           1 row = currency → median/min/max salary, n
  employment_breakdown.*   FullTime / Contract / Internship …
  company_tech.*           1 row = technology → company count
  stats.json               headline KPIs (single row)
parquet/                   ← normalized tables
  fact_job.parquet         1 row = 1 posting (full detail)
  dim_company.parquet      company registry
  bridge_job_tag.parquet   posting ↔ tag junction
  job_snapshot.parquet     SCD-style history (first_seen / last_seen)
warehouse.duckdb           ← full DuckDB star schema, queryable in place
raw/job-posts.json         ← crawler output (source of truth)
```

## `fact_job` schema (main table)

| Column | Type | Notes |
|---|---|---|
| `job_id` | TEXT PK | sha256(company\|title\|source) — stable across runs |
| `company_name` | TEXT | matched against the curated company registry |
| `title`, `description_md` | TEXT | cleaned Markdown description |
| `employment_type` | TEXT | FullTime / PartTime / Contract / Internship / Freelance / Temporary |
| `location_text`, `location_type` | TEXT | Remote / Hybrid / OnSite |
| `salary_min/max/currency` | INT/TEXT | explicit figures only, min ≤ max enforced |
| `deadline_date`, `is_open`, `is_expired` | DATE/BOOL | `is_open = deadline ≥ today or unknown` |
| `tags` | TEXT[] | canonical tech names (React, NodeJS, PostgreSQL…) |
| `apply_links[]`, `source_url` | TEXT[] | provenance back to the original posting |
| `first_seen_at`, `last_seen_at` | TIMESTAMP | freshness tracking |

## Pipeline

```
career pages (curated registry of BD software companies)
  → crawler: ATS JSON APIs → schema.org JSON-LD → markdown + pagination
  → LLM extraction (batched, schema-constrained)
  → LLM refinement pass (id-aligned cleaning batches)
  → deterministic enhancer (dedup, confidence ≥ 0.5 gate, salary sanity)
  → DuckDB warehouse (idempotent upserts on job_id)
  → gold views → this bucket   (refreshed weekly)
```

Every posting keeps its `source_url`. Jobs are sourced from company career pages and the BDJobs board (**registered employers only** — unrelated board posters are excluded).

## Quality guarantees

- **Dedup**: within a company, same `(title, location)` keeps the highest-confidence row.
- **Confidence gate**: rows below 0.5 are dropped before publication.
- **Salary sanity**: explicit numbers only; `min ≤ max`; currency normalized.
- **Freshness**: expired deadlines filtered from all "open" views; `last_seen_at` per row.
- **Idempotent rebuilds**: same input → same `job_id`s; weekly runs never duplicate.

## Quickstart

```bash
# headline numbers
curl -L https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/stats.json
```

```python
import pandas as pd
df = pd.read_json("https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/recent_jobs.json")
print(df.groupby("company_name").size().sort_values(ascending=False).head())
```

```sql
-- DuckDB: query Parquet in place, no download
SELECT company_name, title, salary_min, salary_max
FROM read_json_auto(
  'https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/recent_jobs.json')
WHERE list_contains(tags, 'React')
ORDER BY salary_max DESC NULLS LAST;
```

## Limitations

- Descriptions and structured fields are LLM-extracted; rare extraction noise is possible (mitigated by the refinement pass + confidence gate).
- Salaries are present only where explicitly stated (~minority of postings).
- Coverage tracks the curated registry; companies not yet registered won't appear even if hiring on job boards.

## Citation

```bibtex
@dataset{bangladeshi_tech_jobs_2026,
  title  = {Bangladeshi Tech Jobs — Open Dataset},
  author = {swadhinbiswas},
  year   = {2026},
  url    = {https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs},
  note   = {Weekly-refreshed job postings from Bangladeshi software companies}
}
```

Licensed under **CC-BY-4.0** — cite *software-companies-in-bangladesh* and link the repo.
