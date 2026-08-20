#!/usr/bin/env python3
"""
warehouse/build.py — Build DuckDB warehouse from data/companies.toml + data/job-posts.json

Usage:
  python warehouse/build.py [--db data/warehouse.duckdb] [--parquet data/parquet] [--gold data/gold]
  HF_DATASET=username/software-companies-bd python warehouse/build.py --push

Outputs:
  - data/warehouse.duckdb          (primary warehouse, tables + views)
  - data/parquet/dim_company.parquet
  - data/parquet/fact_job.parquet
  - data/parquet/bridge_job_tag.parquet
  - data/gold/*.json               (pre-aggregated for dashboard, superfast CDN)
  - data/gold/*.parquet            (for DuckDB-WASM on HF)

HF: parquet + gold are pushed to Hugging Face Datasets (dataset viewer + CDN).
    warehouse.duckdb is also pushed as artifact for local analytics.
"""
import argparse
import hashlib
import json
import re
import sys
import os
from pathlib import Path
from datetime import datetime, timezone, date
from urllib.parse import urlparse

try:
    import tomllib
except ImportError:
    import tomli as tomllib
import duckdb

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"
WAREHOUSE_DIR = ROOT / "warehouse"

def fnv_host(host: str) -> int:
    h = 0xCBF29CE484222325
    for b in host.encode():
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h & 0x7FFFFFFF  # fit 32-bit signed for DuckDB INTEGER

def url_host(url: str) -> str:
    try:
        h = urlparse(url).hostname or ""
        h = h.lower().lstrip("www.")
        return h
    except:
        return ""

def content_hash(company: str, title: str, source: str|None) -> str:
    return hashlib.sha256(f"{company}|{title}|{source or ''}".encode()).hexdigest()[:16]

def parse_deadline(deadline_obj):
    if deadline_obj is None:
        return None, None, False, False
    if isinstance(deadline_obj, str) and deadline_obj == "Expired":
        return "Expired", None, True, False
    if isinstance(deadline_obj, dict) and "Expired" in deadline_obj:
        return "Expired", None, True, False
    if isinstance(deadline_obj, dict) and "Date" in deadline_obj:
        raw = deadline_obj["Date"]
        # raw is {"Absolute": "Aug 20, 2026"} or {"Relative": "2 days ago"}
        if isinstance(raw, dict):
            s = raw.get("Absolute") or raw.get("Relative") or ""
        else:
            s = str(raw)
        # try parse date
        d = try_parse_date(s)
        is_expired = False
        if d and s and "Absolute" in str(deadline_obj):
            try:
                is_expired = d < date.today()
            except: pass
        return s, d.isoformat() if d else None, is_expired, not is_expired
    return str(deadline_obj), None, False, True

def try_parse_date(s: str):
    if not s: return None
    # reuse Rust date logic: try chrono parse_date; we try several formats
    fmts = ["%Y-%m-%d", "%B %d, %Y", "%b %d, %Y", "%d %B %Y", "%d %b %Y", "%Y/%m/%d", "%d-%m-%Y", "%m/%d/%Y", "%d %B, %Y"]
    s_clean = s.strip().replace(".", "").replace(",", "")
    # handle "28th February 2026" -> "28 February 2026"
    s_clean = re.sub(r"(\d+)(st|nd|rd|th)", r"\1", s_clean, flags=re.I)
    for fmt in fmts:
        try:
            return datetime.strptime(s_clean, fmt.strip().replace(",", "")).date()
        except: continue
        try:
            return datetime.strptime(s_clean, fmt).date()
        except: continue
    # fallback: try ISO
    try:
        return datetime.fromisoformat(s_clean).date()
    except: return None

def location_normalize(loc_obj):
    if loc_obj is None:
        return None, None, "Unknown"
    if isinstance(loc_obj, str):
        if loc_obj.lower() == "remote": return loc_obj, loc_obj, "Remote"
        return loc_obj, loc_obj, "OnSite"
    if isinstance(loc_obj, dict):
        if "Remote" in loc_obj:
            return json.dumps(loc_obj), "Remote", "Remote"
        if "Hybrid" in loc_obj:
            t = loc_obj["Hybrid"]
            return json.dumps(loc_obj), t, "Hybrid"
        if "OnSite" in loc_obj:
            t = loc_obj["OnSite"]
            return json.dumps(loc_obj), t, "OnSite"
    return json.dumps(loc_obj), str(loc_obj), "Unknown"

