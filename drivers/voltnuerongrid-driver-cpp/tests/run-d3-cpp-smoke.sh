#!/usr/bin/env bash
# D-3 smoke: build + run the C++ wrapper smoke (offline) and, when a server is
# running, the live sample against the real C cdylib.
#
# Offline smoke (no server, no cmake required):
#   ./tests/run-d3-cpp-smoke.sh
#
# Live sample (requires a running server + built C cdylib):
#   cargo build -p vng-driver-c
#   VNG_ADMIN_API_KEY=secret cargo run -p voltnuerongridd   # another shell
#   ./tests/run-d3-cpp-smoke.sh --live 127.0.0.1 8080 secret
set -euo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
C_HEADER_DIR="$HERE/../voltnuerongrid-driver-c"
CXX="${CXX:-c++}"

echo "== D-3 C++ wrapper offline smoke =="
"$CXX" -std=c++17 -Wall -Wextra \
    -I "$HERE/include" -I "$C_HEADER_DIR" \
    "$HERE/tests/smoke.cpp" -o /tmp/vng_cpp_smoke
/tmp/vng_cpp_smoke

if [[ "${1:-}" == "--live" ]]; then
    HOST="${2:-127.0.0.1}"; PORT="${3:-8080}"; KEY="${4:-secret}"
    # Locate the built C cdylib.
    LIBDIR="$HERE/../../target/debug"
    echo "== D-3 C++ live sample against $HOST:$PORT (lib: $LIBDIR) =="
    "$CXX" -std=c++17 -I "$HERE/include" -I "$C_HEADER_DIR" \
        "$HERE/examples/sample.cpp" \
        -L "$LIBDIR" -lvoltnuerongrid_driver \
        -Wl,-rpath,"$LIBDIR" -o /tmp/vng_cpp_sample
    /tmp/vng_cpp_sample "$HOST" "$PORT" "$KEY"
fi

echo "== D-3 smoke PASSED =="
