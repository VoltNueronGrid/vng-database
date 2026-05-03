#!/usr/bin/env bash
set -euo pipefail

# Usage: tools/run_sql_example.sh samples/sql/feature-examples.sql
FILE=${1:-samples/sql/feature-examples.sql}
ADMIN_KEY=${VNG_ADMIN_API_KEY:-dev-admin-key-changeme}
URL=${VNG_SERVER_URL:-http://127.0.0.1:8080/api/v1/sql/execute}

if [ ! -f "$FILE" ]; then
  echo "SQL file not found: $FILE" >&2
  exit 1
fi

# Build a JSON payload safely using python to ensure proper escaping
# Split into statements by semicolon and execute sequentially so we can observe per-statement results.
export __SQL_EXAMPLE_FILE="$FILE"
export __SQL_EXAMPLE_URL="$URL"
export __SQL_EXAMPLE_ADMIN="$ADMIN_KEY"

python3 - <<'PY'
import os, json
from urllib import request
path = os.environ['__SQL_EXAMPLE_FILE']
url = os.environ['__SQL_EXAMPLE_URL']
admin = os.environ['__SQL_EXAMPLE_ADMIN']
sql = open(path).read()
lines = sql.splitlines()
stmts = []
current = []
keywords = ('create','insert','with','merge','select','drop')
for ln in lines:
  s = ln.strip()
  if not s or s.startswith('--'):
    continue
  if not current:
    low = s.lower()
    if not any(low.startswith(k) for k in keywords):
      continue
  current.append(ln)
  if s.endswith(';'):
    stmt = '\n'.join(current).strip()
    stmts.append(stmt.rstrip(';').strip())
    current = []
if current:
  stmts.append('\n'.join(current).strip())
for i,s in enumerate(stmts, start=1):
  payload = json.dumps({"sql_batch": s + (';' if not s.endswith(';') else '')}).encode('utf-8')
  req = request.Request(url, data=payload, headers={
    'Content-Type': 'application/json',
    'x-vng-admin-key': admin,
    'x-vng-operator-id': 'admin'
  })
  try:
    with request.urlopen(req, timeout=30) as resp:
      body = resp.read().decode('utf-8')
      print('\n--- Statement %d ---' % i)
      print(s)
      print('Response:')
      print(body)
  except Exception as e:
    import urllib.error
    print('\n--- Statement %d (error) ---' % i)
    print(s)
    if isinstance(e, urllib.error.HTTPError):
      try:
        print('HTTP Error:', e.code, e.reason)
        print('Body:')
        print(e.read().decode('utf-8'))
      except Exception:
        print('HTTP Error, unable to read body')
    else:
      print('Error:', e)
    # continue to next statement instead of aborting
    continue
PY
