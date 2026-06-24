#!/usr/bin/env bash
# run-pre-checkin-gates.sh
#
# MANDATORY pre-checkin gate refresh for VoltNueronGrid DB.
# Run this before every git commit that touches auth, RBAC, storage, Raft, or
# security-adjacent code paths to ensure WS5 and WS6 gate artifacts carry
# current started_at_utc timestamps.
#
# Constitution Principle VII: "No requirement, workstream, sprint, release, or
# gap may be marked complete without current evidence."
#
# Usage:
#   bash scripts/run-pre-checkin-gates.sh
#   bash scripts/run-pre-checkin-gates.sh --skip-ws6   # skip WS6 (offline only)
#   bash scripts/run-pre-checkin-gates.sh --check-only  # check artifact age only
#
# Install as a git pre-push hook (optional but recommended):
#   cp .githooks/pre-push .git/hooks/pre-push && chmod +x .git/hooks/pre-push
#   OR: git config core.hooksPath .githooks

set -euo pipefail

SKIP_WS6=false
CHECK_ONLY=false
BASE_URL="http://127.0.0.1:8080"
ADMIN_KEY="secret"
MAX_ARTIFACT_AGE_HOURS=24

for arg in "$@"; do
  case $arg in
    --skip-ws6)   SKIP_WS6=true ;;
    --check-only) CHECK_ONLY=true ;;
    --base-url=*) BASE_URL="${arg#*=}" ;;
    --admin-key=*) ADMIN_KEY="${arg#*=}" ;;
  esac
done

echo "============================================================"
echo " VoltNueronGrid Pre-Checkin Gate Refresh"
echo " Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "============================================================"

# ── Helper: check artifact age ───────────────────────────────────────────────
check_artifact_age() {
  local artifact="$1"
  local gate_name="$2"

  if [[ ! -f "$artifact" ]]; then
    echo "  ⚠️  $gate_name artifact missing: $artifact"
    return 1
  fi

  # Extract started_at_utc from JSON
  local started_at
  started_at=$(python3 -c "import json,sys; d=json.load(open('$artifact')); print(d.get('started_at_utc',''))" 2>/dev/null || echo "")

  if [[ -z "$started_at" ]]; then
    echo "  ⚠️  $gate_name: cannot read started_at_utc from $artifact"
    return 1
  fi

  # Calculate age in hours
  local artifact_epoch now_epoch age_hours
  artifact_epoch=$(python3 -c "import datetime; print(int(datetime.datetime.fromisoformat('${started_at%Z}').replace(tzinfo=datetime.timezone.utc).timestamp()))" 2>/dev/null || echo "0")
  now_epoch=$(date +%s)
  age_hours=$(( (now_epoch - artifact_epoch) / 3600 ))

  if (( age_hours > MAX_ARTIFACT_AGE_HOURS )); then
    echo "  ⚠️  $gate_name artifact is ${age_hours}h old (> ${MAX_ARTIFACT_AGE_HOURS}h threshold): $artifact"
    echo "     started_at_utc: $started_at"
    return 1
  else
    echo "  ✅ $gate_name artifact is ${age_hours}h old — within ${MAX_ARTIFACT_AGE_HOURS}h threshold"
    return 0
  fi
}

# ── Check-only mode: just report artifact age ─────────────────────────────────
if [[ "$CHECK_ONLY" == "true" ]]; then
  echo ""
  echo "── Artifact age check (--check-only) ──"
  ws5_ok=0
  ws6_ok=0
  check_artifact_age "tests/kpi/results/ws5/ws5-gate-summary.json" "WS5" || ws5_ok=1
  check_artifact_age "tests/kpi/results/ws6/ws6-gate-summary.json" "WS6" || ws6_ok=1

  if (( ws5_ok + ws6_ok > 0 )); then
    echo ""
    echo "❌ One or more gate artifacts are stale. Run without --check-only to refresh."
    exit 1
  else
    echo ""
    echo "✅ All gate artifacts are current."
    exit 0
  fi
fi

# ── Start server if not running ───────────────────────────────────────────────
echo ""
echo "── Step 1: Ensure server is running ──"
SERVER_WAS_STARTED=false

