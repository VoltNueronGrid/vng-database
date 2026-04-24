#!/usr/bin/env bash
# deploy/local/install.sh — VoltNueronGrid local installation and startup script
# Document: S11-004
# Usage: bash deploy/local/install.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
ENV_FILE="${REPO_ROOT}/.env"
ENV_EXAMPLE="${SCRIPT_DIR}/vng.env.example"
BINARY="${REPO_ROOT}/target/release/voltnuerongridd"
PID_FILE="${VNG_PID_FILE:-/tmp/voltnuerongridd.pid}"

# ─── Colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Colour

info()    { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; }
die()     { error "$*"; exit 1; }

# ─── Step 1: Check prerequisites ──────────────────────────────────────────────
info "Checking prerequisites..."

check_cmd() {
    local cmd="$1"
    local min_label="$2"
    if ! command -v "${cmd}" &>/dev/null; then
        die "'${cmd}' not found. ${min_label}"
    fi
    info "  ${cmd}: $(${cmd} --version 2>&1 | head -1)"
}

check_cmd rustc   "Install Rust 1.75+ via https://rustup.rs/"
check_cmd cargo   "Install Rust 1.75+ via https://rustup.rs/"
check_cmd node    "Install Node.js 20 LTS via https://nodejs.org/"
check_cmd python3 "Install Python 3.11+ via https://www.python.org/"
check_cmd curl    "Install curl (typically via OS package manager)"

# Verify Rust version
RUST_VERSION=$(rustc --version | awk '{print $2}')
RUST_MAJOR=$(echo "${RUST_VERSION}" | cut -d. -f1)
RUST_MINOR=$(echo "${RUST_VERSION}" | cut -d. -f2)
if [[ "${RUST_MAJOR}" -lt 1 ]] || ( [[ "${RUST_MAJOR}" -eq 1 ]] && [[ "${RUST_MINOR}" -lt 75 ]] ); then
    die "Rust 1.75+ required (found ${RUST_VERSION}). Run: rustup update"
fi

# Verify Node version
NODE_MAJOR=$(node --version | tr -d 'v' | cut -d. -f1)
if [[ "${NODE_MAJOR}" -lt 20 ]]; then
    warn "Node.js 20+ recommended (found $(node --version)). Some features may not work."
fi

info "Prerequisites OK."

# ─── Step 2: Environment file ─────────────────────────────────────────────────
if [[ ! -f "${ENV_FILE}" ]]; then
    info "Creating .env from template..."
    cp "${ENV_EXAMPLE}" "${ENV_FILE}"
    warn ".env created from template. Edit ${ENV_FILE} to set VNG_ADMIN_API_KEY before production use."
fi

# Source .env (ignore errors for unset vars)
set +u
# shellcheck disable=SC1090
source "${ENV_FILE}" 2>/dev/null || true
set -u

# ─── Step 3: Build the binary ─────────────────────────────────────────────────
info "Building voltnuerongridd (release)..."
cd "${REPO_ROOT}"
cargo build --release -p voltnuerongridd 2>&1

if [[ ! -f "${BINARY}" ]]; then
    die "Build succeeded but binary not found at ${BINARY}"
fi
info "Build complete: ${BINARY}"

# ─── Step 4: Stop any running instance ───────────────────────────────────────
if [[ -f "${PID_FILE}" ]]; then
    OLD_PID=$(cat "${PID_FILE}" 2>/dev/null || true)
    if [[ -n "${OLD_PID}" ]] && kill -0 "${OLD_PID}" 2>/dev/null; then
        info "Stopping existing voltnuerongridd (PID ${OLD_PID})..."
        kill "${OLD_PID}"
        sleep 1
    fi
    rm -f "${PID_FILE}"
fi

# ─── Step 5: Start the server ────────────────────────────────────────────────
HTTP_PORT="${VNG_HTTP_PORT:-8080}"
LOG_LEVEL="${VNG_LOG_LEVEL:-info}"

info "Starting voltnuerongridd on port ${HTTP_PORT}..."

VNG_HTTP_PORT="${HTTP_PORT}" \
VNG_LOG_LEVEL="${LOG_LEVEL}" \
    "${BINARY}" &

SERVER_PID=$!
echo "${SERVER_PID}" > "${PID_FILE}"
info "Server started (PID ${SERVER_PID}, PID file: ${PID_FILE})"

# Wait for server to be ready
HEALTH_URL="http://localhost:${HTTP_PORT}/health"
MAX_RETRIES=15
RETRY_DELAY=1

info "Waiting for health check at ${HEALTH_URL}..."
for i in $(seq 1 ${MAX_RETRIES}); do
    if curl -sf "${HEALTH_URL}" -o /dev/null 2>/dev/null; then
        info "Server is healthy."
        break
    fi
    if [[ "${i}" -eq "${MAX_RETRIES}" ]]; then
        die "Server did not become healthy after ${MAX_RETRIES} attempts. Check logs."
    fi
    sleep "${RETRY_DELAY}"
done

# ─── Step 6: Print summary ────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}==========================================${NC}"
echo -e "${GREEN}  VoltNueronGrid is running!${NC}"
echo -e "${GREEN}==========================================${NC}"
echo ""
echo "  Health check URL : ${HEALTH_URL}"
echo "  SQL API          : http://localhost:${HTTP_PORT}/api/v1/sql/execute"
echo "  Schema registry  : http://localhost:${HTTP_PORT}/api/v1/ingest/schema/registry"
echo "  PID file         : ${PID_FILE}"
echo ""
echo "  To stop the server:"
echo "    kill \$(cat ${PID_FILE})"
echo ""
echo "  To view logs, restart without background (&):"
echo "    ${BINARY}"
echo ""
