#!/usr/bin/env node
'use strict';
const fs = require('fs');

// Update status_tracker.md
{
  const lines = fs.readFileSync('status_tracker.md','utf8').split('\n');
  let insertAt = -1;
  for(let i = 0; i < lines.length; i++) {
    if(lines[i].includes('9.2n Session 46')) { insertAt = i; break; }
  }
  console.log('Inserting before line:', insertAt + 1);
  const newLines = [
    '### 9.2p Session 48 Implementation Update (S3-WS1-24 + service endpoints)',
    '',
    '- **SQL (`voltnuerongrid-sql`)**: Added `has_in_subquery: bool` field to `SelectStatement` (ast.rs). Detects `IN (SELECT` and `IN(SELECT` via `up` buffer (S3-WS1-24). Updated two existing subquery planner tests to scalar subquery syntax to avoid conflict. Tests: `in_subquery_tests` module (3 tests). Total: **174 passed**.',
    '- **Exec (`voltnuerongrid-exec`)**: Added `InSubquery { input }` variant to `LogicalPlan` enum (planner.rs). Updated `primary_table()`, `has_aggregation()`, `estimate_cost()` (OLAP path, 0.6x row selectivity, +0.8 cost), and `plan_select()` converted Interval to `let after_interval` and added InSubquery outermost wrap. Tests: `planner_in_subquery_select_produces_in_subquery_node`, `cost_in_subquery_query_routes_to_olap`. Total: **72 passed**.',
    '- **Service (`voltnuerongridd`)**: Added `GET /api/v1/store/rows/count/distinct` (operator-auth, returns count of distinct values in MVCC store) and `GET /api/v1/store/rows/key/exists` (operator-auth, `?key=` param, returns bool exists). Tests: 4 new tests (`s11_ws1_24_*`). Total: **425 passed**.',
    '',
    '### 9.2o Session 47 Fix Log (S3-WS1-23 detection fix)',
    '',
    '- **SQL (`voltnuerongrid-sql`)**: Fixed missing `has_interval` detection block (S3-WS1-23) — added `if up.contains("INTERVAL")` detection after TRIM block. Fixed 2 failing tests. Total: **171 passed**.',
    '- **Exec (`voltnuerongrid-exec`)**: `Interval` node and `has_interval` plan wrap already existed from S47 partial apply — fixed by SQL detection fix. Fixed 2 failing tests. Total: **70 passed**.',
    '',
  ];
  lines.splice(insertAt, 0, ...newLines);
  fs.writeFileSync('status_tracker.md', lines.join('\n'), 'utf8');
  console.log('status_tracker.md updated');
}

// Update sprintwise tracker
{
  const lines = fs.readFileSync('status-tracker-sprintwise-v1.md','utf8').split('\n');
  let insertAt = -1;
  for(let i = 0; i < lines.length; i++) {
    if(lines[i].includes('Session 46 Implementation')) { insertAt = i; break; }
  }
  console.log('Inserting before sprintwise line:', insertAt + 1);
  const newLines = [
    '## Session 48 Implementation Log',
    '',
    '**Date:** 2026-04-07 (Sprint 9 continuation)',
    '**Test Baseline to New:** sql 171>174, exec 70>72, service 421>425 (+9 total)',
    '',
    '| Item | Crate | Change | Tests Added |',
    '|---|---|---|-|',
    '| `has_in_subquery: bool` field + detection | `voltnuerongrid-sql` | Detects `IN (SELECT` / `IN(SELECT` (S3-WS1-24); updated 2 existing planner tests to scalar subquery | 3 (`in_subquery_tests` module) |',
    '| `InSubquery { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.6x row selectivity, +0.8 cost | 2 |',
    '| `GET /api/v1/store/rows/count/distinct` | `voltnuerongridd` | Distinct value count across all MVCC rows (operator-auth) | 2 |',
    '| `GET /api/v1/store/rows/key/exists` | `voltnuerongridd` | Key existence check (`?key=` param) (operator-auth) | 2 |',
    '',
    '---',
    '',
    '## Session 47 Fix Log',
    '',
    '**Date:** 2026-04-07 (Sprint 9 continuation)',
    '**Fix:** Added missing INTERVAL detection block in ast.rs (S3-WS1-23). Fixed 2 failing SQL tests (169>171) and 2 failing exec tests (68>70).',
    '',
    '---',
    '',
  ];
  lines.splice(insertAt, 0, ...newLines);
  fs.writeFileSync('status-tracker-sprintwise-v1.md', lines.join('\n'), 'utf8');
  console.log('status-tracker-sprintwise-v1.md updated');
}
