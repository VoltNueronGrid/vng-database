#!/bin/bash

set -euo pipefail

MODE="${1:-both}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../polap-db" && pwd)"
UI_DIR="${REPO_DIR}/ui/voltnuerongrid-studio"

if [[ ! -d "${REPO_DIR}" ]]; then
  echo "Repository not found at ${REPO_DIR}" >&2
  exit 1
fi

if [[ ! -d "${UI_DIR}" ]]; then
  echo "UI directory not found at ${UI_DIR}" >&2
  exit 1
fi

usage() {
  cat <<'EOF'
Usage: start-vng-local.sh [both|onlydb|onlyui]

Options:
  both    Start the database and UI in separate Terminal windows (default)
  onlydb  Start only the database
  onlyui  Start only the UI
EOF
}

open_terminal_window() {
  local command="$1"

  osascript <<EOF
tell application "Terminal"
  activate
  do script ${command@Q}
end tell
EOF
}

start_db() {
  local db_command="cd ${REPO_DIR@Q} && export VNG_ADMIN_API_KEY=secret && cargo run -p voltnuerongridd"
  open_terminal_window "${db_command}"
}

start_ui() {
  local ui_command="cd ${UI_DIR@Q} && export VITE_VNG_DEV_URL=http://127.0.0.1:8080 && npm run dev"
  open_terminal_window "${ui_command}"
}

case "${MODE}" in
  both)
    start_db
    start_ui
    ;;
  onlydb)
    start_db
    ;;
  onlyui)
    start_ui
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage >&2
    exit 1
    ;;
esac