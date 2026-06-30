#!/usr/bin/env bash
# E-5 · Failover RTO/RPO real measurement (multi-node).
#
# Spins up a real 3-node Raft cluster, commits N rows through the leader, kills
# the leader process, then measures:
#   • RTO  — wall-clock seconds until a surviving node reports `role: leader`
#            (asserts ≤ 30 s)
#   • RPO  — committed rows that survive on the new leader, via a row-count diff
#            (asserts 0 lost rows under the strict-sync / linearisable write path)
#
# Emits tests/kpi/results/e/e5-failover-rto-rpo.json (no static echo — every
# value is measured live) and a gate summary under tests/kpi/results/gates/.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

ADMIN_KEY="${VNG_ADMIN_API_KEY:-secret}"
TOKEN="${VNG_CLUSTER_TOKEN:-e5-cluster-token}"
ROWS="${VNG_E5_ROWS:-50}"
RTO_BUDGET_SECS="${VNG_E5_RTO_BUDGET:-30}"
DB="e5"

P1=8101; P2=8102; P3=8103
declare -A PORT=( [node-1]=$P1 [node-2]=$P2 [node-3]=$P3 )
declare -A PID
BASE() { echo "http://127.0.0.1:${PORT[$1]}"; }

# Locate the release server binary (KPI requires release for realistic timing).
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

AUTH=( -H "x-vng-admin-key: ${ADMIN_KEY}" -H "x-vng-operator-id: admin" -H "x-vng-database: ${DB}" -H "content-type: application/json" )

cleanup() {
  for n in node-1 node-2 node-3; do
    [[ -n "${PID[$n]:-}" ]] && kill "${PID[$n]}" 2>/dev/null || true
  done
}
trap cleanup EXIT

start_node() {
  local node="$1" port="$2" peers="$3"
  local dd; dd="$(mktemp -d "/tmp/vng-e5-${node}-XXXX")"
  VNG_ADMIN_API_KEY="$ADMIN_KEY" \
  VNG_NATIVE_LISTENER_ENABLED=false \
  VNG_NODE_ID="$node" \
  VNG_HTTP_BIND="127.0.0.1:${port}" \
  VNG_CLUSTER_MODE="cluster" \
  VNG_RAFT_PEERS="$peers" \
  VNG_CLUSTER_TOKEN="$TOKEN" \
  VNG_DATA_DIR="$dd" \
    "$SERVER_BIN" >"/tmp/vng-e5-${node}.log" 2>&1 &
  PID[$node]=$!
}

role_of() { # node -> role string (leader/follower/candidate) or empty
  curl -s --max-time 2 "${AUTH[@]}" "$(BASE "$1")/api/v1/cluster/raft/status" 2>/dev/null \
    | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin); print(d.get("raft",{}).get("role",""))
except Exception:
    print("")' 2>/dev/null
}

leader_node() { # echo the node-id currently reporting leader, or empty
  for n in node-1 node-2 node-3; do
    [[ -n "${PID[$n]:-}" ]] || continue
    if [[ "$(role_of "$n")" == "leader" ]]; then echo "$n"; return; fi
  done
  echo ""
}

row_count() { # node -> integer rows in table t (counts the rows array)
  curl -s --max-time 5 "${AUTH[@]}" -X POST "$(BASE "$1")/api/v1/sql/execute" \
    -d '{"sql_batch":"SELECT id FROM t"}' 2>/dev/null \
    | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin); r=d.get("rows"); print(len(r) if isinstance(r,list) else 0)
except Exception:
    print(0)' 2>/dev/null
}

echo "=== E-5 failover RTO/RPO harness (3-node Raft) ==="
start_node node-1 "$P1" "$(BASE node-2),$(BASE node-3)"
start_node node-2 "$P2" "$(BASE node-1),$(BASE node-3)"
start_node node-3 "$P3" "$(BASE node-1),$(BASE node-2)"

# Wait for all health endpoints.
for n in node-1 node-2 node-3; do
  ready=0
  for _ in $(seq 1 40); do
    if curl -sf --max-time 2 "$(BASE "$n")/health" >/dev/null 2>&1; then ready=1; break; fi
    sleep 0.5
  done
  [[ "$ready" == 1 ]] || { echo "node $n failed to start"; exit 1; }
done
echo "all 3 nodes healthy"

# Wait for initial leader election.
LEADER=""
for _ in $(seq 1 60); do
  LEADER="$(leader_node)"
  [[ -n "$LEADER" ]] && break
  sleep 0.5
done
[[ -n "$LEADER" ]] || { echo "no leader elected"; exit 1; }
echo "initial leader: ${LEADER} ($(BASE "$LEADER"))"

