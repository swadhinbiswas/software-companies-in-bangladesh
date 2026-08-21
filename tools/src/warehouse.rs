//! tools/src/warehouse.rs — Rust wrapper to trigger Python warehouse build after crawl/docs.
//!
//! Keeps the fast Rust extractor but reuses the Python DuckDB pipeline (single source of truth).
//! Called from `main.rs` after `jobs::run()` or via `cargo run -- warehouse`.

use std::path::Path;
use std::process::Command;

pub fn build_warehouse(dir: &Path) -> crate::Result {
    let script = dir.join("warehouse/build.py");
    if !script.exists() {
        log::warn!("warehouse/build.py not found, skipping warehouse build");
        return Ok(());
    }
    log::info!("Building DuckDB warehouse via warehouse/build.py ...");
    let status = Command::new("python3")
        .arg(&script)
        .arg("--db")
        .arg(dir.join("data/warehouse.duckdb"))
        .arg("--parquet")
        .arg(dir.join("data/parquet"))
        .arg("--gold")
        .arg(dir.join("data/gold"))
        .current_dir(dir)
        .status()?;

    if !status.success() {
        return Err(format!("warehouse build failed: {}", status).into());
    }
    log::info!("Warehouse built: data/warehouse.duckdb + data/parquet + data/gold");
    Ok(())
}
