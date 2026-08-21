# Open Data Platform — Bangladeshi Tech Jobs

Everything this project produces is **open, typed, and free to use**. This document is
for data engineers and developers who want to consume the dataset in their own
projects — dashboards, research, LLM/RAG pipelines, alerts, or BI tools.

## Where the data lives

| Artifact | Location | Format |
|---|---|---|
| Gold datasets (recommended) | `hf://datasets/swadhinbiswas/bangladeshi-jobs/gold/*.json` / `.parquet` | JSON + Parquet |
| Star-schema warehouse | `data/warehouse.duckdb` (built by CI weekly) | DuckDB |
| Normalized tables | `data/parquet/fact_job.parquet`, `dim_company.parquet`, `bridge_job_tag.parquet`, `job_snapshot.parquet` | Parquet |
| Raw crawl output | `data/job-posts.json` | JSON |
| Company registry | `data/companies.toml` | TOML |

Base URL for all HTTP access:

```
https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/
```

## Quickstart

### curl (raw files)

```bash
curl -L https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/stats.json
curl -L .../gold/recent_jobs.parquet -o jobs.parquet
```

### pandas

```python
import pandas as pd
df = pd.read_json("https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/recent_jobs.json")
print(df.groupby("company_name").size().sort_values(ascending=False).head())
```

### DuckDB (query Parquet in place — no download step)

```sql
SELECT company_name, title, salary_min, salary_max
FROM read_json_auto(
  'https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/recent_jobs.json')
WHERE list_contains(tags, 'React')
ORDER BY salary_max DESC NULLS LAST;
```

## Dataset dictionary

| File | Grain | Key columns | Typical use |
|---|---|---|---|
| `gold/recent_jobs` | 1 row = 1 posting | `job_id`, `company_name`, `title`, `description_md`, `location_*`, `salary_*`, `tags[]`, `apply_links[]`, `source_url`, `last_seen_at` | Job boards, alerts, RAG corpora |
| `gold/companies` | 1 row = 1 company | `name`, `host`, `tech[]`, `career_url`, `open_jobs` | Market maps, enrichment |
| `gold/tech_demand` | 1 row = tag | `tag`, `jobs`, `companies` | Skill-demand analytics |
| `gold/salary_stats` | 1 row = currency | `median_min/max`, `min_min`, `max_max`, `n` | Compensation benchmarking |
| `gold/jobs_per_company` | 1 row = company | `name`, `open_jobs`, `tech[]` | Hiring-velocity signals |
| `gold/location_heatmap` | 1 row = location | `location_text`, `jobs` | Geo analysis |
| `fact_job` + `dim_company` + `bridge_job_tag` | star schema | `job_id` PK; SCD snapshots in `job_snapshot` | BI tools (Metabase, Superset, PowerBI) |

## Pipeline & lineage

```
career pages (companies.toml → verified job URLs)
  → crawler: ATS JSON APIs → schema.org JSON-LD → markdown + pagination
  → LLM extraction (batched) → LLM refinement (batches of 20, id-aligned JSON)
  → deterministic enhancer (dedup, confidence gate ≥ 0.5, salary sanity)
  → DuckDB warehouse (idempotent upserts on job_id)
  → gold views → Hugging Face bucket (public)
```

Every job keeps its `source_url` — full provenance back to the original posting.

## Quality guarantees

- **Dedup**: within a company, same `(title, location)` keeps only the highest-confidence row.
- **Confidence gate**: rows below `0.5` are dropped before publication.
- **Salary sanity**: `min ≤ max`; currency normalized to ISO codes; no invented numbers.
- **Freshness**: `last_seen_at` per job; expired deadlines are filtered from "open" views.
- **Incremental**: crawls are cached (24h); re-runs fill gaps instead of re-scraping everything.

## Notes for specific consumers

- **RAG / embeddings**: descriptions are cleaned Markdown (≤ ~8k chars). Chunk on headings;
  use `title + company_name + location_text` as metadata.
- **BI tools**: point Metabase/Superset at a DuckDB file built via
  `cargo run -- warehouse` (or mount `data/parquet/` and query with any engine).
- **Alerting**: diff consecutive `job_snapshot` runs — new `job_id`s are new postings.
- **Attribution**: CC-BY-4.0 — cite *software-companies-in-bangladesh* and link the repo.

## Contributing data

Add or fix a company in `data/companies.toml`:

```toml
["Example Ltd."]
tech = ["React", "NodeJS"]      # must exist in data/schema.toml
type = ["Software Firm"]        # must exist in data/schema.toml
website = "https://example.com/"
job = "https://example.com/careers"   # verified career page
```

CI validates every entry (schema conformance, non-empty arrays, implication rules)
on every push. The crawler's discovery pass verifies each `job` URL actually
contains listings before it is trusted.
