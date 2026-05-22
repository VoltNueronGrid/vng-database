# VoltNueronGrid Core Systems & UI Diagnostic Report (vng-issues-1.md)

This diagnostic report provides a comprehensive architectural and systems-level analysis of all issues identified within the VoltNueronGrid Database Engine and its Studio UI (`localhost:1420`). 

Based on rigorous codebase auditing and running the Playwright End-to-End (E2E) verification suite, six major architectural and implementation gaps have been discovered. Below is the detailed breakdown of each issue, followed by a summary matrix.

---

## 1. Comprehensive Analysis of Identified Issues

### ISSUE-01: RDBMS Column Type Parenthesis Bug (Parser Failure)
* **Component/Layer**: `crates/voltnuerongrid-sql/src/ast.rs` (`parse_create` function)
* **Severity**: **CRITICAL**
* **Description**:
  When a column is defined with length, precision, or scale constraints inside parentheses (e.g., `VARCHAR(255)` or `DECIMAL(10,2)`), the AST datatype parser breaks prematurely.
* **Root Cause & Code Reference**:
  Inside `ast.rs` (lines 1262-1278), the inner loop parsing the column's data type encounters a closing parenthesis `)` and breaks:
  ```rust
  while pos < tokens.len() {
      match &tokens[pos] {
          Token::Symbol(s) if s == "," || s == ")" => break,
          // ...
      }
  }
  ```
  This inner loop breaks *without* consuming the `)` token or advancing `pos`. Consequently, in the next iteration of the outer loop parsing the table's columns:
  ```rust
  while pos < tokens.len() {
      match &tokens[pos] {
          Token::Symbol(s) if s == ")" => break,
          Token::Symbol(s) if s == "," => pos += 1,
          tok => { // ... }
      }
  }
  ```
  The outer loop immediately sees the unconsumed `)` token and interprets it as the end of the `CREATE TABLE` column definitions list, terminating the entire parsing process.
* **Implications & Impact**:
  * **Silent Data Loss**: Any columns listed *after* a column with a parenthesis constraint are completely ignored and never created.
  * **E2E Test Failures**: This is the root cause of the 4 failures in `live-headed.spec.ts` (e.g., `live-headed: right panel generate insert`). The test table `rp_gen_insert_e2e` is created with `name VARCHAR(255)` as the second column, which causes the subsequent columns (`score DECIMAL(10,2)`, `active BOOLEAN`, `created_at TIMESTAMP`) to be completely lost.
  * **Malformed Datatype**: The datatype of the affected column is stored in the catalog without the closing parenthesis, e.g., `"VARCHAR ( 255"`.
* **Recommendation**:
  Implement parenthetical depth tracking inside the inner datatype loop. Do not break when seeing `)` unless the parentheses depth is zero. When `paren_depth > 0`, consume the `)` token, decrement the depth, append the symbol, and advance `pos`.

---

### ISSUE-02: Sidebar UI Class Name Collision
* **Component/Layer**: `ui/voltnuerongrid-studio/src/components/Sidebar/UsersPanel.tsx` & `ConnectionList.tsx`
* **Severity**: **MAJOR**
* **Description**:
  Both the Connections list component and the Users & Roles panel component use the exact same CSS class name `.conn-item` for their respective list elements.
* **Root Cause & Code Reference**:
  * `ConnectionList.tsx` (line 58): `<div className={`conn-item ${c.id === activeId ? "active" : ""}`} ...>`
  * `UsersPanel.tsx` (line 133): `<div key={u.user_id} className="conn-item" ...>`
  * `UsersPanel.tsx` (line 165): `<div key={r} className="conn-item" ...>`
* **Implications & Impact**:
  * **E2E Test Timeout**: Playwright tests (e.g. `resource-modal.spec.ts` and `context-menu.spec.ts`) locate target user rows in the sidebar using `.conn-item`. When connections exist, `page.locator(".conn-item").first()` resolves to the connection list item rather than the user item in the Users panel.
  * **Incorrect Context Menu**: The E2E tests right-click on the connection item, opening the Connection context menu instead of the User context menu. This leads to test timeouts and failures because the tests search for and fail to find items like `"Drop User…"` or `"Grant Role…"`.
