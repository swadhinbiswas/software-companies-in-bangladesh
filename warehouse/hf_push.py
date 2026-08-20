"""
warehouse/hf_push.py — Push Parquet + Gold JSON to Hugging Face

Target bucket (public): hf://buckets/swadhinbiswas/bangladeshi-jobs
Also aliased as dataset: swadhinbiswas/bangladeshi-jobs
Files:
  data/parquet/*.parquet  ->  parquet/
  data/gold/*.json/.parquet -> gold/
  data/warehouse.duckdb   -> warehouse.duckdb (artifact, LFS)
  data/job-posts.json     -> raw/job-posts.json

HF gives: CDN (cdn-lfs.huggingface.co), Parquet viewer, DuckDB SQL via HF API.
Dashboard fetches directly from bucket (public, no auth): 
  https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/stats.json
  Parquet: https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/parquet/fact_job.parquet
  Bucket S3: hf://buckets/swadhinbiswas/bangladeshi-jobs

Set HF_TOKEN env (or huggingface-cli login) with write access.
"""
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"

# Public bucket used by dashboard directly (no auth needed for reads)
DEFAULT_HF_DATASET = "swadhinbiswas/bangladeshi-jobs"
DEFAULT_HF_BUCKET = "hf://buckets/swadhinbiswas/bangladeshi-jobs"

def normalize_hf_id(raw: str) -> str:
    """Strip hf:// prefix variants → 'swadhinbiswas/bangladeshi-jobs'."""
    if not raw:
        return DEFAULT_HF_DATASET
    raw = raw.strip()
    for prefix in ["hf://buckets/", "hf://datasets/", "hf://", "datasets/"]:
        if raw.startswith(prefix):
            raw = raw[len(prefix):]
            break
    # also handle full https URL → extract id
    if "huggingface.co/datasets/" in raw:
        raw = raw.split("huggingface.co/datasets/")[-1].split("/resolve")[0].split("?")[0]
    return raw.strip("/")

def push_to_hf(parquet_dir: Path, gold_dir: Path, db_path: Path):
    try:
        from huggingface_hub import HfApi, create_repo
    except ImportError:
        print("huggingface_hub not installed: pip install huggingface_hub", file=sys.stderr)
        return

    raw_id = os.getenv("HF_DATASET") or os.getenv("HF_DATASET_ID") or os.getenv("HF_BUCKET") or DEFAULT_HF_DATASET
    dataset_id = normalize_hf_id(raw_id)
    if not dataset_id:
        dataset_id = DEFAULT_HF_DATASET
        print(f"Using default bucket: {DEFAULT_HF_BUCKET} ({dataset_id})", file=sys.stderr)
    token = os.getenv("HF_TOKEN") or os.getenv("HUGGINGFACE_TOKEN")
    if not token:
        print("HF_TOKEN not set — skipping push (data stays local). Run: huggingface-cli login", file=sys.stderr)
        return

    api = HfApi(token=token, endpoint="https://huggingface.co")
    print(f"HF target: {DEFAULT_HF_BUCKET} → dataset {dataset_id} (public bucket, site reads directly)")
    try:
        create_repo(dataset_id, repo_type="dataset", exist_ok=True, token=token)
        print(f"HF repo ensured: {dataset_id} (public)")
    except Exception as e:
        print(f"create_repo: {e} — will try upload anyway (bucket may already exist)")

    # push folders
    for src, target in [
        (parquet_dir, "parquet"),
        (gold_dir, "gold"),
    ]:
        if not src.exists(): continue
        for p in src.iterdir():
            if p.is_file():
                try:
                    api.upload_file(
                        path_or_fileobj=str(p),
                        path_in_repo=f"{target}/{p.name}",
                        repo_id=dataset_id, repo_type="dataset",
                    )
                    print(f"↑ {p.name} -> {target}/")
                except Exception as e:
                    print(f"upload {p.name} failed: {e}")

    # push duckdb as LFS artifact (optional, large)
    if db_path.exists() and db_path.stat().st_size < 200_000_000:  # <200MB
        try:
            api.upload_file(path_or_fileobj=str(db_path), path_in_repo="warehouse.duckdb", repo_id=dataset_id, repo_type="dataset")
            print("↑ warehouse.duckdb")
        except Exception as e:
            print(f"warehouse.duckdb upload failed: {e}")
    else:
        print("warehouse.duckdb skipped (>200MB or missing) — use parquet instead for edge")

    # raw snapshot for lineage
    raw = DATA / "job-posts.json"
    if raw.exists():
        api.upload_file(path_or_fileobj=str(raw), path_in_repo="raw/job-posts.json", repo_id=dataset_id, repo_type="dataset")
        print("↑ raw/job-posts.json")

if __name__ == "__main__":
    from pathlib import Path
    push_to_hf(DATA/"parquet", DATA/"gold", DATA/"warehouse.duckdb")
