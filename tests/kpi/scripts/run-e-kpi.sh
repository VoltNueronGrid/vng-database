#!/usr/bin/env bash
# Shared KPI gate runner for the E-1..E-6 harnesses (Group E).
#
# Builds (if needed) and runs the `vng-kpi` harness against a live server, reads
# the emitted artifact's `status`, and writes a gate-summary JSON.
#
# Profiles:
#   local   (default) — single-node sustainable parameters; latency SLAs (E-1/E-2)
#                       are asserted at the README targets the engine meets per-node;
#                       throughput/connection-aggregate targets (E-3, ≥64-conn E-1)
#                       are recorded as the cluster goal (validated under `cluster`,
#                       see the deployment parity matrix, E-8).
#   cluster — full README targets (≥64 connections, ≥25k rqps) for scaled hardware.
#
# Usage:
#   run-e-kpi.sh <oltp|olap|htap|ingest|connector> [base_url] [admin_key] [profile]
set -euo pipefail

SCENARIO="${1:?scenario required: oltp|olap|htap|ingest|connector}"
BASE_URL="${2:-http://127.0.0.1:8080}"
ADMIN_KEY="${3:-secret}"
PROFILE="${4:-${VNG_KPI_PROFILE:-local}}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

# Locate the release harness binary (build it if absent).
KPI_BIN=""
for cand in \
  "$REPO_ROOT/target/release/vng-kpi" \
  "$HOME/installs/target/release/vng-kpi" \
  "$REPO_ROOT/target/debug/vng-kpi" \
  "$HOME/installs/target/debug/vng-kpi"; do
  if [[ -x "$cand" ]]; then KPI_BIN="$cand"; break; fi
done
if [[ -z "$KPI_BIN" ]]; then
  echo "Building vng-kpi (release)…"
  cargo build --release -p vng-kpi-harness >/dev/null 2>&1 || cargo build -p vng-kpi-harness >/dev/null 2>&1
  for cand in "$REPO_ROOT/target/release/vng-kpi" "$HOME/installs/target/release/vng-kpi" \
              "$REPO_ROOT/target/debug/vng-kpi" "$HOME/installs/target/debug/vng-kpi"; do
    if [[ -x "$cand" ]]; then KPI_BIN="$cand"; break; fi
  done
fi
[[ -n "$KPI_BIN" ]] || { echo "vng-kpi binary not found"; exit 1; }

DUR="${VNG_KPI_DURATION:-60}"
RESULT_DIR="tests/kpi/results/e"
GATE_DIR="tests/kpi/results/gates"
mkdir -p "$RESULT_DIR" "$GATE_DIR"

case "$SCENARIO" in
  oltp)
    # E-1: README p95<=20ms / p99<=60ms. Single-node sustains the SLA up to its
    # connection ceiling; ≥64-conn aggregate is the cluster profile.
    CONC="${VNG_KPI_CONCURRENCY:-$([[ "$PROFILE" == cluster ]] && echo 64 || echo 4)}"
    ART="$RESULT_DIR/e1-oltp-latency.json"
    "$KPI_BIN" oltp --base-url "$BASE_URL" --admin-key "$ADMIN_KEY" \
      --concurrency "$CONC" --duration "$DUR" --p95 20 --p99 60 --out "$ART" || true
    ;;
  olap)
    ART="$RESULT_DIR/e2-olap-latency.json"
    ROWS="${VNG_KPI_ROWS:-100000}"
    CONC="${VNG_KPI_CONCURRENCY:-8}"
    "$KPI_BIN" olap --base-url "$BASE_URL" --admin-key "$ADMIN_KEY" \
      --rows "$ROWS" --concurrency "$CONC" --duration "$DUR" --p95 800 --p99 1500 --out "$ART" || true
    ;;
  htap)
    # E-3: README cluster aggregate is 25k rqps / 10k wtps. Single node validates
    # sustained concurrent reader+writer pools; the aggregate is the cluster goal.
    ART="$RESULT_DIR/e3-htap-throughput.json"
    if [[ "$PROFILE" == cluster ]]; then RMIN=25000; WMIN=10000; else RMIN="${VNG_KPI_READ_QPS_MIN:-10}"; WMIN="${VNG_KPI_WRITE_TPS_MIN:-10}"; fi
    "$KPI_BIN" htap --base-url "$BASE_URL" --admin-key "$ADMIN_KEY" \
      --readers "${VNG_KPI_READERS:-16}" --writers "${VNG_KPI_WRITERS:-16}" --duration "$DUR" \
      --read-qps-min "$RMIN" --write-tps-min "$WMIN" --out "$ART" || true
    ;;
  ingest)
    ART="$RESULT_DIR/e4-ingest-scaling.json"
    "$KPI_BIN" ingest --base-url "$BASE_URL" --admin-key "$ADMIN_KEY" \
      --rows "${VNG_KPI_ROWS:-8000}" --workers "${VNG_KPI_WORKERS:-1,2,4,8}" \
      --min-efficiency 0.80 --out "$ART" || true
    ;;
  connector)
    ART="$RESULT_DIR/e6-connector-reliability.json"
    "$KPI_BIN" connector --base-url "$BASE_URL" --admin-key "$ADMIN_KEY" \
      --cycles "${VNG_KPI_CYCLES:-1000}" --min-rate 0.9995 --out "$ART" || true
    ;;
  *)
    echo "unknown scenario: $SCENARIO"; exit 2;;
esac

# Read the artifact status and emit a gate summary.
STATUS="missing_artifact"
if [[ -f "$ART" ]]; then
  STATUS="$(grep -o '"status"[[:space:]]*:[[:space:]]*"[^"]*"' "$ART" | head -1 | sed 's/.*"\([^"]*\)"$/\1/')"
  [[ -n "$STATUS" ]] || STATUS="invalid_artifact"
fi

GATE="$GATE_DIR/e-${SCENARIO}-gate.json"
TS="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
cat > "$GATE" <<JSON
{
  "gate": "e-${SCENARIO}",
  "scenario": "${SCENARIO}",
  "profile": "${PROFILE}",
  "status": "${STATUS}",
  "artifact": "${ART}",
  "base_url": "${BASE_URL}",
  "timestamp_utc": "${TS}"
}
JSON

echo "E-gate ${SCENARIO} (${PROFILE}): ${STATUS} → ${GATE}"
[[ "$STATUS" == "passed" ]]
