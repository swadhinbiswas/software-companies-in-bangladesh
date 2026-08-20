#!/usr/bin/env bash
# Run the full job-crawling pipeline and report timing.
#
# Usage: ./crawl.sh [extra cli args...]
#   e.g. ./crawl.sh --force        (clear cache and re-fetch everything)
#        ./crawl.sh -c 4           (lower concurrency)
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOLS_DIR="$REPO_DIR/tools"
BIN="$TOOLS_DIR/target/release/cli"

cd "$REPO_DIR"

echo "=================================================="
echo "  Software Companies in Bangladesh - Job Crawler"
echo "=================================================="

# 1. Build the crawler if the release binary is missing or outdated.
if [[ ! -x "$BIN" ]]; then
    echo "[1/4] Building release binary..."
    cargo build --release --manifest-path "$TOOLS_DIR/Cargo.toml"
else
    echo "[1/4] Using existing binary: $BIN"
fi

# 2. Run the crawl (phase 1+2+3 inside the tool).
echo "[2/4] Crawling companies + LLM extraction..."
START=$(date +%s)
"$BIN" index "$@"
CRAWL_ELAPSED=$(( $(date +%s) - START ))

# 3. Regenerate jobs.md from the fresh data.
echo "[3/4] Regenerating jobs.md..."
"$BIN" --docs 2>/dev/null || true
DOCS_ELAPSED=$(( $(date +%s) - START ))

# 4. Summary.
echo "[4/4] Done."
echo "=================================================="
printf "Crawl time:      %s\n" "$(printf '%02d:%02d' $((CRAWL_ELAPSED / 60)) $((CRAWL_ELAPSED % 60)))"
printf "Total pipeline:  %s\n" "$(printf '%02d:%02d' $((DOCS_ELAPSED / 60)) $((DOCS_ELAPSED % 60)))"

if [[ -f "$REPO_DIR/data/job-posts.json" ]]; then
    COMPANIES=$(python3 -c "import json;print(len(json.load(open('$REPO_DIR/data/job-posts.json'))))" 2>/dev/null || echo "?")
    JOBS=$(python3 -c "import json;print(sum(len(v['jobs']) for v in json.load(open('$REPO_DIR/data/job-posts.json')).values()))" 2>/dev/null || echo "?")
    echo "Companies with jobs: $COMPANIES"
    echo "Total jobs:          $JOBS"
fi
echo "Output: data/job-posts.json + jobs.md"
