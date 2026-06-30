#!/usr/bin/env bash
# E-1..E-6 KPI gate orchestrator (Group E).
#
# Runs each KPI harness against a FRESH release server (clean data dir) so
# measurements are not contaminated by cross-scenario state accumulation —
# standard benchmarking hygiene. Aggregates the per-scenario gate artifacts into
# an E-group release-readiness summary.
#
# Usage:
#   run-e-all-gate.sh [profile]      # profile: local (default) | cluster
set -euo pipefail

PROFILE="${1:-${VNG_KPI_PROFILE:-local}}"
ADMIN_KEY="${VNG_ADMIN_API_KEY:-secret}"
PORT="${VNG_KPI_PORT:-8080}"
BASE_URL="http://127.0.0.1:${PORT}"
DUR="${VNG_KPI_DURATION:-30}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

# Locate the release server binary.
SERVER_BIN=""
for cand in "$REPO_ROOT/target/release/voltnuerongridd" "$HOME/installs/target/release/voltnuerongridd"; do
  [[ -x "$cand" ]] && { SERVER_BIN="$cand"; break; }
done
if [[ -z "$SERVER_BIN" ]]; then
  echo "Building voltnuerongridd (release)…"
  cargo build --release -p voltnuerongridd >/dev/null 2>&1
  for cand in "$REPO_ROOT/target/release/voltnuerongridd" "$HOME/installs/target/release/voltnuerongridd"; do
    [[ -x "$cand" ]] && { SERVER_BIN="$cand"; break; }
  done
fi
[[ -n "$SERVER_BIN" ]] || { echo "voltnuerongridd release binary not found"; exit 1; }

GATE_DIR="tests/kpi/results/gates"
mkdir -p "$GATE_DIR"

SCENARIOS=(oltp olap htap ingest connector)
declare -A RESULTS

run_one() {
  local scenario="$1"
  local data_dir; data_dir="$(mktemp -d "/tmp/vng-kpi-${scenario}-XXXX")"
  # Start a fresh server.
  VNG_ADMIN_API_KEY="$ADMIN_KEY" VNG_NATIVE_LISTENER_ENABLED=false VNG_DATA_DIR="$data_dir" \
    "$SERVER_BIN" >"/tmp/vng-kpi-${scenario}.log" 2>&1 &
  local pid=$!
  # Wait for health.
  local ready=0
  for _ in $(seq 1 30); do
    if curl -sf "${BASE_URL}/health" >/dev/null 2>&1; then ready=1; break; fi
    sleep 0.5
  done
  if [[ "$ready" != 1 ]]; then
    echo "server failed to become ready for ${scenario}"
    kill "$pid" 2>/dev/null || true
    RESULTS[$scenario]="server_unavailable"
    return
  fi
  # Run the gate (don't let its non-zero exit abort the orchestrator).
  set +e
  VNG_KPI_DURATION="$DUR" tests/kpi/scripts/run-e-kpi.sh "$scenario" "$BASE_URL" "$ADMIN_KEY" "$PROFILE"
  set -e
  local art="tests/kpi/results/gates/e-${scenario}-gate.json"
  local status="missing"
  [[ -f "$art" ]] && status="$(grep -o '"status"[[:space:]]*:[[:space:]]*"[^"]*"' "$art" | head -1 | sed 's/.*"\([^"]*\)"$/\1/')"
  RESULTS[$scenario]="$status"
  kill "$pid" 2>/dev/null || true
  sleep 1
  rm -rf "$data_dir"
}

echo "=== E-group KPI gate orchestrator (profile: ${PROFILE}) ==="
for s in "${SCENARIOS[@]}"; do
  echo ""
  echo "--- ${s} ---"
  run_one "$s"
done

# Aggregate.
ALL_PASS=1
ENTRIES=""
for s in "${SCENARIOS[@]}"; do
  st="${RESULTS[$s]:-missing}"
  [[ "$st" == "passed" ]] || ALL_PASS=0
  ENTRIES="${ENTRIES}    { \"scenario\": \"${s}\", \"status\": \"${st}\" },\n"
done
ENTRIES="$(printf '%b' "$ENTRIES" | sed '$ s/,$//')"
OVERALL="$([[ "$ALL_PASS" == 1 ]] && echo passed || echo failed)"
TS="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

SUMMARY="$GATE_DIR/e-group-release-readiness.json"
{
  echo "{"
  echo "  \"gate\": \"e-group\","
  echo "  \"profile\": \"${PROFILE}\","
  echo "  \"status\": \"${OVERALL}\","
  echo "  \"timestamp_utc\": \"${TS}\","
  echo "  \"scenarios\": ["
  printf '%b\n' "$ENTRIES"
  echo "  ]"
  echo "}"
} > "$SUMMARY"

echo ""
echo "=== E-group summary: ${OVERALL} → ${SUMMARY} ==="
cat "$SUMMARY"
[[ "$OVERALL" == passed ]]
