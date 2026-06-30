#!/usr/bin/env bash
# D-1 smoke: connect to the VoltNueronGrid Postgres wire front-end with psql and
# run DDL + INSERT + SELECT. Proves BI/JDBC/ODBC Postgres-driver compatibility.
#
# Usage:
#   VNG_ADMIN_API_KEY=secret VNG_PGWIRE_ENABLED=true VNG_PGWIRE_PORT=5455 \
#     cargo run -p voltnuerongridd        # in another shell
#   ./tests/kpi/scripts/run-d1-pgwire-smoke.sh 127.0.0.1 5455 secret
set -euo pipefail

HOST="${1:-127.0.0.1}"
PORT="${2:-5455}"
KEY="${3:-secret}"
PSQL="${PSQL:-psql}"

export PGPASSWORD="$KEY"

echo "== D-1 Postgres wire smoke: $HOST:$PORT =="
"$PSQL" -h "$HOST" -p "$PORT" -U admin -d smokedb -w <<'SQL'
CREATE TABLE d1_smoke (id INT PRIMARY KEY, name TEXT);
INSERT INTO d1_smoke (id, name) VALUES (1, 'alice');
INSERT INTO d1_smoke (id, name) VALUES (2, 'bob');
SELECT id, name FROM d1_smoke;
SQL

echo "== D-1 smoke PASSED (psql connected, ran DDL/INSERT/SELECT) =="