* **Recommendation**:
  Differentiate UI elements by using unique, semantic CSS classes. Change the class names in `UsersPanel.tsx` to `user-item` and `role-item`. Update global stylesheets and update all target selectors in the E2E test suites accordingly.

---

### ISSUE-03: Trigger Execution Engine Deficit
* **Component/Layer**: `crates/voltnuerongrid-store/src/trigger_emitter.rs` & `triggers.rs`
* **Severity**: **CRITICAL**
* **Description**:
  Although the database catalog successfully registers triggers and DDL events in-memory, the engine lacks any integration hooks to invoke them during query and transaction processing.
* **Root Cause & Code Reference**:
  The `TriggerEmitter::emit` method is fully implemented for logging (`LoggingTriggerEmitter`) and stubs, but it is *never* called or instantiated anywhere in the active transactional or storage pipelines (`voltnuerongrid-exec` or `voltnuerongrid-store`).
* **Implications & Impact**:
  * **Non-Functional Triggers**: Triggers are purely static catalog placeholders. Any `BEFORE` or `AFTER` write trigger will silently fail to execute when `INSERT`, `UPDATE`, or `DELETE` statements are processed.
  * **RDBMS Disqualification**: The system fails a core requirement of relational database systems (ACID side-effect execution).
* **Recommendation**:
  Inject the `TriggerRegistry` and `TriggerEmitter` into the MVCC transaction commit pipeline. When writes are committed, evaluate matching triggers and run their associated functions in the execution engine context.

---

### ISSUE-04: Fragile Updatable Views Implementation
* **Component/Layer**: `services/voltnuerongridd/src/handlers/sql.rs` (`rewrite_dml_for_view`) & `helpers/sql_parse.rs`
* **Severity**: **MAJOR**
* **Description**:
  View updatability is implemented as a simple string-substitution heuristic in the API handler layer, which is highly limited and fragile.
* **Root Cause & Code Reference**:
  * `sql.rs` (lines 2970-2993) performs a case-insensitive string rewrite that replaces the *first occurrence* of the view name with the underlying table name:
    ```rust
    if let Some(pos) = lower.find(view_name.as_str()) {
        let end = pos + view_name.len();
        return format!("{}{}{}", &sql[..pos], base_table, &sql[end..]);
    }
    ```
* **Implications & Impact**:
  * **Limited Updatability**: Any views that include `JOIN`, `GROUP BY`, `HAVING`, `DISTINCT`, aggregate functions, or subqueries are correctly rejected as non-updatable, but even valid updatable views can easily fail.
  * **Fragile Rewriting**: Since it is a text-level replace of the first match, if the view name appears in a string literal or comment before the actual target table identifier, the SQL query will be corrupted, resulting in query failures or unintended execution paths.
* **Recommendation**:
  Replace text-level query rewriting with logical plan-level AST expansion during query analysis and planning in `voltnuerongrid-sql`. This ensures proper semantic isolation and robustness.

---

### ISSUE-05: Lack of User-Defined Function (UDF) Execution Engine
* **Component/Layer**: `crates/voltnuerongrid-exec-datafusion/src/lib.rs` & `crates/voltnuerongrid-store/src/ddl_catalog.rs`
* **Severity**: **MAJOR**
* **Description**:
  Users can successfully declare functions using `CREATE FUNCTION` syntax, but they are catalog-only metadata placeholders.
* **Root Cause & Code Reference**:
  The `ddl_catalog` parser registers `object_kind = "function"`, but the query execution planes (transactional OLTP and DataFusion-based OLAP) do not register these functions into the SQL expression execution runner.
* **Implications & Impact**:
  * **Inoperative Functions**: Any SQL queries referencing user-defined functions will result in planning or evaluation errors because the execution engines do not know how to compile or run the custom function bodies.
* **Recommendation**:
  Implement a UDF registry within the analytical execution crate that maps catalog functions to dynamic DataFusion ScalarUDF or TableUDF implementations, compiling SQL function bodies into executable logical sub-plans.

---

### ISSUE-06: Heuristic HTAP Query Routing Consistency Gap
* **Component/Layer**: `crates/voltnuerongrid-exec/src/lib.rs` (`HtapQueryRouter`)
* **Severity**: **MAJOR**
* **Description**:
  Routing of SELECT statements to either the OLTP transactional row store or the OLAP analytical engine is determined by simple keyword scanning.
