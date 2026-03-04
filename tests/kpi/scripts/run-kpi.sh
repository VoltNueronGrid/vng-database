#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${1:-}"
SQL_URL="${2:-}"
OUTPUT_DIR="${3:-}"
TARGETS_FILE="${4:-}"
AUTH_MODE="${5:-none}"
AUTH_TOKEN="${6:-}"
API_KEY_HEADER_NAME="${7:-X-API-Key}"

if [[ -z "${BASE_URL}" || -z "${SQL_URL}" || -z "${OUTPUT_DIR}" ]]; then
  echo "Usage: ./tests/kpi/scripts/run-kpi.sh <base_url> <sql_url> <output_dir> [targets_file] [auth_mode] [auth_token] [api_key_header_name]"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCENARIOS_DIR="$(cd "${SCRIPT_DIR}/../scenarios" && pwd)"
if [[ -z "${TARGETS_FILE}" ]]; then
  TARGETS_FILE="${SCRIPT_DIR}/../config/targets.yaml"
fi

mkdir -p "${OUTPUT_DIR}"

extract_yaml_number() {
  local key="$1"
  local file="$2"
  awk -F ':' -v key="${key}" '
    $1 ~ "^[[:space:]]*"key"[[:space:]]*$" {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2);
      print $2;
      exit
    }
  ' "${file}"
}

build_auth_args() {
  AUTH_ARGS=()
  if [[ "${AUTH_MODE}" == "bearer" && -n "${AUTH_TOKEN}" ]]; then
    AUTH_ARGS=(-H "Authorization: Bearer ${AUTH_TOKEN}")
  elif [[ "${AUTH_MODE}" == "apiKey" && -n "${AUTH_TOKEN}" ]]; then
    AUTH_ARGS=(-H "${API_KEY_HEADER_NAME}: ${AUTH_TOKEN}")
  fi
}

percentile() {
  local percentile="$1"
  shift
  local values=("$@")
  local count="${#values[@]}"
  if [[ "${count}" -eq 0 ]]; then
    echo "0"
    return
  fi
  IFS=$'\n' read -r -d '' -a sorted < <(printf '%s\n' "${values[@]}" | sort -n && printf '\0')
  local index
  index=$(awk -v p="${percentile}" -v c="${count}" 'BEGIN { i = int((p * c) + 0.999999) - 1; if (i < 0) i = 0; if (i >= c) i = c - 1; print i }')
  echo "${sorted[${index}]}"
}

timed_get() {
  local url="$1"
  local start_ms end_ms elapsed_ms
  start_ms=$(date +%s%3N)
  local body
  body=$(curl -sS --max-time 10 "${AUTH_ARGS[@]}" "${url}")
  end_ms=$(date +%s%3N)
  elapsed_ms=$((end_ms - start_ms))
  printf '%s\n%s\n' "${elapsed_ms}" "${body}"
}

timed_post() {
  local url="$1"
  local payload="$2"
  local start_ms end_ms elapsed_ms
  start_ms=$(date +%s%3N)
  local body
  body=$(curl -sS --max-time 10 "${AUTH_ARGS[@]}" -H "Content-Type: application/json" -X POST "${url}" -d "${payload}")
  end_ms=$(date +%s%3N)
  elapsed_ms=$((end_ms - start_ms))
  printf '%s\n%s\n' "${elapsed_ms}" "${body}"
}

build_auth_args
oltp_p95_target="$(extract_yaml_number "p95_ms" "${TARGETS_FILE}")"
oltp_p99_target="$(awk '/oltp_latency:/{f=1} f&&/p99_ms:/{print $2; exit}' "${TARGETS_FILE}")"
olap_p95_target="$(awk '/olap_latency:/{f=1} f&&/p95_ms:/{print $2; exit}' "${TARGETS_FILE}")"
olap_p99_target="$(awk '/olap_latency:/{f=1} f&&/p99_ms:/{print $2; exit}' "${TARGETS_FILE}")"
htap_read_qps_target="$(awk '/htap_mixed_throughput:/{f=1} f&&/read_qps_min:/{print $2; exit}' "${TARGETS_FILE}")"
htap_write_tps_target="$(awk '/htap_mixed_throughput:/{f=1} f&&/write_tps_min:/{print $2; exit}' "${TARGETS_FILE}")"
failover_rto_target="$(awk '/failover:/{f=1} f&&/rto_sec_max:/{print $2; exit}' "${TARGETS_FILE}")"
failover_rpo_target="$(awk '/failover:/{f=1} f&&/rpo_committed_data_loss:/{print $2; exit}' "${TARGETS_FILE}")"