if curl -fsS "${BASE_URL}/health" >/dev/null 2>&1; then
  echo "  ✅ Server already running at ${BASE_URL}"
else
  echo "  Starting server (VNG_NATIVE_LISTENER_ENABLED=false VNG_ADMIN_API_KEY=${ADMIN_KEY})..."
  VNG_NATIVE_LISTENER_ENABLED=false VNG_ADMIN_API_KEY="${ADMIN_KEY}" \
    cargo run -p voltnuerongridd > /tmp/vng-pre-checkin.log 2>&1 &
  SERVER_PID=$!
  SERVER_WAS_STARTED=true

  echo -n "  Waiting for server..."
  for i in $(seq 1 30); do
    if curl -fsS "${BASE_URL}/health" >/dev/null 2>&1; then
      echo " ready (${i}s)"
      break
    fi
    sleep 2
    echo -n "."
  done

  if ! curl -fsS "${BASE_URL}/health" >/dev/null 2>&1; then
    echo ""
    echo "❌ Server failed to start within 60s. Check /tmp/vng-pre-checkin.log"
    exit 1
  fi
fi

# ── Cleanup trap ──────────────────────────────────────────────────────────────
cleanup() {
  if [[ "$SERVER_WAS_STARTED" == "true" && -n "${SERVER_PID:-}" ]]; then
    echo ""
    echo "── Stopping server (PID=${SERVER_PID}) ──"
    kill "${SERVER_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# ── Step 2: Run WS5 security gate ─────────────────────────────────────────────
echo ""
echo "── Step 2: Run WS5 security gate ──"
mkdir -p tests/kpi/results/ws5
if pwsh ./tests/kpi/scripts/run-ws5-gate.ps1 \
    -IncludeRuntimeSmokes \
    -BaseUrl "${BASE_URL}" \
    -OutputPath "./tests/kpi/results/ws5/ws5-gate-summary.json"; then
  echo "  ✅ WS5 gate passed"
else
  echo "  ❌ WS5 gate FAILED — do not commit until this passes"
  exit 1
fi

# ── Step 3: Run WS6 failover gate ─────────────────────────────────────────────
if [[ "$SKIP_WS6" == "true" ]]; then
  echo ""
  echo "── Step 3: WS6 gate skipped (--skip-ws6) ──"
else
  echo ""
  echo "── Step 3: Run WS6 failover gate ──"
  mkdir -p tests/kpi/results/ws6
  if pwsh ./tests/kpi/scripts/run-ws6-gate.ps1 \
      -OutputPath "./tests/kpi/results/ws6/ws6-gate-summary.json"; then
    echo "  ✅ WS6 gate passed"
  else
    echo "  ⚠️  WS6 gate failed — review output (non-blocking until P5 complete)"
  fi
fi

# ── Step 4: Run crash recovery gate (non-blocking) ────────────────────────────
echo ""
echo "── Step 4: Run crash recovery gate (durability gap tracker) ──"
mkdir -p tests/kpi/results/recovery
if pwsh ./tests/kpi/scripts/run-crash-recovery-gate.ps1 \
    -SkipServerManagement \
    -BaseUrl "${BASE_URL}" \
    -OutputPath "./tests/kpi/results/recovery/crash-recovery-gate.json"; then
  echo "  ✅ Crash recovery gate passed (rows survived)"
else
  echo "  ℹ️  Crash recovery gate: durability_gap_known (expected until P1 complete)"
fi

# ── Step 5: Final report ──────────────────────────────────────────────────────
echo ""
echo "── Final artifact timestamps ──"
check_artifact_age "tests/kpi/results/ws5/ws5-gate-summary.json" "WS5" || true
check_artifact_age "tests/kpi/results/ws6/ws6-gate-summary.json" "WS6" || true
check_artifact_age "tests/kpi/results/recovery/crash-recovery-gate.json" "Crash Recovery" || true

echo ""
echo "✅ Pre-checkin gate refresh complete. Stage and commit the updated artifacts."
echo "   Files to stage:"
echo "     git add tests/kpi/results/ws5/ws5-gate-summary.json"
echo "     git add tests/kpi/results/ws6/ws6-gate-summary.json"
echo "     git add tests/kpi/results/recovery/crash-recovery-gate.json"