def build(db_path: Path, parquet_dir: Path, gold_dir: Path, push: bool=False):
    db_path.parent.mkdir(parents=True, exist_ok=True)
    parquet_dir.mkdir(parents=True, exist_ok=True)
    gold_dir.mkdir(parents=True, exist_ok=True)

    # load companies.toml
    with open(DATA / "companies.toml", "rb") as f:
        companies = tomllib.load(f)
    # load job-posts.json
    with open(DATA / "job-posts.json", "r") as f:
        jobs_raw = json.load(f)

    crawl_id = datetime.now(timezone.utc).isoformat()

    # prepare rows
    dim_rows = []
    fact_rows = []
    bridge_rows = []
    now = datetime.now(timezone.utc)

    for name, meta in companies.items():
        website = meta.get("website", "")
        host = url_host(website)
        cid = fnv_host(host or name)
        has_job = meta.get("job") is not None
        dim_rows.append((
            cid, name, website, meta.get("job"), host,
            meta.get("linkedin"), meta.get("github"), meta.get("twitter"),
            meta.get("facebook"), meta.get("youtube"),
            meta.get("tech", []), meta.get("type", []),
            has_job, len(meta.get("tech", []))
        ))

    # fact
    for company_name, entry in jobs_raw.items():
        source_company_url = entry.get("source", "")
        # find company_id via name lookup
        # need to map name -> cid
        host = url_host(companies.get(company_name, {}).get("website","") or "")
        cid = fnv_host(host or company_name)
        if company_name not in companies:
            # company may have been removed but still in job-posts.json
            cid = fnv_host(company_name)
        for job in entry.get("jobs", []):
            title = job.get("title","").strip()
            if not title: continue
            desc = job.get("description","")
            source = job.get("source")
            # resolve source absolute if relative
            if source and source.startswith("/"):
                try:
                    from urllib.parse import urljoin
                    source = urljoin(source_company_url, source)
                except: pass
            jid = content_hash(company_name, title, source)
            deadline_raw = job.get("deadline")
            deadline_str, deadline_date, is_expired, is_open = parse_deadline(deadline_raw)
            # if expired, is_open False
            # also handle null deadline => open
            if deadline_raw is None:
                is_open = True
                is_expired = False
            loc_raw, loc_text, loc_type = location_normalize(job.get("location"))
            emp = job.get("employmentType") or job.get("employment_type")
            cat = job.get("category")
            if isinstance(cat, dict):
                cat = cat.get("Other") or str(cat)
            posted = job.get("postedAt") or job.get("posted_at")
            if isinstance(posted, dict):
                posted = posted.get("Absolute") or posted.get("Relative") or str(posted)
            exp = job.get("experience")
            sal = job.get("salary") or {}
            s_min = sal.get("min") if isinstance(sal, dict) else None
            s_max = sal.get("max") if isinstance(sal, dict) else None
            cur = sal.get("currency") if isinstance(sal, dict) else None
            apply_json = job.get("apply", [])
            apply_links = []
            for a in apply_json:
                if isinstance(a, dict) and "Website" in a:
                    apply_links.append(a["Website"])
                elif isinstance(a, dict) and "Email" in a:
                    apply_links.append(f"mailto:{a['Email']}")
                elif isinstance(a, str):
                    apply_links.append(a)
            # resolve apply links
            resolved_apply = []
            for link in apply_links:
                if link.startswith("mailto:"):
                    resolved_apply.append(link)
                elif link.startswith("/") and source_company_url:
                    try:
                        from urllib.parse import urljoin
                        resolved_apply.append(urljoin(source_company_url, link))
                    except:
                        resolved_apply.append(link)
                else:
                    resolved_apply.append(link)

            fact_rows.append((
                jid, cid, company_name, source_company_url,
                title, desc, len(desc),
                emp, job.get("role"), cat,
                posted, json.dumps(deadline_raw) if deadline_raw else None, deadline_date,
                is_expired, is_open,
                json.dumps(job.get("location")) if job.get("location") else None, loc_text, loc_type,
                exp, s_min, s_max, cur, job.get("vacancies"),
                job.get("tags", []),
                json.dumps(apply_json), resolved_apply,
                source, float(job.get("confidence", 1.0)), bool(job.get("needsFetch", False)),
                now, now, crawl_id
            ))
            for tag in job.get("tags", []):
                bridge_rows.append((jid, tag))

    # duckdb
    con = duckdb.connect(str(db_path))
    con.execute("PRAGMA disable_progress_bar;")
    # load schema
    schema_sql = (WAREHOUSE_DIR / "schema.sql").read_text()
    con.execute(schema_sql)

    # insert dim
    con.executemany("""
        INSERT INTO dim_company (company_id, name, website, job_url, host, linkedin, github, twitter, facebook, youtube, tech, company_type, has_job_page, tech_count)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (company_id) DO UPDATE SET
          name=excluded.name, website=excluded.website, job_url=excluded.job_url, host=excluded.host,
          linkedin=excluded.linkedin, github=excluded.github, tech=excluded.tech, company_type=excluded.company_type,
          has_job_page=excluded.has_job_page, tech_count=excluded.tech_count, updated_at=now()
    """, dim_rows)

    # insert fact
    con.executemany("""
        INSERT OR REPLACE INTO fact_job (
          job_id, company_id, company_name, source_company_url, title, description_md, description_len,
          employment_type, role, category, posted_at_raw, deadline_raw, deadline_date, is_expired, is_open,
          location_raw, location_text, location_type, experience, salary_min, salary_max, salary_currency,
          vacancies, tags, apply_json, apply_links, source_url, confidence, needs_fetch, first_seen_at, last_seen_at, crawl_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    """, fact_rows)

    con.executemany("INSERT OR REPLACE INTO bridge_job_tag (job_id, tag) VALUES (?, ?)", bridge_rows)

    # snapshot SCD2 lite
    con.execute("""
        INSERT OR REPLACE INTO job_snapshot (job_id, company_id, title, first_seen_at, last_seen_at, is_current, valid_from, valid_to)
        SELECT job_id, company_id, title, first_seen_at, last_seen_at, true, first_seen_at, NULL FROM fact_job
    """)

    # parquet exports (HF expects hive-style, one file per table)
    con.execute(f"COPY (SELECT * FROM dim_company) TO '{parquet_dir}/dim_company.parquet' (FORMAT PARQUET, COMPRESSION ZSTD);")
    con.execute(f"COPY (SELECT * FROM fact_job) TO '{parquet_dir}/fact_job.parquet' (FORMAT PARQUET, COMPRESSION ZSTD);")
    con.execute(f"COPY (SELECT * FROM bridge_job_tag) TO '{parquet_dir}/bridge_job_tag.parquet' (FORMAT PARQUET, COMPRESSION ZSTD);")
    con.execute(f"COPY (SELECT * FROM job_snapshot) TO '{parquet_dir}/job_snapshot.parquet' (FORMAT PARQUET, COMPRESSION ZSTD);")

    # gold aggregates -> JSON + parquet for dashboard (superfast: static JSON via CDN)
    gold_queries = {
        "stats": "SELECT (SELECT COUNT(*) FROM dim_company) AS total_companies, (SELECT COUNT(*) FROM dim_company WHERE has_job_page) AS companies_with_jobs, (SELECT COUNT(*) FROM fact_job WHERE is_open) AS open_jobs, (SELECT COUNT(*) FROM fact_job) AS total_jobs, (SELECT COUNT(DISTINCT company_id) FROM fact_job WHERE is_open) AS hiring_companies",
        "jobs_per_company": "SELECT * FROM v_jobs_per_company",
        "tech_demand": "SELECT * FROM v_tech_demand LIMIT 100",
        "company_tech": "SELECT * FROM v_company_tech",
        "location_heatmap": "SELECT * FROM v_location_heatmap LIMIT 50",
        "salary_stats": "SELECT * FROM v_salary_stats",
        "employment_breakdown": "SELECT * FROM v_employment_breakdown",
        "recent_jobs": "SELECT company_name, title, location_text, location_type, employment_type, salary_min, salary_max, salary_currency, tags, source_url, apply_links, last_seen_at, SUBSTRING(description_md, 1, 6000) AS description_md, experience FROM fact_job WHERE is_open ORDER BY last_seen_at DESC LIMIT 150",
        "companies": "SELECT company_id, name, website, job_url, host, tech, company_type, has_job_page FROM dim_company ORDER BY name",
    }
    import pandas as pd
    for name, sql in gold_queries.items():
        df = con.execute(sql).fetchdf()
        # parquet
        df.to_parquet(gold_dir / f"{name}.parquet", compression="zstd", index=False)
        # json (for edge dashboard fetch - fastest: single JSON, CDN cached, no WASM)
        df.to_json(gold_dir / f"{name}.json", orient="records", indent=2, date_format="iso")
        print(f"gold/{name}: {len(df)} rows")

    # also export single api.json for dashboard convenience
    stats = con.execute(gold_queries["stats"]).fetchone()
    print(f"Warehouse: {len(dim_rows)} companies, {len(fact_rows)} jobs, {len(bridge_rows)} tags")
    print(f"DuckDB: {db_path}  Parquet: {parquet_dir}  Gold: {gold_dir}")

    con.close()

    if push:
        try:
            from warehouse.hf_push import push_to_hf
            push_to_hf(parquet_dir, gold_dir, db_path)
        except Exception as e:
            print(f"HF push failed: {e}", file=sys.stderr)

    return db_path

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=str(DATA / "warehouse.duckdb"))
    ap.add_argument("--parquet", default=str(DATA / "parquet"))
    ap.add_argument("--gold", default=str(DATA / "gold"))
    ap.add_argument("--push", action="store_true", help="push parquet/gold to Hugging Face")
    args = ap.parse_args()
    build(Path(args.db), Path(args.parquet), Path(args.gold), push=args.push)
