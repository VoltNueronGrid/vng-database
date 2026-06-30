#!/usr/bin/env bash
# E-8 · Deployment parity validation.
#
# Runs an identical smoke suite against the local single-node and local
# multi-node (3-node Raft) topologies, proving the developer topology is
# exercised with the same checks used for the cloud overlays. Emits
# tests/kpi/results/e/e8-parity-validation.json + a gate summary, with any
# cloud-only capability recorded as a flagged gap (see
# docs/deployment-parity-matrix.md) rather than silently skipped.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

ADMIN_KEY="${VNG_ADMIN_API_KEY:-secret}"
TOKEN="${VNG_CLUSTER_TOKEN:-e8-cluster-token}"
DB="e8"
AUTH=( -H "x-vng-admin-key: ${ADMIN_KEY}" -H "x-vng-operator-id: admin" -H "x-vng-database: ${DB}" -H "content-type: application/json" )

SERVER_BIN=""
for cand in "$REPO_ROOT/target/release/voltnuerongridd" "$HOME/installs/target/release/voltnuerongridd"; do
  [[ -x "$cand" ]] && { SERVER_BIN="$cand"; break; }
done
if [[ -z "$SERVER_BIN" ]]; then
  cargo build --release -p voltnuerongridd >/dev/null 2>&1
  for cand in "$REPO_ROOT/target/release/voltnuerongridd" "$HOME/installs/target/release/voltnuerongridd"; do
    [[ -x "$cand" ]] && { SERVER_BIN="$cand"; break; }
  done
fi
[[ -n "$SERVER_BIN" ]] || { echo "voltnuerongridd release binary not found"; exit 1; }

PIDS=()
cleanup() { for p in "${PIDS[@]:-}"; do [[ -n "$p" ]] && kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT

wait_health() { # base_url
  for _ in $(seq 1 40); do
    curl -sf --max-time 2 "$1/health" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  return 1
}

# Smoke suite — returns "health,sql,rbac,raft" pass counts via globals.
SMOKE_HEALTH=false; SMOKE_SQL=false; SMOKE_RBAC=false; SMOKE_RAFT=false
run_smoke() { # base_url
  local base="$1"
  SMOKE_HEALTH=false; SMOKE_SQL=false; SMOKE_RBAC=false; SMOKE_RAFT=false

  curl -sf --max-time 3 "$base/health" >/dev/null 2>&1 && SMOKE_HEALTH=true

  curl -s "${AUTH[@]}" -X POST "$base/api/v1/sql/execute" \
    -d '{"sql_batch":"CREATE TABLE smk (id INT PRIMARY KEY, v TEXT)"}' >/dev/null
  curl -s "${AUTH[@]}" -X POST "$base/api/v1/sql/execute" \
    -d "{\"sql_batch\":\"INSERT INTO smk (id, v) VALUES (1, 'x')\"}" >/dev/null
  local n=0
  for _ in $(seq 1 12); do
    n="$(curl -s --max-time 5 "${AUTH[@]}" -X POST "$base/api/v1/sql/execute" \
          -d '{"sql_batch":"SELECT id FROM smk"}' 2>/dev/null \
        | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin); r=d.get("rows"); print(len(r) if isinstance(r,list) else 0)
except Exception:
    print(0)' 2>/dev/null)"
    [[ "${n:-0}" -ge 1 ]] && break
    sleep 0.5
  done
  [[ "${n:-0}" -ge 1 ]] && SMOKE_SQL=true

  # RBAC — unauthenticated control-plane call must be rejected (401/403).
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 \
          -X POST "$base/api/v1/sql/execute" -H 'content-type: application/json' \
          -d '{"sql_batch":"SELECT 1"}' 2>/dev/null)"
  [[ "$code" == "401" || "$code" == "403" ]] && SMOKE_RBAC=true

  curl -sf --max-time 3 "${AUTH[@]}" "$base/api/v1/cluster/raft/status" >/dev/null 2>&1 && SMOKE_RAFT=true
}

json_topology() { # name base_url -> JSON object string
  local name="$1"
  local all=true
  for v in "$SMOKE_HEALTH" "$SMOKE_SQL" "$SMOKE_RBAC" "$SMOKE_RAFT"; do
    [[ "$v" == true ]] || all=false
  done
  cat <<JSON
    {
      "topology": "${name}",
      "health": ${SMOKE_HEALTH},
      "sql_roundtrip": ${SMOKE_SQL},
      "rbac_enforced": ${SMOKE_RBAC},
      "raft_reachable": ${SMOKE_RAFT},
      "all_passed": ${all}
    }
JSON
}

echo "=== E-8 deployment parity validation ==="

# ── Topology 1: local single-node ──
SN_DIR="$(mktemp -d /tmp/vng-e8-single-XXXX)"
VNG_ADMIN_API_KEY="$ADMIN_KEY" VNG_NATIVE_LISTENER_ENABLED=false \
  VNG_NODE_ID=node-1 VNG_HTTP_BIND=127.0.0.1:8121 VNG_DATA_DIR="$SN_DIR" \
  "$SERVER_BIN" >/tmp/vng-e8-single.log 2>&1 &
