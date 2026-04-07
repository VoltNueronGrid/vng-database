#!/usr/bin/env node
// Update status_tracker.md with Session 46 results
'use strict';
const fs = require('fs');

const lines = fs.readFileSync('status_tracker.md', 'utf8').split('\n');
const insertAt = 433; // Before line 434 (9.2m Session 45)

const s46Lines = [
  '### 9.2n Session 46 Implementation Update (S3-WS1-22 + service endpoints)',
  '',
  '- **SQL (`voltnuerongrid-sql`)**: Added `has_trim: bool` field to `SelectStatement` (ast.rs). Detects `TRIM(`, `LTRIM(`, `RTRIM(` via `up_trim` buffer (S3-WS1-22). Tests: `trim_tests` module (3 tests). Total: **168 passed**.',
  '- **Exec (`voltnuerongrid-exec`)**: Added `Trim { input }` variant to `LogicalPlan` enum (planner.rs). Updated `primary_table()`, `has_aggregation()`, `estimate_cost()` (OLTP path, pass-through rows, +0.05 cost), and `plan_select()` converted NotIn to `let after_not_in` and added Trim outermost wrap. Tests: `planner_trim_select_produces_trim_node`, `cost_trim_query_routes_to_oltp`. Total: **68 passed**.',
  '- **Service (`voltnuerongridd`)**: Added `GET /api/v1/store/wal/age` (operator-auth, returns oldest_sequence, newest_sequence, sequence_span from live WAL) and `GET /api/v1/store/rows/first/key` (operator-auth, returns first alphabetically-sorted key in the MVCC row store). Tests: 4 new tests (`s11_ws1_22_*`). Total: **417 passed**.',
  '',
];

lines.splice(insertAt, 0, ...s46Lines);
fs.writeFileSync('status_tracker.md', lines.join('\n'), 'utf8');
console.log('Inserted S46 update. New line count:', lines.length);
