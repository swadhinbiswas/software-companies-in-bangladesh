# BD Jobs Dashboard — shadcn + Next.js 14 (Vercel / Cloudflare Workers)

Superfast job board for 230+ Bangladeshi software companies. Data = DuckDB warehouse on Hugging Face parquet (CDN), UI = shadcn/ui + Next.js ISR 60s on edge.

## Architecture
```
Rust crawler (tools/)  ATS API → JSON-LD → HTML+LLM batch, 24h zstd cache, 8 concur, 270s deadline
  → data/job-posts.json (316 postings)
  → python warehouse/build.py → data/warehouse.duckdb + data/parquet/*.parquet (zstd) + data/gold/*.json (pre-aggregated)
  → python warehouse/hf_push.py → HF Datasets  https://huggingface.co/datasets/$HF_DATASET/{parquet,gold}
  → Next.js dashboard (shadcn) — Vercel edge OR Cloudflare Workers via @cloudflare/next-on-pages
       fetch gold/*.json via HF CDN (<50ms, s-maxage=60) — no DB on request
       job description in Dialog (shadcn) with Markdown + tags, salary, apply links
       optional DuckDB-WASM range-queries on parquet for power users (install duckdb-wasm)
```

## Quick start (local, no HF)

```bash
# 1. Build warehouse from existing data/job-posts.json (no crawl, no LLM needed)
python -m venv .venv && source .venv/bin/activate
pip install -r warehouse/requirements.txt
python warehouse/build.py
# → data/warehouse.duckdb, data/parquet/, data/gold/ (stats, tech_demand, recent_jobs with description_md, ...)

# 2. Dashboard dev (uses local gold via public/gold fallback — superfast, no HF)
mkdir -p dashboard/public/gold && cp -r data/gold/* dashboard/public/gold/
cd dashboard
yarn install
yarn dev  # http://localhost:3000  (ISR 60s, shadcn, JobDetail Dialog)
```

## Full crawl (optional)

```bash
export GEMINI_API_KEY=... # or ZEN_API_KEY
cargo run --manifest-path tools/Cargo.toml -- .. index --concurrent 8 --warehouse
# also builds warehouse after crawl
python warehouse/build.py
# or: ./crawl.sh  (builds release binary, crawls, regenerates jobs.md + warehouse)
```

## Deploy

### Vercel
```bash
cd dashboard
# set env in Vercel dashboard:
#   NEXT_PUBLIC_HF_DATASET=your-username/software-companies-bd
#   HF_DATASET=...  HF_TOKEN=hf_... (for warehouse push)
vercel --prod
# or via GitHub: Vercel auto-deploys on push (ISR 60s)
```

### Cloudflare Workers / Pages
```bash
cd dashboard
npx @cloudflare/next-on-pages   # builds .vercel/output/static
npx wrangler pages deploy .vercel/output/static --project-name=bd-jobs-dashboard
# set vars in Cloudflare dashboard: NEXT_PUBLIC_HF_DATASET
```

### Hugging Face push
```bash
export HF_TOKEN=hf_xxx
export HF_DATASET=your-username/software-companies-bd
python warehouse/hf_push.py        # parquet/* + gold/* + warehouse.duckdb + raw/job-posts.json
# or: python warehouse/build.py --push
# HF gives you: CDN  https://huggingface.co/datasets/<id>/resolve/main/gold/stats.json
#                Parquet viewer + SQL API (duckdb query directly on HF)
```

## UI — shadcn

- `components/ui/*` — shadcn (Button, Card, Badge, Dialog, Table, Tabs, Select, Separator)
- `components/jobs-table.tsx` — filter by search/company/type/location, paginated 20, View → `JobDetailDialog`
- `components/job-detail-dialog.tsx` — shadcn Dialog, shows full `description_md` (Markdown), tags, salary (`formatSalary`), location, apply `mailto:` + `Website`, source link
- `components/kpi-cards.tsx` + `components/charts.tsx` (Recharts bar/pie) — tech demand, employment pie
- `app/page.tsx` — Tabs: Overview / Jobs / Companies, ISR 60s, edge-friendly `fetchGold()` that tries HF CDN first then `/gold/*.json`
- `app/api/gold/[name]/route.ts` — edge API that proxies HF or local `public/gold` (for Cloudflare)

## Warehouse gold (superfast reads)

`warehouse/build.py` generates:
- `stats.json` — total_companies, open_jobs, hiring_companies
- `tech_demand.json` — tag jobs/companies (from bridge_job_tag)
- `jobs_per_company.json` — v_jobs_per_company
- `recent_jobs.json` — 150 open jobs with `description_md` (capped 6k) for dialog — no extra fetch
- `companies.json`, `location_heatmap.json`, etc. + matching `.parquet` (zstd) for HF

Dashboard fetches `gold/*.json` via CDN — no DuckDB on request, <50ms. For ad-hoc SQL, install `duckdb-wasm` and use `queryParquetViaWasm()` on HF parquet via range-requests.

## Cost & speed

- Crawl: ATS API + JSON-LD avoid LLM for ~40% pages; batch LLM 12 sites/call, 7s pacing stays under free-tier 10 req/min; 24h cache makes re-run seconds.
- Warehouse: DuckDB local, Parquet zstd (~1/10 JSON), HF LFS + CDN free.
- Dashboard: Next.js static + ISR 60s on edge, no server DB, shadcn + Tailwind, 47kB JS.

## Env

```
# dashboard/.env.example
NEXT_PUBLIC_HF_DATASET=your-username/software-companies-bd
HF_DATASET=...
HF_TOKEN=hf_...
GEMINI_API_KEY=...
```
