#!/usr/bin/env bash
# E-7 · Autonomous action safety validation gate.
#
# Runs the consolidated e7 service test (which drives every autonomous action
# endpoint and emits the coverage artifact) and derives gate status from the
# emitted artifact `status` field — never from $? of the nested cargo run.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

ARTIFACT="tests/kpi/results/e/e7-autonomous-safety.json"
GATE_DIR="tests/kpi/results/gates"
mkdir -p "$GATE_DIR"

echo "=== E-7 autonomous safety gate ==="
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/installs/target}" \
  cargo test -p voltnuerongridd e7_autonomous_actions_all_audited -- --nocapture >/tmp/e7-gate.log 2>&1 || true

STATUS="missing"
COVERAGE="0"
if [[ -f "$ARTIFACT" ]]; then
  STATUS="$(grep -o '"status"[[:space:]]*:[[:space:]]*"[^"]*"' "$ARTIFACT" | head -1 | sed 's/.*"\([^"]*\)"$/\1/')"
  COVERAGE="$(grep -o '"coverage_pct"[[:space:]]*:[[:space:]]*[0-9.]*' "$ARTIFACT" | head -1 | sed 's/.*:[[:space:]]*//')"
fi

TS="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
SUMMARY="$GATE_DIR/e7-safety-gate.json"
cat > "$SUMMARY" <<JSON
{
  "gate": "e7-autonomous-safety",
  "status": "${STATUS}",
  "coverage_pct": ${COVERAGE:-0},
  "artifact": "${ARTIFACT}",
  "timestamp_utc": "${TS}"
}
JSON

echo "E-7 gate: ${STATUS} (coverage ${COVERAGE}%) → ${SUMMARY}"
cat "$SUMMARY"
[[ "$STATUS" == "passed" ]]
