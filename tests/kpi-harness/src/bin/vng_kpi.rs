//! `vng-kpi` — KPI measurement harness CLI (E-1..E-6).
//!
//! Drives real concurrent load against a live VoltNueronGrid server and emits a
//! JSON artifact (with a `status` field) per scenario. Gate scripts read the
//! artifact's `status`.
//!
//! Usage:
//!   vng-kpi oltp      --base-url URL --admin-key K [--concurrency N] [--duration S] [--p95 MS] [--p99 MS] --out PATH
//!   vng-kpi olap      --base-url URL --admin-key K [--rows N] [--concurrency N] [--duration S] [--p95 MS] [--p99 MS] --out PATH
//!   vng-kpi htap      --base-url URL --admin-key K [--readers N] [--writers N] [--duration S] [--read-qps-min Q] [--write-tps-min T] --out PATH
//!   vng-kpi ingest    --base-url URL --admin-key K [--rows N] [--workers 1,2,4,8] [--min-efficiency F] --out PATH
//!   vng-kpi connector --base-url URL --admin-key K [--cycles N] [--min-rate F] --out PATH

use std::collections::HashMap;
use std::time::Duration;
use vng_kpi_harness::{
    run_connector_reliability, run_htap_throughput, run_ingest_scaling, run_olap_latency,
    run_oltp_latency, write_artifact, HarnessConfig,
};

fn parse_flags(args: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(key) = args[i].strip_prefix("--") {
            let val = args.get(i + 1).cloned().unwrap_or_default();
            map.insert(key.to_string(), val);
            i += 2;
        } else {
            i += 1;
        }
    }
    map
}

fn get<'a>(f: &'a HashMap<String, String>, k: &str, default: &'a str) -> String {
    f.get(k).cloned().unwrap_or_else(|| default.to_string())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: vng-kpi <oltp|olap|htap|ingest|connector> --base-url URL --admin-key K --out PATH [flags]");
        std::process::exit(2);
    }
    let scenario = args[1].clone();
    let flags = parse_flags(&args[2..]);

    let base_url = get(&flags, "base-url", "http://127.0.0.1:8080");
    let admin_key = get(&flags, "admin-key", "");
    let cfg = HarnessConfig::from_env_or(&base_url, &admin_key);
    let out = get(&flags, "out", "tests/kpi/results/e/kpi-result.json");

    let artifact: serde_json::Value = match scenario.as_str() {
        "oltp" => {
            let concurrency: usize = get(&flags, "concurrency", "64").parse().unwrap_or(64);
            let duration: u64 = get(&flags, "duration", "60").parse().unwrap_or(60);
            let p95: f64 = get(&flags, "p95", "20").parse().unwrap_or(20.0);
            let p99: f64 = get(&flags, "p99", "60").parse().unwrap_or(60.0);
            let r = run_oltp_latency(&cfg, concurrency, Duration::from_secs(duration), p95, p99).await;
            serde_json::to_value(&r).unwrap()
        }
        "olap" => {
            let rows: usize = get(&flags, "rows", "100000").parse().unwrap_or(100_000);
            let concurrency: usize = get(&flags, "concurrency", "16").parse().unwrap_or(16);
            let duration: u64 = get(&flags, "duration", "60").parse().unwrap_or(60);
            let p95: f64 = get(&flags, "p95", "800").parse().unwrap_or(800.0);
            let p99: f64 = get(&flags, "p99", "1500").parse().unwrap_or(1500.0);
            let (r, loaded) =
                run_olap_latency(&cfg, rows, concurrency, Duration::from_secs(duration), p95, p99).await;
            let mut v = serde_json::to_value(&r).unwrap();
            v["rows_loaded"] = serde_json::json!(loaded);
            v
        }
        "htap" => {
            let readers: usize = get(&flags, "readers", "32").parse().unwrap_or(32);
            let writers: usize = get(&flags, "writers", "32").parse().unwrap_or(32);
            let duration: u64 = get(&flags, "duration", "60").parse().unwrap_or(60);
            let read_min: f64 = get(&flags, "read-qps-min", "25000").parse().unwrap_or(25000.0);
            let write_min: f64 = get(&flags, "write-tps-min", "10000").parse().unwrap_or(10000.0);
            let r = run_htap_throughput(&cfg, readers, writers, Duration::from_secs(duration), read_min, write_min).await;
            serde_json::to_value(&r).unwrap()
        }
        "ingest" => {
            let rows: usize = get(&flags, "rows", "8000").parse().unwrap_or(8000);
            let workers: Vec<usize> = get(&flags, "workers", "1,2,4,8")
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            let min_eff: f64 = get(&flags, "min-efficiency", "0.80").parse().unwrap_or(0.80);
            let r = run_ingest_scaling(&cfg, rows, &workers, min_eff).await;
            serde_json::to_value(&r).unwrap()
        }
        "connector" => {
            let cycles: u64 = get(&flags, "cycles", "1000").parse().unwrap_or(1000);
            let min_rate: f64 = get(&flags, "min-rate", "0.9995").parse().unwrap_or(0.9995);
            let r = run_connector_reliability(&cfg, cycles, min_rate).await;
            serde_json::to_value(&r).unwrap()
        }
        other => {
            eprintln!("unknown scenario: {other}");
            std::process::exit(2);
        }
    };

    if let Err(e) = write_artifact(&out, &artifact) {
        eprintln!("failed to write artifact {out}: {e}");
        std::process::exit(1);
    }
    let status = artifact.get("status").and_then(|s| s.as_str()).unwrap_or("unknown");
    println!("{} → {} ({})", scenario, out, status);
    // Non-zero exit on failure so callers can short-circuit if desired.
    if status != "passed" {
        std::process::exit(1);
    }
}