* **Root Cause & Code Reference**:
  `lib.rs` (lines 58-79) checks if the SELECT query has simple string patterns:
  ```rust
  if upper.contains("GROUP BY")
      || upper.contains("JOIN")
      || upper.contains("HAVING")
      || upper.contains("OVER(")
      || upper.contains("SUM(")
      || upper.contains("COUNT(")
      // ...
  ```
* **Implications & Impact**:
  * **Incorrect Routing**: A query scanning large datasets without analytical keywords (e.g. `SELECT * FROM large_table`) is incorrectly routed to the OLTP store, degrading OLTP performance.
  * **Consistency Gaps**: Since analytical queries run in the OLAP engine and mutations run in the OLTP engine, changes are synchronized asynchronously via `InMemoryReplicationTransport`. An OLAP query immediately following an OLTP update might read stale data due to replication lag, violating strict RDBMS read-your-writes consistency guarantees.
* **Recommendation**:
  Transition query routing from string keyword matching to cost-based optimizer decisions using the `StatsRegistry` and schema metadata. Implement read-barrier synchronization to ensure OLAP reads await the commit sequence of prior OLTP transactions.

---

## 2. Issues Summary Table

| Issue ID | Layer / Component | Severity | Description | Core Recommendation |
| :--- | :--- | :--- | :--- | :--- |
| **ISSUE-01** | `crates/voltnuerongrid-sql` | **CRITICAL** | Data type parenthesis bug: column parser breaks outer column list parsing upon seeing `)` in length constraints like `VARCHAR(255)`. | Track parentheses depth during data type token consumption; only break loop at depth 0. |
| **ISSUE-02** | `ui/voltnuerongrid-studio` | **MAJOR** | CSS class name collision: both connection items and user/role rows use `.conn-item`, causing Playwright E2E failures. | Rename user and role rows in `UsersPanel.tsx` to `.user-item` and `.role-item` respectively. |
| **ISSUE-03** | `crates/voltnuerongrid-store` | **CRITICAL** | Trigger execution framework deficit: registered triggers are catalog-only mock placeholders and never invoked. | Integrate trigger execution hooks in the MVCC transaction commit or RocksDB storage paths. |
| **ISSUE-04** | `services/voltnuerongridd` | **MAJOR** | Fragile updatable views: simple first-occurrence substring replace rewrites DML targeting views, which is limited and bug-prone. | Move view expansion to the query planner using AST rewrites rather than text substitution in the HTTP handlers. |
| **ISSUE-05** | `crates/voltnuerongrid-exec` | **MAJOR** | Lack of UDF runtime execution: registered functions cannot be evaluated in SQL statements. | Connect catalog function definitions to a DataFusion ScalarUDF/TableUDF registration pipeline. |
| **ISSUE-06** | `crates/voltnuerongrid-exec` | **MAJOR** | Keyword heuristic HTAP query routing can route analytical scans to OLTP and cause dirty/stale reads on OLAP. | Upgrade query routing to use a cost-based planner and implement read-barrier replication consistency. |

---

## 3. Systems Validation Assessment (ACID & Persistence)

Despite the gaps detailed above, VoltNueronGrid implements several standard RDBMS features correctly:

* **Transactional Persistence**: Transaction durability is guaranteed via RocksDB. Mutations are packed into atomic `rocksdb::WriteBatch` transactions across three core column families: `cf_wal` (write-ahead log), `cf_meta` (catalog and checkpoints), and dynamic DB-isolated row column families (e.g. `rows_db`), successfully preventing table cross-contamination.
* **ACID WAL Durability**: By enabling `VNG_WAL_FSYNC_ON_COMMIT`, every commit executes an honest `fsync(2)` checkpoint. The engine automatically handles transactional log replay during recovery upon server startup.
* **Write-Write Conflict Detection**: The MVCC engine in `crates/voltnuerongrid-store/src/mvcc.rs` utilizes a thread-safe `write_intents` map to record lock intents per row key. If concurrent transactions attempt to write to the same key, a conflict is detected immediately and returned to the caller, preventing dirty writes.
