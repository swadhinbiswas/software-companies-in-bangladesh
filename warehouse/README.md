# Warehouse — DuckDB + Hugging Face

## Build
```bash
python -m venv .venv && source .venv/bin/activate
pip install -r warehouse/requirements.txt
python warehouse/build.py            # → data/warehouse.duckdb + data/parquet/*.parquet + data/gold/*.json/.parquet
python warehouse/build.py --push     # + push to HF (needs HF_TOKEN, HF_DATASET)
# or via Rust: cargo run -- .. --warehouse --push   or  cargo run -- warehouse --push
```

## Schema
- `dim_company` — from `data/companies.toml` (231 rows) + `tech[]` pruned via `data/schema.toml` implies
- `fact_job` — from `data/job-posts.json` (316 rows) with `job_id = sha256(company|title|source)`, `is_open` via Deadline parsing, `location_type` normalized
- `bridge_job_tag` — tags normalized for `v_tech_demand`
- `job_snapshot` — SCD2 lite (valid_from/valid_to)
- Views: `v_jobs_per_company`, `v_tech_demand`, `v_company_tech` (UNNEST), `v_location_heatmap`, `v_salary_stats`, `v_employment_breakdown`

## Gold (for dashboard — superfast)
- `stats`, `tech_demand`, `jobs_per_company`, `recent_jobs` (150, includes `description_md` 6k for Dialog), `companies`, etc.
- JSON is served via HF CDN `https://huggingface.co/datasets/<HF_DATASET>/resolve/main/gold/*.json` (ISR 60s on dashboard) + local `dashboard/public/gold` fallback
- Parquet (zstd) same queries for HF Parquet viewer + DuckDB-WASM range queries

## HF
Dataset: `HF_DATASET=username/software-companies-bd`
- `parquet/` — dim_company, fact_job, bridge, snapshot
- `gold/` — pre-aggregated json+parquet
- `warehouse.duckdb` — artifact (<200MB)
- `raw/job-posts.json` — lineage

Dashboard `lib/data.ts` does: HF CDN → `/gold/*.json` → `/api/gold/[name]` (edge)

## Debugging
```bash
duckdb data/warehouse.duckdb "SELECT * FROM v_tech_demand LIMIT 5"
parquet-tools meta data/parquet/fact_job.parquet
```