# Let heartbeats establish follower next_index before issuing linearisable writes.
sleep 3

# Create table + commit N rows through the leader (linearisable writes wait for quorum).
curl -s "${AUTH[@]}" -X POST "$(BASE "$LEADER")/api/v1/sql/execute" \
  -d '{"sql_batch":"CREATE TABLE t (id INT PRIMARY KEY, v TEXT)"}' >/dev/null
for i in $(seq 1 "$ROWS"); do
  curl -s "${AUTH[@]}" -X POST "$(BASE "$LEADER")/api/v1/sql/execute" \
    -d "{\"sql_batch\":\"INSERT INTO t (id, v) VALUES (${i}, 'r${i}')\"}" >/dev/null
done
# Re-read the committed count (retry briefly to let the apply loop settle).
COMMITTED=0
for _ in $(seq 1 10); do
  COMMITTED="$(row_count "$LEADER")"
  [[ "${COMMITTED:-0}" -ge "$ROWS" ]] && break
  sleep 0.5
done
echo "committed rows on leader: ${COMMITTED} (target ${ROWS})"

# Inject leader failure.
OLD_LEADER="$LEADER"
echo "killing leader ${OLD_LEADER} (pid ${PID[$OLD_LEADER]})…"
kill "${PID[$OLD_LEADER]}" 2>/dev/null || true
PID[$OLD_LEADER]=""
T0=$(python3 -c 'import time; print(time.time())')

# Measure RTO: time until a surviving node becomes leader.
NEW_LEADER=""
RTO="-1"
for _ in $(seq 1 $(( RTO_BUDGET_SECS * 4 )) ); do
  for n in node-1 node-2 node-3; do
    [[ -n "${PID[$n]:-}" ]] || continue
    if [[ "$(role_of "$n")" == "leader" ]]; then NEW_LEADER="$n"; break; fi
  done
  if [[ -n "$NEW_LEADER" ]]; then
    RTO=$(python3 -c "import time; print(round(time.time()-${T0},3))")
    break
  fi
  sleep 0.25
done

# Measure RPO: rows that survive on the new leader.
SURVIVED=0
if [[ -n "$NEW_LEADER" ]]; then
  # Allow a brief moment for the new leader to apply any committed tail.
  sleep 1
  SURVIVED="$(row_count "$NEW_LEADER")"
fi
LOST=$(( COMMITTED - SURVIVED ))
[[ "$LOST" -lt 0 ]] && LOST=0

# Derive status.
STATUS="failed"
RTO_OK=false
RPO_OK=false
if [[ -n "$NEW_LEADER" ]] && python3 -c "import sys; sys.exit(0 if ${RTO} >=0 and ${RTO} <= ${RTO_BUDGET_SECS} else 1)"; then RTO_OK=true; fi
if [[ "$LOST" -eq 0 && "$COMMITTED" -gt 0 ]]; then RPO_OK=true; fi
[[ "$RTO_OK" == true && "$RPO_OK" == true ]] && STATUS="passed"

GATE_DIR="tests/kpi/results"
mkdir -p "$GATE_DIR/e" "$GATE_DIR/gates"
TS="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
ART="$GATE_DIR/e/e5-failover-rto-rpo.json"
cat > "$ART" <<JSON
{
  "task": "E-5",
  "name": "failover-rto-rpo",
  "status": "${STATUS}",
  "nodes": 3,
  "old_leader": "${OLD_LEADER}",
  "new_leader": "${NEW_LEADER:-none}",
  "committed_rows": ${COMMITTED:-0},
  "survived_rows": ${SURVIVED:-0},
  "rows_lost": ${LOST},
  "rto_seconds": ${RTO},
  "rto_budget_seconds": ${RTO_BUDGET_SECS},
  "rto_within_budget": ${RTO_OK},
  "rpo_zero": ${RPO_OK},
  "write_path": "linearisable-quorum",
  "timestamp_utc": "${TS}"
}
JSON

cp "$ART" "$GATE_DIR/gates/e5-failover-gate.json"
echo ""
echo "=== E-5 result: ${STATUS} ==="
echo "  new leader : ${NEW_LEADER:-none}"
echo "  RTO        : ${RTO}s (budget ${RTO_BUDGET_SECS}s, ok=${RTO_OK})"
echo "  RPO        : committed=${COMMITTED} survived=${SURVIVED} lost=${LOST} (zero=${RPO_OK})"
echo "  artifact   : ${ART}"
cat "$ART"
[[ "$STATUS" == "passed" ]]
