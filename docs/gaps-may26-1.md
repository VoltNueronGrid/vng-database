# Gap Analysis — `vng-database` vs HTAP RDBMS Production Requirements

**Prepared:** 2026-05-04
**Reviewer:** Claude (code review pass)
**Repo:** `VoltNueronGrid/vng-database` @ `5dcebab`
**Source of requirements:** User prompt (Codd's 12 rules, ACID, persistence, multi-DB, drivers, OLAP, Studio UI)
**Methodology:** Static read of every workspace crate, the `voltnuerongridd` service, the Rust driver, and the React Studio. No build / no execution; gaps are identified by reading source.

> **Headline finding.** This is **not** a production database. It is a sophisticated **scaffold + demo** with extensive REST surface area, KPI/gate paperwork, and contract-style Rust types — but the storage engine, SQL execution, transaction manager, replication, and connection pool are all in-memory simulations or hard-coded behavior. The cursor rules even state that three crates (`core`, `meta`, `failover`) are stubs by policy. The repo is honest about this internally (`pending.md` says many requirements are at "~65%"), but the external claim of "HTAP database" is far ahead of the implementation.

---

## 0. How to read this document

Each gap is labeled with:

- **Severity**: 🔴 Critical (blocks any production use), 🟠 High (data correctness or durability risk), 🟡 Medium (scale / completeness), 🟢 Low (polish)
- **Effort**: `S` ≤ 1 week, `M` 2–4 weeks, `L` 1–3 months, `XL` quarter+, `XXL` multi-quarter
- **Where**: file:line pointer to a representative location

The user's requirement bullets are mapped at the end (§ 12).

---

## 1. Executive ranking — top 15 gaps

| # | Severity | Gap | Effort | Section |
|---|---|---|---|---|
| 1 | 🔴 | No real on-disk storage engine. State is `HashMap<String,String>` in RAM; "WAL" is text lines without checksums or `fsync`. Survives restart only by replaying lines. | XXL | §3.1 |
| 2 | 🔴 | No real `CREATE DATABASE` / `DROP DATABASE`. Database is a string fragment in catalog keys. No physical isolation, no per-DB users, no per-DB metadata schema. | XL | §3.2 |
| 3 | 🔴 | "SQL parser" is mostly `uppercase().contains(" GROUP BY ")` substring matching after token throwaway. Misclassifies anything in string literals or comments. | L | §3.3 |
| 4 | 🔴 | SELECT execution doesn't honor WHERE properly: `prefix_str = WHERE_RHS_value`, then `row_key.contains(prefix_str)`. `WHERE id=5` returns any row whose key contains "5". | M | §3.4 |
| 5 | 🔴 | No metadata / system catalog schema (`information_schema` / `pg_catalog` analogue). User explicitly asked for this; the `voltnuerongrid-meta` crate is a 3-line stub. | L | §3.5 |
| 6 | 🔴 | ACID is not actually enforced. MVCC exists in memory but is not coupled to a durable WAL with LSN ordering, no group commit, no two-phase commit, no proper isolation levels at the executor. | XL | §3.6 |
| 7 | 🔴 | No real users/passwords/login. RBAC has roles + grants but no user accounts, no password hashing (no bcrypt/argon2), no session management. Admin = a static API key in env. | M | §3.7 |
| 8 | 🟠 | Connection pool in driver is bookkeeping only — tracks `String` IDs and metrics; it does not own real TCP/HTTP connections. Each call still opens a fresh `TcpStream::connect_timeout`. | M | §3.8 |
| 9 | 🟠 | Replication / HA / failover is a documented scaffold ("does not run a background election timer or do network I/O" — `raft.rs:6`). Cluster topology endpoints return canned data. | XL | §3.9 |
| 10 | 🟠 | OLAP path: `ColumnBatch` is `HashMap<String, Vec<i64>>`-style — no Arrow IPC layout, no compression, no row groups, no zone maps, no on-disk columnar files. Apache Parquet is used only on the *ingest* boundary. | L | §3.10 |
| 11 | 🟠 | No real query planner / optimizer. `voltnuerongrid-opt` exists; the actual hot path picks "oltp/olap/hybrid" via cost = `relative_cost` heuristics on AST flags. No statistics, no cardinality estimation, no JOIN reorder. | L | §3.11 |
| 12 | 🟠 | `services/voltnuerongridd/src/main.rs` is **33,743 lines** with **469 top-level functions** and **311 HTTP routes** in one file. Unmaintainable, untestable in isolation, very high refactor risk. | L | §4.1 |
| 13 | 🟡 | Drivers: only Rust + C are real source crates. Java/Python/Node/TS/Deno/Perl folders exist but most are placeholder packaging. The user wants HTTP/HTTPS/native drivers across languages. | L | §3.12 |
| 14 | 🟡 | Studio UI design tokens drift from `studio-design.html` (token name mismatches: `--radius-sm` vs `--r-sm`; some hex values differ). UI does not expose CREATE DATABASE / per-DB users at all. | M | §6 |
| 15 | 🟡 | Tests use mostly in-process state. KPI gates appear to derive pass/fail from JSON the same code writes. Few end-to-end tests against a real socket; almost no chaos / crash-recovery tests against actual disk. | M | §10 |

---

## 2. What's actually solid

It's only fair to call out what's well done before listing gaps:

1. **Workspace structure** — Cargo workspace with one crate per concern (`sql`, `store`, `exec`, `auth`, `audit`, `ingest`, etc.) is the right shape.
2. **Tokenizer** (`crates/voltnuerongrid-sql/src/tokenizer.rs`) — proper handcrafted lexer, handles quoted idents, comments, numeric literals, multi-char symbols. About 415 lines, looks correct.
3. **MVCC primitives** (`crates/voltnuerongrid-store/src/mvcc.rs`) — version chains with snapshot reads at `xid <= snapshot_xid`. Logically correct as an in-memory model; just not connected to durable storage.
4. **Audit chain** (`voltnuerongrid-audit`) — append-only sink with chained hash records; a reasonable starting point for SOC-style logging.
5. **Ingest** — real Apache Arrow + Parquet readers, real CSV/JSON/Excel handlers. Among the most production-shaped code in the repo.
6. **Cursor rules + workspace-level tooling** — the team cares about hygiene; `.cursorrules` is concrete and enforces RBAC checks.
7. **Native wire-frame protocol** — `services/voltnuerongridd/src/main.rs` does have framed-binary `TcpStream`-based reads/writes for a "native" protocol; the framing is real (length-prefixed JSON), even if higher-level concepts aren't.

---

## 3. Engine / runtime gaps

### 3.1 🔴 Storage engine is not durable storage

**Where:** `crates/voltnuerongrid-store/src/lib.rs:58-169`, `wal_adapter.rs:88-150`

**What's there:**
- `InMemoryDurabilityEngine` — a `HashMap<String, String>` plus a `Vec<WalRecord>`.
- `FileWalAdapter` — appends `seq\ttsms\tkey\tvalue\n` text lines to a file, calls `file.flush()` (NOT `fsync`), reads them back on recovery.

**Why it's not production:**
1. **No `fsync`/`fdatasync`.** `file.flush()` only drains userspace buffers. A power loss after a successful 200 OK loses committed data. Production WALs `fsync` on commit (or batch via group commit).
2. **No checksums.** A torn write produces a "valid-looking" partial line. Recovery has no way to detect corruption.
3. **No record framing other than newline.** A literal `\n` in a column value is escaped via `\\n` substitution — fine for plaintext, but binary data (BLOBs, UUIDs as raw bytes) is impossible.
4. **No log segmentation, no rotation, no retention.** The WAL grows forever until `force_checkpoint()` truncates the in-memory `Vec` — but the on-disk file is **not truncated** by checkpoint (see `force_checkpoint` body — disk truncate isn't called). So the file grows unbounded, and replay time grows linearly with history.
5. **Recovery loads into RAM only.** `recover_from_records` rebuilds the `HashMap` — there's no concept of pages, no buffer pool, no bringing tables back from disk lazily.
6. **Schema-less keys.** Records are `String -> String`. Multi-column rows are encoded into row-data hashmaps and serialized via the SQL path. The storage layer has no concept of types.
7. **Two parallel WALs.** `services/voltnuerongridd/src/main.rs:828-911` defines its **own** DDL WAL and DML WAL paths separate from the store's WAL adapter. They are uncoordinated — there is no global LSN, no causal ordering between DDL and DML.

**What "production" means for this:**
- Page-based heap files (8-16 KB pages, like Postgres / InnoDB).
- A single global WAL with LSN-ordered records, segmented into ~16 MB files.
- Records framed with `[length][type][page_id][before_image|after_image][CRC32]`.
- Group commit: `fsync` once for N concurrent commits to amortize disk latency.
- A buffer pool with clock/LRU eviction.
- Recovery via ARIES-style redo + undo, replaying from last checkpoint LSN.

**Effort:** XXL. This is the heart of the database. Greenfield page-based storage + WAL is 2-3 person-quarters minimum to a usable bar; production-grade is much more. Realistic alternative: use **`sled`** or **`redb`** (embedded ACID KV stores written in Rust) as the storage substrate, and build the row-store on top. That gets durable storage in weeks instead of quarters.

---

### 3.2 🔴 No `CREATE DATABASE` / database isolation

**Where:** Search for `CREATE DATABASE` in `services/voltnuerongridd/src/main.rs` returns 0 hits. Catalog entries store `database_name` as a string but it's just a key prefix — not a separate physical store.

**User requirements not met:**
- "We can create any number of databases in each connection" — there's no API to create one.
- "Each database should not be repeated" — there's nothing to enforce uniqueness *of databases*; only of objects within them.
- "Each database can have its own users and roles" — RBAC is global, not per-DB.
- "Inside a database, we cannot have any duplicate names for schemas/tables/columns/views/triggers/events/functions" — only tables/views/functions/triggers/events are tracked in `DdlCatalog`. Columns and schemas have no uniqueness enforcement of their own.
- "If I login into a database using a connection and have privileges, then I should be able to manipulate all the database objects" — there's no notion of "logging into a database"; the connection has no DB context.

**Code evidence:** `crates/voltnuerongrid-store/src/ddl_catalog.rs:55-62`:
```rust
fn qualified_key(db: &str, schema: &str, name: &str) -> String {
    format!("{}.{}.{}", db.trim().to_ascii_lowercase(), ...)
}
```
That's the entire database isolation story — a string key. All "databases" share the same in-memory map.

**Effort:** XL. To do this right:
- A `Database` aggregate root that owns: its own catalog, its own row store partition (or its own files if §3.1 is fixed), its own role grants, its own metadata schema.
- New SQL: `CREATE DATABASE`, `DROP DATABASE`, `\c <db>` / `USE <db>`.
- New connection state: every connection has a current-database pointer; login requires a database name.
- New routes: `POST /api/v1/admin/databases`, `GET /api/v1/admin/databases`, `DELETE /api/v1/admin/databases/{name}`.

---

### 3.3 🔴 SQL "parser" relies on uppercase substring search

**Where:** `crates/voltnuerongrid-sql/src/ast.rs:309-440` (the `parse_tokens` body)

The tokenizer is fine. The parser then **discards the tokens** for many decisions and does:

```rust
let up = raw.to_ascii_uppercase();
if up.contains(" UNION ") { stmt.has_union = true; }
if up.contains("(SELECT") || up.contains("( SELECT") { stmt.has_subquery = true; }
if up.contains("OVER (") || up.contains("OVER(") { stmt.has_window_fn = true; }
let up_trim = up.replace(' ', "");
if up_trim.contains("COUNT(") || up_trim.contains("SUM(") ... { stmt.has_agg_fn = true; }
if up.contains("IS NULL") || up.contains("IS NOT NULL") { stmt.has_null_literal = true; }
if up.contains("GROUP BY") { stmt.has_group_by = true; }
```

**Concrete bugs this causes:**
- `SELECT 'GROUP BY' FROM t` → `has_group_by = true`.
- `SELECT '-- COUNT(*)' FROM t` → `has_agg_fn = true`.
- `SELECT name FROM users -- WHERE deleted = 1` → comment text is in `up`, predicate flags get set.
- Any case-folding of multi-byte characters in UTF-8 column values is broken (`to_ascii_uppercase` ignores non-ASCII).

**Why it matters:** these flags drive routing decisions (OLTP vs OLAP), which means a SELECT with a string literal containing "GROUP BY" can be routed to a different executor than intended.

**Effort:** L. The fix is mechanical but invasive: walk the token stream, track parenthesis depth, recognize keywords by token kind not by raw text. The structure is already there in `semantic_tokens()`; just stop calling `up.contains(...)`. Or replace with `sqlparser-rs` (a mature, ANSI-ish SQL parser used by DataFusion).

---

### 3.4 🔴 SELECT execution ignores WHERE predicates

**Where:** `services/voltnuerongridd/src/main.rs:15991-16038` (`execute_oltp_select`)

The actual SELECT handler does:

```rust
let prefix: Option<String> = sel.where_clause.as_deref().and_then(|w| {
    let eq = w.find('=')?;
    let rhs = w[eq + 1..].trim();
    let val = rhs.trim_matches('\'').trim_matches('"').trim();
    if val.is_empty() { None } else { Some(val.to_string()) }
});
let prefix_str = prefix.as_deref().unwrap_or("");
let batch: Vec<OltpRowResult> = all_rows
    .iter()
    .filter(|(k, _)| prefix_str.is_empty() || k.contains(prefix_str))
    .take(remaining)
    .map(|(k, d)| OltpRowResult { key: k.clone(), data: d.clone() })
    .collect();
```

Translation:
- Find `=`, take RHS as string.
- Match rows where the **row key** (which is `"tablename:rowid"`) contains that string anywhere.

**Concrete failure cases:**
- `WHERE id = 5` matches every row in any table whose name contains "5" or whose rowid contains "5" (so rows 5, 15, 25, 50, 51, ...).
- `WHERE name = 'alice'` matches rows whose key (not value!) contains "alice" — so almost never anything correct.
- `WHERE x = 1 AND y = 2` ignores everything after the first `=`.
- `WHERE x > 10` (or `<`, `<>`, `LIKE`, `IN`, `BETWEEN`) is silently ignored — no `=` found, returns all rows.
- `SELECT col1, col2 FROM t` always returns the entire row hashmap, ignoring projection.

This is not a bug; it's the entire SELECT implementation. It would not pass a "Hello, World" use case for a database.

**Effort:** M to get a correct in-memory SQL evaluator (filter + project + sort + limit on a row vector). L to plug into a real planner. Better path: integrate **DataFusion** as the executor, which gives correct SQL semantics and a real optimizer for free.

---

### 3.5 🔴 No metadata / system catalog schema

**Where:** `crates/voltnuerongrid-meta/src/lib.rs` is **3 lines**:

```rust
#![forbid(unsafe_code)]
pub const CRATE_NAME: &str = "voltnuerongrid-meta";
```

The user explicitly asked: *"a system schema (called as metadata schema — a separate schema) in each database where all the parameters and information is stored, which is created for every database by default. Similar to information_schema and pg_catalog schema in postgres."*

**What's missing:**
- No `information_schema.tables`, `information_schema.columns`, `information_schema.schemata`, `information_schema.routines`, etc.
- No `pg_catalog`-style internal views over the catalog.
- No SQL-queryable settings/parameters table for tuning.
- The Studio UI has no place to render metadata via SQL — it goes through `/api/v1/admin/schema/tree`, which is a custom JSON endpoint, not a SQL surface.

**Effort:** L. With §3.1-3.4 fixed, this becomes "expose the in-memory catalog as virtual tables in a `metadata` schema per database." Without those fixed, it can still be done as read-only views that a future executor will surface.

---

### 3.6 🔴 ACID is claimed but not enforced end-to-end

**Where:** Across `mvcc.rs`, `main.rs:7078-7345` (sql_transaction), `wal_adapter.rs`

Individual pieces exist:
- MVCC version chains: ✅ (in-memory).
- BEGIN / COMMIT / ROLLBACK statements: parsed.
- A `TX_COUNTER`, a `transaction_id` returned on commit.
- Pessimistic-lock endpoints with deadlock-detection counters.

What's missing for actual ACID:
- **Atomicity:** when a multi-statement batch fails midway, partial inserts have already been written to the row store. There's no UNDO log because there's no journaled before-image. Rollback on crash is impossible — recovery just replays the WAL forward.
- **Consistency:** constraints exist (`constraints.rs`), but they aren't checked against the version chain at commit time; they're checked per-statement.
- **Isolation:** the parser exposes `READ COMMITTED / REPEATABLE READ / SERIALIZABLE`; the executor doesn't differentiate them. Snapshot reads use `current_xid()`, which is just a counter.
- **Durability:** see §3.1 — no `fsync`, so a crash between flush and disk-cache write loses committed transactions.

**Effort:** XL — coupled with §3.1.

---

### 3.7 🔴 No user accounts / no real authentication

**Where:** `crates/voltnuerongrid-auth/src/lib.rs` (985 lines)

Searches for `password`, `bcrypt`, `argon2`, `hash`, `login` return zero matches in the auth crate. What it has:
- KMS key-reference adapters (AWS / Azure / GCP CLI shellouts).
- An `RbacPrivilegeMatrix` mapping role → resource grants.
- `SecurityConfigContract` defining "admin_api_key_env" — i.e. the entire admin auth model is `if header equals env-var, you're admin`.

User requirement: *"Each database can have its own users and roles."*

**What's missing:**
- A `users` table (per database) with username + password hash (Argon2id, ideally).
- `CREATE USER`, `ALTER USER`, `DROP USER` SQL.
- `CREATE ROLE`, `GRANT role TO user`, `GRANT priv ON object TO role`.
- A login flow that takes (database, username, password) and returns a session token / cookie / JWT.
- Password rotation, password complexity, lockouts.
- Per-connection authenticated identity that propagates to RBAC checks.

**Effort:** M for the basics; L for full Postgres-equivalent behavior.

---

### 3.8 🟠 Connection pool is a metric facade

**Where:** `drivers/voltnuerongrid-driver-rust/src/lib.rs:1851-1970`

`ConnectionPoolManager` keeps a `Vec<PooledConnection>` where each `PooledConnection` has a `connection_id: String` and state markers, but **no socket, no client, no underlying resource**. `acquire()` returns a string ID; nothing actually happens to a TCP connection.

Meanwhile, the real wire code (`run_native_connection`, `driver.execute`) opens a fresh `TcpStream::connect_timeout` per request. The pool is not in the call path.

**Effort:** M. Replace with a real pool — for HTTP, use `reqwest::Client` (which has its own keep-alive pool); for the native protocol, build a `bb8`-style pool over `TcpStream` (or `tokio::net::TcpStream` for async).

---

### 3.9 🟠 No real replication / HA

**Where:** `services/voltnuerongridd/src/raft.rs:1-7` says it explicitly:

> Raft consensus algorithm scaffold — S7-WS6-02. Provides a single-node Raft state machine that can answer vote requests and accept append-entries RPCs. **The implementation is a scaffold: it models all the required state transitions and log structures but does not run a background election timer or do network I/O.**

`failover` crate is a 3-line stub. `htap_sync.rs` exists in store but is `InMemoryReplicationTransport`.

**Effort:** XL. Honestly, defer this — fix §3.1 first.

---

### 3.10 🟠 OLAP path is in-memory only

**Where:** `crates/voltnuerongrid-store/src/columnar.rs`

`ColumnVector::Int64(Vec<i64>)`, `Float64(Vec<f64>)`, `Utf8(Vec<String>)`, `Bool(Vec<bool>)`. No nulls bitmap (separate type `Null(usize)` — every value or no value, no per-row null mask). No dictionary encoding, no run-length, no compression, no chunking, no zone maps. Always built from a full scan of all rows.

User requirement: *"this is HTAP database, we need to have similar functionalities for OLAP based database as well."*

**Path forward:** adopt **Apache Arrow `RecordBatch`** as the in-memory columnar format (already a dependency!) and **Parquet** for on-disk OLAP storage. Both are already used at the ingest boundary; promoting them to the storage substrate would cut a huge amount of bespoke code.

**Effort:** L if §3.1 is unblocked.

---

### 3.11 🟠 Optimizer / planner is heuristic, not cost-based

**Where:** `crates/voltnuerongrid-opt/src/lib.rs` (720 lines), `crates/voltnuerongrid-exec/src/planner.rs` (4417 lines), `services/voltnuerongridd/src/main.rs:8040-8063`

It picks `oltp` / `olap` / `hybrid` paths based on AST flags (has_aggregate, has_join, has_window_fn) and a `relative_cost` number that's mostly just summed flag weights. There's no:
- Table cardinality stats.
- Histogram-based selectivity estimation.
- JOIN order choice.
- Predicate pushdown to storage (the bool exists but the storage scan is full-table anyway).
- Plan caching.

**Effort:** L (or "free" if we adopt DataFusion as the executor).

---

### 3.12 🟡 Drivers — Rust + C only; rest are stubs

**Where:** `drivers/`

- `voltnuerongrid-driver-rust` — 4184 lines, real (but talks to a single endpoint without real pooling, see §3.8).
- `voltnuerongrid-driver-c` — present.
- `voltnuerongrid-driver-cffi-poc` — POC.
- `voltnuerongrid-driver-deno` / `node` / `typescript` / `python` / `java` / `perl` — directories exist; need to verify each is more than a `package.json` + README.

User requirement: *"I should be able to retrieve the data from the database using http/https/native drivers."*

**Effort:** L per language for a basic-functional driver (HTTP wrapper, prepared statements, cursors). Pure HTTP wrappers are M each.

---

## 4. Codebase organization gaps

### 4.1 🟠 33,743-line `main.rs`

**Where:** `services/voltnuerongridd/src/main.rs`

469 top-level functions. 311 routes. Multiple concerns (HTTP handlers, SQL execution, WAL persistence, audit, RBAC, MCP, Raft, ingest dispatch, native protocol) live in one file.

**Why it matters:**
- Compile times: any change recompiles 33k lines.
- Code review: PRs touching this file are unreviewable.
- Test isolation: nothing can be tested without bringing up the whole monolith.
- Cognitive load: there's no way for a new contributor to know where, say, "session token issuance" lives.

**Effort:** L. Mechanical refactor — move each route handler into a `routes/` module, each subsystem into its own module. Should reduce risk of further bit-rot dramatically.

---

### 4.2 🟡 Pervasive `.unwrap()` and `.expect()` in handlers

**Where:** Examples in `main.rs`:
- `state.row_store.lock().expect("row_store lock early_call")`
- `state.ddl_catalog.lock().expect("ddl_catalog lock early_call")`
- Many more.

The `.cursorrules` file says: *"No `unwrap()` in handler paths, no `panic!` in handlers"*. The actual code violates this in dozens of places. A poisoned mutex (which happens after any panic in a critical section) takes the entire service down for any subsequent request.

**Effort:** S — replace with `if let Ok(x) = lock` + 503 response.

---

### 4.3 🟡 Demo data hardcoded in production paths

**Where:** `services/voltnuerongridd/src/main.rs:7562-7682` (the `CALL insert_rows(...)` early intercept)

The SQL execute handler intercepts `CALL insert_rows('tbl', N)` and generates synthetic rows with column-name-based heuristics:

```rust
} else if col_name.contains("name") {
    format!("Generated {table_name} {row_id}")
} else if col_name.contains("price") {
    format!("{:.2}", 10.0 + (row_id as f64) * 0.5)
} else if col_name.contains("status") {
    ["active", "pending", "done", "cancelled"][row_id % 4].to_string()
}
```

This is fine as a demo. It is **not fine** living in the SQL execute path — it intercepts before the parser runs, so any user-defined stored procedure named `insert_rows` is shadowed forever.

**Effort:** S — move to a separate `/api/v1/demo/seed` route.

---

### 4.4 🟢 `.DS_Store`, multiple status trackers, scratch `.md` files committed

The repo has `.DS_Store`, `status-tracker.md` (228 KB), `status-tracker-v3.md`, `status-tracker-sprintwise-v1.md`, `status_tracker.md`, `status-todo.md`, `temp.md`, `wip.md`, `understanding.md` — clear sign of a working file dump rather than maintained docs.

**Effort:** S — consolidate to one `STATUS.md` and move the rest to a `docs/archive/` folder, gitignore `.DS_Store`.

---

## 5. SQL feature coverage gaps (against ANSI / Codd)

Codd's 12 rules require specific behaviors. Mapping to current state:

| Codd rule | Status | Notes |
|---|---|---|
| 1. Information rule (data as values in tables) | 🟡 Partial | Tables exist; they're keyed by string row keys, not first-class relations. |
| 2. Guaranteed access (every value reachable by table+pk+col) | 🔴 No | SELECT can't project columns or filter by anything but row-key substring. |
| 3. Systematic null handling | 🔴 No | `ColumnVector::Null(usize)` is "all rows null"; no per-row null mask. |
| 4. Active online catalog as relations | 🔴 No | Catalog is a `HashMap`, not queryable via SQL. See §3.5. |
| 5. Comprehensive data sublanguage (full SQL DDL+DML+auth+constraints+xact) | 🟠 Partial | Most SQL is parsed; little is executed correctly. |
| 6. View update | 🔴 No | Views recorded in catalog; no update propagation, no view materialization. |
| 7. High-level insert/update/delete (set-at-a-time) | 🔴 No | UPDATE / DELETE handlers extract a single key from WHERE; cannot affect a set. |
| 8. Physical data independence | 🟠 Partial | Schema is decoupled from row representation, but SELECT execution leaks the storage key format. |
| 9. Logical data independence | 🟡 Partial | Views defined but not used in queries. |
| 10. Integrity independence (constraints in catalog, enforced by DBMS) | 🟡 Partial | `constraints.rs` exists; not enforced uniformly. |
| 11. Distribution independence | 🔴 No | Single process, no distribution. |
| 12. Nonsubversion (low-level access can't bypass integrity) | 🟡 Partial | `forbid(unsafe_code)` is set workspace-wide — good. But the in-memory state can be mutated through admin endpoints without going through MVCC. |

**Concrete missing SQL:**
- `CREATE DATABASE` / `DROP DATABASE`
- `CREATE SCHEMA` / `DROP SCHEMA` (catalog tracks schemas but no DDL exists for them)
- `CREATE USER` / `CREATE ROLE` / `GRANT` / `REVOKE` (RBAC is config-file driven)
- `ALTER TABLE ADD/DROP/ALTER COLUMN` (catalog records the ALTER but doesn't apply it to data)
- Real `JOIN` execution (parsed; not executed)
- `GROUP BY` execution (parsed; the OLAP path applies a `Count` aggregator over every column unconditionally — see `main.rs:8118-8140`)
- `ORDER BY` execution
- Sub-queries
- Window functions
- `WITH` (CTEs)
- `MERGE` / `UPSERT` (other than the demo)
- Prepared statements / parameter binding (the wire protocol carries them; the executor ignores binding)
- Cursors / pagination beyond LIMIT
- Transactions with savepoints (parsed; not executed)
- Indexes — `IndexManager` exists but the SELECT path doesn't consult it.

---

## 6. UI / Studio gaps

### 6.1 🟡 Design tokens drift from the design source-of-truth

**Where:** `ui/voltnuerongrid-studio/design/studio-design.html` vs `ui/voltnuerongrid-studio/src/styles/globals.css`

Diffing the two CSS variable blocks:

| Token | Design | Code (dark) | Notes |
|---|---|---|---|
| `--radius-sm` / `--r-sm` | `--radius-sm: 4px` | `--r-sm: 4px` | **Name mismatch** — design uses `--radius-*`, code uses `--r-*`. |
| `--bg-4` | `#222232` | `#232334` | drift |
| `--bg-hover` | `#252538` | `#25253a` | drift |
| `--border` | `#252534` | `#21212e` | drift |
| `--border-strong` | `#333348` | `#2e2e3e` | drift |
| `--text-3` | `#6a6a88` | `#5a5a78` | drift |

Plus the code has `--brand-cyan-low` and `--right-panel-w` that aren't in the design (additions are fine), and the design has `--radius-*` that aren't in the code (gap).

**Effort:** S. Two paths:
- **A:** Rename code's `--r-{sm,md,lg}` to match design's `--radius-{sm,md,lg}`, and update the dark hex values to exactly match. Keep additions (`--brand-cyan-low`, `--right-panel-w`).
- **B:** Update `studio-design.html` to match the code (declare it the source of truth). I'd lean toward A since the user explicitly called out the design file.

### 6.2 🟠 No UI surface for any of the user's requirements

The user wants the Studio to support:
- Create / drop databases with name uniqueness.
- Per-database users and roles.
- Connection settings (pool size, timeouts).
- Tunable database parameters (a "settings" panel reading the metadata schema).
- HTAP query routing visibility (OLTP vs OLAP for each query).
- OLAP-specific monitoring.

Inventory of `src/components/`:
- `ConnectionPanel/` — connection management ✅ (but only against a fixed `database` string field on the connection, no create UI)
- `Sidebar/SchemaTree.tsx` — renders the catalog tree ✅
- `Sidebar/UsersPanel.tsx` — exists; need to check if wired to backend (the backend has no users endpoint, so this can't be live-wired)
- `Settings/SettingsPanel.tsx` — exists; need to check if it tunes anything real
- `Workspace/SqlEditorPane.tsx` — Monaco editor for SQL ✅
- `ResultsPane/` — table rendering ✅
- `RightPanel/RightPanel.tsx` — DDL inspector
- `Modals/ResourceModal.tsx` — generic create-resource modal

**What's missing:**
- A "Databases" view: list / create / drop databases.
- A real Users & Roles panel (depends on backend §3.7).
- A "Server settings" panel reading from a metadata schema (depends on §3.5).
- Per-query routing badges in the results pane (the backend response carries `route_path`; surface it).

**Effort:** M for the UI; gated on backend gaps §3.2, §3.5, §3.7.

### 6.3 🟢 Playwright tests exist but coverage is limited

`ui/voltnuerongrid-studio/tests/` — confirm coverage.

---

## 7. Observability gaps

- **Tracing:** no `tracing` / OTEL integration in the service. Some `eprintln!` and audit-event writes; no spans, no metrics export.
- **Metrics:** several `AtomicU64` counters in `main.rs`; no `/metrics` endpoint in Prometheus format that I can find.
- **Health:** `/health` exists; returns a static-shape JSON.

**Effort:** S to add `tracing-subscriber` + `metrics` crate + `/metrics` route.

---

## 8. Security gaps

| Gap | Severity | Notes |
|---|---|---|
| No password hashing (no users) | 🔴 | §3.7 |
| Admin auth = single static API key in env | 🔴 | Cannot rotate per-user; one leak compromises everything |
| TLS termination supported (`tokio-rustls`) | ✅ | Good |
| mTLS / client cert auth | 🟡 | `mtls_required` in config but I haven't traced enforcement |
| Audit log integrity | 🟡 | Hash chain exists; need to verify it's not optional |
| KMS integration | 🟡 | CLI-shellout based — fragile in production; should use cloud SDKs |
| Plugin signing | 🟡 | Manifest signature struct exists; verification path unclear |
| SQL injection protection | 🔴 | Since SQL "execution" is mostly substring matching, prepared statements with parameter binding aren't really used; the wire protocol carries them but the server inlines |

---

## 9. Persistence configurability

User requirement: *"All the data should be written to the files and read from the files with high performance and using parallel reads/writes — which can also be fine-tuned. You can suggest better idea if not in files to avoid crashes to maintain persistence."*

**Recommendation:** for the next milestone, do **not** build a bespoke storage engine. Use one of:

1. **`sled`** — pure-Rust embedded ACID KV with a log-structured tree. Good fit for this codebase's "everything is a key" current shape. Battle-tested.
2. **`redb`** — also pure-Rust, copy-on-write B-tree, simpler model than sled.
3. **`rocksdb`** (via `rust-rocksdb`) — C++ dep, but the most production-grade LSM available. Used by Cockroach, TiDB, and many others.
4. **Postgres-as-storage**: deploy a Postgres process and use it as the backing store, while VNG handles routing/HTAP/OLAP. (Like Citus / Hyperscale.) Not what the user asked for, but it's what most "production HTAP" startups do.

Given the user explicitly wants files + parallel I/O fine-tuning, **RocksDB** (option 3) is the closest off-the-shelf match. It supports column families (one per database!), tunable write threads, configurable compaction, and direct I/O.

**Effort:** L to swap in RocksDB and rewire the row store; a transformative change.

---

## 10. Testing gaps

- **Unit tests:** present for many crates. Good coverage at the leaf level.
- **Integration tests:** mostly in-process — start an `AppState` via `state_with_key()` and call handler functions directly. Don't exercise the HTTP path or the wire protocol end-to-end.
- **Crash tests:** no test that I found which:
  1. Inserts data
  2. Hard-kills the process
  3. Restarts
  4. Asserts data is still there
- **Concurrency / soak:** `tests/soak/` exists; would need to run them to know what they assert. The fact that there's no real disk I/O limits how meaningful soak results are.
- **KPI gates:** scripts read JSON files written by the same process. Self-graded. Risk of "test passes because the test wrote the success status itself."

**Effort:** M to build a real end-to-end harness (testcontainers-style), L to instrument crash-recovery tests once §3.1 is fixed.

---

## 11. OLAP-specific gaps (user explicitly asked)

User requirement: *"Since this is HTAP database, we need to have similar functionalities for OLAP based database as well. Please add all those functionalities needed for OLAP as well."*

**What's missing:**
- On-disk columnar format (Parquet or Arrow IPC). Currently OLAP is a one-time scan of the row store, copied into in-memory `ColumnBatch`.
- Materialized views with refresh policies. `materialized_view` is recorded in the catalog; nothing materializes anything.
- Aggregation pushdown.
- Star-schema detection (fact + dimension tables).
- Join algorithms beyond nested-loop (hash join, sort-merge join).
- Result caching for repeat OLAP queries.
- Tiered storage (hot OLTP rows in memory, cold OLAP columns on disk/object store).
- Bulk-load fast path (most "real" OLAP DBs let you bulk-import without going through the OLTP write path).
- Query parallelism (currently single-threaded executor per query).

**Effort:** XL collectively. Highest-value wedge: integrate **DataFusion + Parquet** for the OLAP path — pay maybe 2 weeks of integration cost and get a real columnar engine in return.

---

## 12. Mapping back to the user's explicit requirement bullets

| Bullet (paraphrased) | State | Section |
|---|---|---|
| Codd's 12 rules + ANSI SQL + ACID | 🔴 Mostly not | §3.1 §3.6 §5 |
| Data persistent across restarts/crashes | 🔴 Not durable | §3.1 |
| Files with parallel reads/writes, tunable | 🔴 Not present | §9 |
| Database ≠ connection | 🟠 Connection model exists, no DB model | §3.2 |
| Many connections per DB | 🟡 Driver pool decorative | §3.8 |
| Many DBs per connection | 🔴 No CREATE DATABASE | §3.2 |
| Per-DB users & roles | 🔴 No users | §3.7 |
| No duplicate DB names | 🔴 No DBs | §3.2 |
| No duplicate names within DB (schemas/tables/cols/views/triggers/events/functions) | 🟡 Partial — catalog enforces some | §3.5 |
| Privileged login → manipulate all DB objects | 🟠 Privilege check yes, login no | §3.7 |
| Pool & direct connection options, tunable | 🟡 API exists, not effective | §3.8 |
| HTTP/HTTPS/native drivers | 🟡 Rust + C real, others stubs | §3.12 |
| Internal paging / row limits | 🟡 LIMIT is honored; no real paging | §3.4 |
| Tunable via service / config / metadata schema | 🔴 No metadata schema | §3.5 |
| OLAP functionality | 🟠 In-memory columnar, no on-disk | §3.10 §11 |
| Studio UI for all the above, Playwright-tested | 🟡 Components exist; no DB/users UI | §6 |
| Studio design fidelity to studio-design.html | 🟡 Token name + value drift | §6.1 |

---

## 13. Recommended sequencing

You said "all of the above" for focus. Here is an honest sequencing — going fastest-to-most-leverage. Each phase is a milestone that produces something usable.

### Phase 0 — week 1 (immediate)
- **0.1** Wire the design tokens: rename `--r-*` ↔ `--radius-*`, sync hex values, update both `globals.css` (dark + light) and `studio-design.html` to a single matched palette. (§6.1)
- **0.2** Move the `CALL insert_rows` demo intercept out of the SQL execute path. (§4.3)
- **0.3** Replace `.expect("... lock")` in handler paths with proper error returns. (§4.2)
- **0.4** Add a `/metrics` endpoint and basic `tracing` setup. (§7)
- **0.5** Remove `.DS_Store`, consolidate `status*` files. (§4.4)

### Phase 1 — month 1 (correctness wedge)
- **1.1** Replace the substring-flag parser with proper token-walking (or adopt `sqlparser-rs`). (§3.3)
- **1.2** Implement correct `WHERE` evaluation, projection, ORDER BY, LIMIT/OFFSET on the row store. (§3.4)
- **1.3** Implement `CREATE DATABASE` / `DROP DATABASE` / `USE DATABASE` end-to-end (catalog + connection state + UI). (§3.2)
- **1.4** Stand up a `metadata` schema per database with virtual tables: `tables`, `columns`, `schemas`, `routines`, `settings`. (§3.5)
- **1.5** Studio: add Databases pane and per-DB metadata browser. (§6.2)

### Phase 2 — month 2-3 (durability wedge)
- **2.1** Adopt **RocksDB** (or `sled` if zero-C++-deps is required) as the storage substrate. One column family per database = real isolation. (§9)
- **2.2** Move WAL into RocksDB's WAL; remove text-line WAL. (§3.1)
- **2.3** Make recovery a startup invariant — open RocksDB, no replay step needed. (§3.1)
- **2.4** Add a real crash-recovery integration test (§10) and run it in CI.

### Phase 3 — month 3-4 (auth wedge)
- **3.1** Build users + password hashing (Argon2id) + login (per-DB session tokens). (§3.7)
- **3.2** `CREATE USER` / `GRANT` SQL plumbed through to the existing RBAC matrix.
- **3.3** Studio: real Users & Roles panel.

### Phase 4 — month 5+ (performance / OLAP wedge)
- **4.1** Adopt **DataFusion** for the OLAP execution path; back it with Parquet files written on a configurable cadence from the OLTP store (HTAP sync). (§3.10 §3.11 §11)
- **4.2** Build a real driver connection pool (`bb8` or `deadpool`) for both HTTP and native protocols. (§3.8)
- **4.3** Drivers: bring TypeScript and Python to first-class, since the user names them in the prompt history. (§3.12)

### Phase 5+ — quarter+ (HA)
- **5.1** Real Raft (or use `openraft` crate). Probably gated on enterprise demand.

---

## 14. Open questions for you

Before I start coding fixes, I'd like to confirm:

1. **Storage substrate:** are you OK with adopting **RocksDB** for durable persistence, or do you specifically want a from-scratch Rust storage engine? RocksDB is the realistic path; from-scratch is a year of work.
2. **SQL engine:** are you OK with adopting **DataFusion + sqlparser-rs** for execution & parsing, or do you want to keep the bespoke pieces? DataFusion would replace `voltnuerongrid-exec`, `voltnuerongrid-opt`, and most of the SQL execution surface — big refactor, but turns "doesn't work" into "works correctly" overnight.
3. **Scope priority:** if I have to pick ONE of {durable storage, real SQL execution, multi-DB + users, OLAP path, drivers, UI parity} to land first, what do you want to demo end-to-end?
4. **Backwards compatibility:** are the existing 311 HTTP routes a contract you need to keep, or are some of them legacy / experimental that I can prune in the refactor?
5. **The 33k-line `main.rs` refactor:** can I split it into modules in a separate PR before functional work? It's a prerequisite for clean changes after.

---

*End of analysis. Total identified gaps: 35+ across 11 sections.*