PIDS+=($!)
wait_health "http://127.0.0.1:8121" || { echo "single-node failed to start"; exit 1; }
run_smoke "http://127.0.0.1:8121"
SINGLE_JSON="$(json_topology "local-single-node")"
echo "single-node: health=$SMOKE_HEALTH sql=$SMOKE_SQL rbac=$SMOKE_RBAC raft=$SMOKE_RAFT"
SINGLE_ALL_HEALTH=$SMOKE_HEALTH; SINGLE_ALL_SQL=$SMOKE_SQL; SINGLE_ALL_RBAC=$SMOKE_RBAC; SINGLE_ALL_RAFT=$SMOKE_RAFT

# ── Topology 2: local multi-node (3 nodes) ──
declare -A MP=( [node-1]=8131 [node-2]=8132 [node-3]=8133 )
for n in node-1 node-2 node-3; do
  p=${MP[$n]}
  peers=""
  for m in node-1 node-2 node-3; do
    [[ "$m" == "$n" ]] && continue
    peers+="http://127.0.0.1:${MP[$m]},"
  done
  peers="${peers%,}"
  dd="$(mktemp -d "/tmp/vng-e8-${n}-XXXX")"
  VNG_ADMIN_API_KEY="$ADMIN_KEY" VNG_NATIVE_LISTENER_ENABLED=false \
    VNG_NODE_ID="$n" VNG_HTTP_BIND="127.0.0.1:${p}" VNG_CLUSTER_MODE=cluster \
    VNG_RAFT_PEERS="$peers" VNG_CLUSTER_TOKEN="$TOKEN" VNG_DATA_DIR="$dd" \
    "$SERVER_BIN" >"/tmp/vng-e8-${n}.log" 2>&1 &
  PIDS+=($!)
done
for n in node-1 node-2 node-3; do
  wait_health "http://127.0.0.1:${MP[$n]}" || { echo "multi-node $n failed"; exit 1; }
done
# Wait for a leader.
LEADER_PORT=""
for _ in $(seq 1 60); do
  for n in node-1 node-2 node-3; do
    r="$(curl -s --max-time 2 "${AUTH[@]}" "http://127.0.0.1:${MP[$n]}/api/v1/cluster/raft/status" 2>/dev/null \
        | python3 -c 'import sys,json
try: print(json.load(sys.stdin).get("raft",{}).get("role",""))
except Exception: print("")' 2>/dev/null)"
    [[ "$r" == "leader" ]] && { LEADER_PORT="${MP[$n]}"; break; }
  done
  [[ -n "$LEADER_PORT" ]] && break
  sleep 0.5
done
MULTI_LEADER_ELECTED=false; [[ -n "$LEADER_PORT" ]] && MULTI_LEADER_ELECTED=true
sleep 3
run_smoke "http://127.0.0.1:${LEADER_PORT:-${MP[node-1]}}"
# Multi-node additionally requires a leader for full parity.
[[ "$MULTI_LEADER_ELECTED" == true ]] || SMOKE_RAFT=false
MULTI_JSON="$(json_topology "local-multi-node")"
echo "multi-node: health=$SMOKE_HEALTH sql=$SMOKE_SQL rbac=$SMOKE_RBAC raft=$SMOKE_RAFT leader=$MULTI_LEADER_ELECTED"

# ── Aggregate ──
STATUS="passed"
for v in "$SINGLE_ALL_HEALTH" "$SINGLE_ALL_SQL" "$SINGLE_ALL_RBAC" "$SINGLE_ALL_RAFT" \
         "$SMOKE_HEALTH" "$SMOKE_SQL" "$SMOKE_RBAC" "$SMOKE_RAFT"; do
  [[ "$v" == true ]] || STATUS="failed"
done

mkdir -p tests/kpi/results/e tests/kpi/results/gates
TS="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
ART="tests/kpi/results/e/e8-parity-validation.json"
cat > "$ART" <<JSON
{
  "task": "E-8",
  "name": "deployment-parity-validation",
  "status": "${STATUS}",
  "matrix_doc": "docs/deployment-parity-matrix.md",
  "smoke_suite": ["health", "sql_roundtrip", "rbac_enforced", "raft_reachable"],
  "topologies": [
${SINGLE_JSON},
${MULTI_JSON}
  ],
  "cloud_only_gaps": [
    "object_storage_cold_tier",
    "managed_autoscaling",
    "multi_az_region_replication",
    "provider_managed_kms_and_tls",
    "cluster_scale_kpi_targets"
  ],
  "timestamp_utc": "${TS}"
}
JSON
cp "$ART" tests/kpi/results/gates/e8-parity-gate.json

echo ""
echo "=== E-8 parity validation: ${STATUS} → ${ART} ==="
cat "$ART"
[[ "$STATUS" == "passed" ]]