for scenario in "${SCENARIOS_DIR}"/*.yaml; do
  scenario_name="$(basename "${scenario}" .yaml)"
  result_file="${OUTPUT_DIR}/${scenario_name}-result.json"

  mapfile -t health_probe < <(timed_get "${BASE_URL}/health")
  health_elapsed_ms="${health_probe[0]}"
  health_payload="${health_probe[1]}"

  status="passed"
  metrics_json='{}'

  case "${scenario_name}" in
    oltp-latency)
      latencies=()
      for i in $(seq 1 30); do
        payload='{"statements":["BEGIN","INSERT INTO kpi_probe(id,v) VALUES ('"${i}"','\''ok'\'')","COMMIT"]}'
        mapfile -t tx < <(timed_post "${SQL_URL}/api/v1/sql/transaction" "${payload}")
        latencies+=("${tx[0]}")
      done
      p95="$(percentile 0.95 "${latencies[@]}")"
      p99="$(percentile 0.99 "${latencies[@]}")"
      threshold_p95="${oltp_p95_target:-20}"
      threshold_p99="${oltp_p99_target:-60}"
      if (( p95 > threshold_p95 || p99 > threshold_p99 )); then
        status="failed"
      fi
      metrics_json='{"sample_count":'"${#latencies[@]}"',"p95_latency_ms":'"${p95}"',"p99_latency_ms":'"${p99}"',"threshold_p95_ms":'"${threshold_p95}"',"threshold_p99_ms":'"${threshold_p99}"'}'
      ;;
    olap-latency)
      latencies=()
      for _ in $(seq 1 20); do
        payload='{"query":"SELECT SUM(v) FROM kpi_probe WHERE ts > now() - interval '\''1 hour'\''","max_rows":1000}'
        mapfile -t olap < <(timed_post "${BASE_URL}/api/v1/olap/query" "${payload}")
        latencies+=("${olap[0]}")
      done
      p95="$(percentile 0.95 "${latencies[@]}")"
      p99="$(percentile 0.99 "${latencies[@]}")"
      threshold_p95="${olap_p95_target:-800}"
      threshold_p99="${olap_p99_target:-1500}"
      if (( p95 > threshold_p95 || p99 > threshold_p99 )); then
        status="failed"
      fi
      metrics_json='{"sample_count":'"${#latencies[@]}"',"p95_latency_ms":'"${p95}"',"p99_latency_ms":'"${p99}"',"threshold_p95_ms":'"${threshold_p95}"',"threshold_p99_ms":'"${threshold_p99}"'}'
      ;;
    htap-mixed-throughput)
      duration_seconds=10
      end_epoch=$(( $(date +%s) + duration_seconds ))
      read_ops=0
      write_ops=0
      while (( $(date +%s) < end_epoch )); do
        tx_payload='{"statements":["BEGIN","UPDATE kpi_probe SET v='\''mix'\'' WHERE id=1","COMMIT"]}'
        olap_payload='{"query":"SELECT COUNT(*) FROM kpi_probe","max_rows":10}'
        timed_post "${SQL_URL}/api/v1/sql/transaction" "${tx_payload}" >/dev/null
        timed_post "${BASE_URL}/api/v1/olap/query" "${olap_payload}" >/dev/null
        read_ops=$((read_ops + 1))
        write_ops=$((write_ops + 1))
      done
      read_qps="$(awk -v ops="${read_ops}" -v d="${duration_seconds}" 'BEGIN { printf "%.3f", ops / d }')"
      write_tps="$(awk -v ops="${write_ops}" -v d="${duration_seconds}" 'BEGIN { printf "%.3f", ops / d }')"
      read_too_low="$(awk -v t="${read_qps}" -v th="${htap_read_qps_target:-25000}" 'BEGIN { print (t < th) ? "1" : "0" }')"
      write_too_low="$(awk -v t="${write_tps}" -v th="${htap_write_tps_target:-10000}" 'BEGIN { print (t < th) ? "1" : "0" }')"
      if [[ "${read_too_low}" == "1" || "${write_too_low}" == "1" ]]; then
        status="failed"
      fi
      metrics_json='{"duration_seconds":'"${duration_seconds}"',"read_operations":'"${read_ops}"',"write_operations":'"${write_ops}"',"read_qps":'"${read_qps}"',"write_tps":'"${write_tps}"',"threshold_read_qps_min":'"${htap_read_qps_target:-25000}"',"threshold_write_tps_min":'"${htap_write_tps_target:-10000}"'}'
      ;;
    failover-rto-rpo)
      mapfile -t failover_probe < <(timed_get "${BASE_URL}/api/v1/failover/status")
      failover_payload="${failover_probe[1]}"
      reported_rto="$(printf '%s' "${failover_payload}" | sed -n 's/.*"rto_seconds_target":[[:space:]]*\([0-9][0-9]*\).*/\1/p')"
      reported_rpo="$(printf '%s' "${failover_payload}" | sed -n 's/.*"rpo_data_loss_rows_target":[[:space:]]*\([0-9][0-9]*\).*/\1/p')"
      reported_rto="${reported_rto:-99999}"
      reported_rpo="${reported_rpo:-99999}"
      threshold_rto="${failover_rto_target:-30}"
      threshold_rpo="${failover_rpo_target:-0}"
      if (( reported_rto > threshold_rto || reported_rpo > threshold_rpo )); then
        status="failed"
      fi
      metrics_json='{"reported_rto_seconds":'"${reported_rto}"',"reported_rpo_rows":'"${reported_rpo}"',"threshold_rto_seconds":'"${threshold_rto}"',"threshold_rpo_rows":'"${threshold_rpo}"'}'
      ;;
    *)
      status="failed"
      metrics_json='{"error":"unknown scenario"}'
      ;;
  esac

  timestamp_utc="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  printf '{\n  "scenario": "%s",\n  "base_url": "%s",\n  "sql_url": "%s",\n  "status": "%s",\n  "timestamp_utc": "%s",\n  "health": { "elapsed_ms": %s, "payload": %s },\n  "metrics": %s\n}\n' \
    "${scenario_name}" "${BASE_URL}" "${SQL_URL}" "${status}" "${timestamp_utc}" "${health_elapsed_ms}" "${health_payload}" "${metrics_json}" > "${result_file}"
  echo "Generated KPI result: ${result_file} (${status})"
done

echo "KPI harness run completed."
