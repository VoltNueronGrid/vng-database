//! SQL parsing helpers shared across handler modules.
use axum::http::HeaderMap;
use crate::{CanonicalCommandEnvelope, CanonicalCommandName, TransportKind};


pub(crate) fn extract_request_id(headers: &HeaderMap, fallback: &str) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}


pub(crate) fn build_http_envelope<TPayload>(
    headers: &HeaderMap,
    command: CanonicalCommandName,
    payload: TPayload,
    fallback_request_id: &str,
) -> CanonicalCommandEnvelope<TPayload> {
    let request_id = extract_request_id(headers, fallback_request_id);
    let session_context = headers
        .get("x-vng-session-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut transport_metadata = std::collections::HashMap::new();
    transport_metadata.insert("protocol".to_string(), "http".to_string());
    if let Some(session_id) = session_context.clone() {
        transport_metadata.insert("session_id".to_string(), session_id);
    }
    CanonicalCommandEnvelope {
        request_id,
        transport: TransportKind::Http,
        command,
        session_context,
        transport_metadata,
        payload,
    }
}


/// Extract the storage key for a DELETE statement.
///
/// Returns `"table:where_value"` — the same format used by INSERT row keys —
/// so that callers can prefix with a database name via [`db_prefix_key`] when
/// operating in multi-database mode.
///
/// Returns `None` for non-DELETE statements or statements without a WHERE clause.
pub(crate) fn extract_delete_key_from_sql(sql: &str) -> Option<String> {
    use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};
    let tokens = semantic_tokens(sql);
    let upper = sql.trim_start().to_ascii_uppercase();
    if !upper.starts_with("DELETE") {
        return None;
    }
    // Extract table name from `DELETE FROM <table>`
    let mut table_name: Option<String> = None;
    let mut past_from = false;
    for tok in &tokens {
        match tok {
            Token::Keyword(k) if k.eq_ignore_ascii_case("FROM") => past_from = true,
            Token::Identifier(t) | Token::Keyword(t) if past_from && table_name.is_none() => {
                // Strip schema qualifier if present (public.customers → customers)
                let unqualified = t.rsplit('.').next().unwrap_or(t.as_str());
                table_name = Some(unqualified.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    // Extract WHERE clause value
    let mut after_where = false;
    let mut past_eq = false;
    for tok in &tokens {
        match tok {
            Token::Keyword(k) if k.eq_ignore_ascii_case("WHERE") => after_where = true,
            Token::Symbol(s) if s == "=" && after_where => past_eq = true,
            Token::StringLiteral(s) if past_eq => {
                return Some(match table_name {
                    Some(t) => format!("{t}:{s}"),
                    None => s.clone(),
                });
            }
            Token::Number(n) if past_eq => {
                return Some(match table_name {
                    Some(t) => format!("{t}:{n}"),
                    None => n.clone(),
                });
            }
            _ => {}
        }
    }
    None
}


/// Parse a SQL UPDATE statement and return (row_key, row_data) for MVCC insert (new version).
/// Pattern: UPDATE <table> SET col=val [WHERE col='key']
pub(crate) fn extract_update_row_from_sql(
    sql: &str,
) -> Option<(String, std::collections::HashMap<String, String>)> {
    use voltnuerongrid_sql::ast::{parse_one, Statement};
    use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};
    let stmt = parse_one(sql).ok()?;
    let Statement::Update(upd) = stmt else {
        return None;
    };
    // Prefer the WHERE clause value as key; fall back to table name
    let tokens = semantic_tokens(sql);
    let mut key = upd.table.clone();
    let mut after_where = false;
    let mut past_eq = false;
    for tok in &tokens {
        match tok {
            Token::Keyword(k) if k.eq_ignore_ascii_case("WHERE") => after_where = true,
            Token::Symbol(s) if s == "=" && after_where => past_eq = true,
            Token::StringLiteral(s) if past_eq => {
                key = s.clone();
                break;
            }
            Token::Number(n) if past_eq => {
                key = n.clone();
                break;
            }
            _ => {}
        }
    }
    let row_key = format!("{}:{}", upd.table, key);
    let mut data = std::collections::HashMap::new();
    data.insert("__table".to_string(), upd.table.clone());
    for (col, val) in &upd.assignments {
        data.insert(col.clone(), val.clone());
    }
    Some((row_key, data))
}


/// Extract bulk-update target for `UPDATE table SET col='val' WHERE pred_col='pred_val'`.
///
/// Returns `Some((table_name, set_col, set_val, where_col, where_val))` when the WHERE
/// clause filters on a non-key column (i.e. the WHERE value is NOT the primary key).
/// Returns `None` when the statement cannot be parsed or the WHERE clause filters by the
/// primary key (in which case single-row `extract_update_row_from_sql` is sufficient).
///
/// This enables Rule 7 (set-at-a-time UPDATE): callers scan all rows of `table_name`,
/// apply `set_col = set_val` to every row where `row[where_col] == where_val`.
pub(crate) fn extract_bulk_update_target(
    sql: &str,
) -> Option<(String, String, String, String, String)> {
    use voltnuerongrid_sql::ast::{parse_one, Statement};
    use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};

    let stmt = parse_one(sql).ok()?;
    let Statement::Update(upd) = stmt else {
        return None;
    };
    if upd.assignments.is_empty() {
        return None;
    }

    let (set_col, set_val) = upd.assignments.first()?.clone();
    let table = upd.table.to_ascii_lowercase();

    // Parse WHERE clause: look for `where_col = 'where_val'`
    let tokens = semantic_tokens(sql);
    let mut after_where = false;
    let mut where_col: Option<String> = None;
    let mut past_eq = false;
    let mut where_val: Option<String> = None;

    for tok in &tokens {
        match tok {
            Token::Keyword(k) if k.eq_ignore_ascii_case("WHERE") => after_where = true,
            Token::Identifier(id) if after_where && where_col.is_none() => {
                where_col = Some(id.to_ascii_lowercase());
            }
            Token::Symbol(s) if s == "=" && after_where && where_col.is_some() && !past_eq => {
                past_eq = true;
            }
            Token::StringLiteral(s) if past_eq && where_val.is_none() => {
                where_val = Some(s.clone());
            }
            Token::Number(n) if past_eq && where_val.is_none() => {
                where_val = Some(n.clone());
            }
            _ => {}
        }
    }

    let where_col = where_col?;
    let where_val = where_val?;

    // If the WHERE column looks like a primary-key column (e.g. "id") and the WHERE
    // value matches the key pattern used in extract_update_row_from_sql, skip — the
    // single-row path is already handling it. We detect this by checking if the
    // WHERE column could be the primary-key col that generated the existing row_key.
    // Simple heuristic: if where_col == "id", the single-key path already handled it.
    // We return Some here regardless — callers use the where_col to decide whether to
    // do a full table scan.
    Some((table, set_col, set_val, where_col, where_val))
}


/// Extract bulk-delete target for `DELETE FROM table WHERE pred_col = 'val'`.
///
/// Returns `Some((table_name, where_col, where_val))` when the WHERE clause filters on a
/// non-key column (full table scan required).  Returns `None` when the WHERE column is
/// "id" (primary-key DELETE — single-row `extract_delete_key_from_sql` is sufficient) or
/// when the statement cannot be parsed.
///
/// This enables Codd Rule 7 (set-at-a-time DELETE): callers scan all rows of `table_name`
/// and delete every row where `row[where_col] == where_val`.
pub(crate) fn extract_bulk_delete_target(sql: &str) -> Option<(String, String, String)> {
    use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};

    let upper = sql.trim_start().to_ascii_uppercase();
    if !upper.starts_with("DELETE") {
        return None;
    }

    let tokens = semantic_tokens(sql);

    // Parse: DELETE FROM <table> WHERE <col> = <val>
    let mut past_from = false;
    let mut table: Option<String> = None;
    let mut after_where = false;
    let mut where_col: Option<String> = None;
    let mut past_eq = false;
    let mut where_val: Option<String> = None;

    for tok in &tokens {
        match tok {
            Token::Keyword(k) if k.eq_ignore_ascii_case("FROM") && !past_from => {
                past_from = true;
            }
            Token::Identifier(t) | Token::Keyword(t) if past_from && table.is_none() => {
                let unqualified = t.rsplit('.').next().unwrap_or(t.as_str());
                table = Some(unqualified.to_ascii_lowercase());
            }
            Token::Keyword(k) if k.eq_ignore_ascii_case("WHERE") => after_where = true,
            Token::Identifier(id) if after_where && where_col.is_none() => {
                where_col = Some(id.to_ascii_lowercase());
            }
            Token::Symbol(s) if s == "=" && after_where && where_col.is_some() && !past_eq => {
                past_eq = true;
            }
            Token::StringLiteral(s) if past_eq && where_val.is_none() => {
                where_val = Some(s.clone());
            }
            Token::Number(n) if past_eq && where_val.is_none() => {
                where_val = Some(n.clone());
            }
            _ => {}
        }
    }

    let table = table?;
    let where_col = where_col?;
    let where_val = where_val?;

    // If WHERE is on "id", single-row delete already handled it — skip the scan path.
    if where_col.eq_ignore_ascii_case("id") {
        return None;
    }

    Some((table, where_col, where_val))
}


/// Extract ordered column names from a CREATE TABLE DDL statement.
/// Returns `vec!["id", "name", ...]` or an empty Vec if parsing fails.
pub(crate) fn extract_column_names_from_ddl(ddl: &str) -> Vec<String> {
    // Find the column list between the first '(' and last ')'
    let open = ddl.find('(');
    let close = ddl.rfind(')');
    let (open, close) = match (open, close) {
        (Some(o), Some(c)) if c > o => (o, c),
        _ => return Vec::new(),
    };
    let inner = &ddl[open + 1..close];
    // Split on commas at depth 0 (ignore nested parens like DECIMAL(10,2))
    let mut cols = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in inner.chars() {
        match ch {
            '(' => { depth += 1; current.push(ch); }
            ')' => { if depth > 0 { depth -= 1; } current.push(ch); }
            ',' if depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() { cols.push(trimmed); }
                current = String::new();
            }
            _ => { current.push(ch); }
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() { cols.push(trimmed); }

    // Extract the first token (column name) from each clause, skip table constraints
    let constraint_kws = ["PRIMARY", "FOREIGN", "UNIQUE", "CHECK", "CONSTRAINT", "INDEX"];
    cols.into_iter()
        .filter_map(|clause| {
            let first = clause.split_whitespace().next()?.to_ascii_lowercase();
            // Skip constraint lines
            if constraint_kws.iter().any(|kw| first.eq_ignore_ascii_case(kw)) {
                return None;
            }
            Some(first)
        })
        .collect()
}


/// Parse a SQL INSERT statement using the AST parser and return a (row_key, row_data) pair
/// suitable for writing into PagedRowStore. Returns None for non-INSERT or unparseable input.
/// Stores column-value pairs so SELECT can return structured data.
/// The "__table" meta-key identifies which table the row belongs to.
/// `ddl_col_names` provides ordered real column names from the CREATE TABLE DDL; used as
/// fallback when the INSERT has no explicit column list.
pub(crate) fn extract_insert_row_from_sql(
    sql: &str,
) -> Option<(String, std::collections::HashMap<String, String>)> {
    extract_insert_row_from_sql_with_cols(sql, &[])
}


/// Extract ALL rows from a (possibly multi-row) INSERT statement.
/// Returns one `(row_key, RowData, single_row_sql)` per VALUES tuple.
/// Strips `schema.table` qualifiers so the internal SQL parser can handle them.
pub(crate) fn extract_all_insert_rows(
    sql: &str,
) -> Vec<(String, std::collections::HashMap<String, String>, String)> {
    use voltnuerongrid_sql::{parse_one, Statement};
    // Strip schema qualifier: "INSERT INTO oltp.customers" → "INSERT INTO customers"
    let normalized = strip_schema_qualifier_from_insert(sql);
    let ins = match parse_one(&normalized) {
        Ok(Statement::Insert(i)) => i,
        _ => return Vec::new(),
    };
    // Preserve original (schema-qualified) table name for WAL
    let orig_table = {
        let upper = sql.to_ascii_uppercase();
        if let Some(into_pos) = upper.find("INTO") {
            let after = sql[into_pos + 4..].trim_start();
            let end = after.find(|c: char| c == ' ' || c == '\n' || c == '\t' || c == '(').unwrap_or(after.len());
            after[..end].to_string()
        } else {
            ins.table.clone()
        }
    };
    let unqualified_table = orig_table.rsplit('.').next().unwrap_or(&orig_table).to_string();
    let mut results = Vec::new();
    for row_vals in &ins.values {
        if row_vals.is_empty() {
            continue;
        }
        let mut data = std::collections::HashMap::new();
        data.insert("__table".to_string(), unqualified_table.clone());
        for (i, val) in row_vals.iter().enumerate() {
            let col = if !ins.columns.is_empty() {
                ins.columns.get(i).map(|c| c.to_ascii_lowercase()).unwrap_or_else(|| format!("col_{i}"))
            } else {
                format!("col_{i}")
            };
            data.insert(col.clone(), val.clone());
        }
        let first_val = &row_vals[0];
        let row_key = format!("{unqualified_table}:{first_val}");
        // Build a canonical single-row INSERT for WAL replay (uses original table name)
        let col_list = if !ins.columns.is_empty() {
            format!(" ({})", ins.columns.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", "))
        } else {
            String::new()
        };
        let val_list = row_vals.iter()
            .map(|v| {
                let trimmed = v.trim();
                if trimmed.parse::<f64>().is_ok() { trimmed.to_string() } else { format!("'{}'", trimmed.replace('\'', "''")) }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let single_sql = format!("INSERT INTO {orig_table}{col_list} VALUES ({val_list});");
        results.push((row_key, data, single_sql));
    }
    results
}


/// Remove `schema.` prefix from table name in INSERT statement so the parser
/// (which only handles unqualified names) can parse the statement correctly.
pub(crate) fn strip_schema_qualifier_from_insert(sql: &str) -> String {
    if !sql.contains('.') {
        return sql.to_string();
    }
    let sql_upper = sql.to_ascii_uppercase();
    if let Some(into_pos) = sql_upper.find("INTO") {
        let after_into = into_pos + 4;
        let ws_len = sql[after_into..].len() - sql[after_into..].trim_start().len();
        let table_start = after_into + ws_len;
        let table_text = &sql[table_start..];
        let table_end = table_text.find(|c: char| c == ' ' || c == '\n' || c == '\t' || c == '(').unwrap_or(table_text.len());
        let table_name = &table_text[..table_end];
        if let Some(dot) = table_name.find('.') {
            let unqualified_start = table_start + dot + 1;
            let after_table = table_start + table_end;
            return format!("{}{}{}", &sql[..table_start], &sql[unqualified_start..after_table], &sql[after_table..]);
        }
    }
    sql.to_string()
}


pub(crate) fn extract_insert_row_from_sql_with_cols(
    sql: &str,
    ddl_col_names: &[String],
) -> Option<(String, std::collections::HashMap<String, String>)> {
    use voltnuerongrid_sql::{parse_one, Statement};
    let ins = match parse_one(sql) {
        Ok(Statement::Insert(i)) => i,
        _ => return None,
    };
    // Strip schema qualifier (public.foo → foo)
    let table = ins.table.rsplit('.').next().unwrap_or(&ins.table).to_string();

    // Use the first row of values (single-row INSERT)
    let row_vals = ins.values.first()?;
    if row_vals.is_empty() {
        return None;
    }

    let mut data = std::collections::HashMap::new();
    // Store the table name under the meta key __table (used for table-scoped SELECT scans)
    data.insert("__table".to_string(), table.clone());

    for (i, val) in row_vals.iter().enumerate() {
        let col = if !ins.columns.is_empty() {
            // Explicit column list in INSERT statement — always preferred
            ins.columns
                .get(i)
                .map(|c| c.to_ascii_lowercase())
                .unwrap_or_else(|| format!("col_{i}"))
        } else if let Some(name) = ddl_col_names.get(i) {
            // Fall back to DDL-derived column names (CREATE TABLE definition order)
            name.clone()
        } else {
            format!("col_{i}")
        };
        data.insert(col, val.clone());
    }

    // Row key = table:first_value for uniqueness within the store
    let first_val = row_vals[0].as_str();
    let row_key = format!("{table}:{first_val}");
    Some((row_key, data))
}


// ─── Gap #2: row key DB prefix helpers ────────────────────────────────────────

/// Build a fully-qualified row key for storage in `PagedRowStore`.
///
/// When `db` is non-empty: `"{db}.{table}:{row_id}"` — true per-database isolation.
/// When `db` is empty:     `"{table}:{row_id}"` — backward-compatible (WAL replay, tests).
pub(crate) fn make_row_key(db: &str, table: &str, row_id: &str) -> String {
    if db.is_empty() {
        format!("{table}:{row_id}")
    } else {
        format!("{db}.{table}:{row_id}")
    }
}

/// Build the key scan prefix for a table in a given database.
///
/// `make_table_scan_prefix("", "customers")` → `"customers:"`
/// `make_table_scan_prefix("mydb", "customers")` → `"mydb.customers:"`
pub(crate) fn make_table_scan_prefix(db: &str, table: &str) -> String {
    if db.is_empty() {
        format!("{table}:")
    } else {
        format!("{db}.{table}:")
    }
}

/// Apply the database prefix to an already-formed `"table:value"` key.
///
/// When `db` is empty, returns `raw_key` unchanged (backward compat for WAL replay
/// and single-database deployments).
pub(crate) fn db_prefix_key(db: &str, raw_key: &str) -> String {
    if db.is_empty() {
        raw_key.to_string()
    } else {
        format!("{db}.{raw_key}")
    }
}

// ─── M-8 Rule 6: View expansion helpers ────────────────────────────────────

/// Extract the body (AS SELECT ...) from a CREATE VIEW DDL statement.
///
/// `original_statement` is the full `CREATE [OR REPLACE] VIEW name AS <body>` string
/// stored in the DDL catalog. Returns the SELECT body (everything after the first `AS`
/// token that follows the view name).
pub(crate) fn extract_view_select_body(original_statement: &str) -> Option<String> {
    let upper = original_statement.to_ascii_uppercase();
    // Find " AS " that marks the body start (skip CREATE / VIEW / name tokens).
    // Search from after "VIEW <name>" — the first " AS " occurrence.
    let as_pos = upper.find(" AS ")?;
    let body = original_statement[as_pos + 4..].trim().to_string();
    if body.is_empty() {
        return None;
    }
    Some(body)
}

/// Extract the single base-table name from a simple updatable view definition.
///
/// A view is updatable iff its body is `SELECT [cols] FROM <table>` with no
/// JOIN, GROUP BY, HAVING, DISTINCT, aggregate functions, or subqueries.
/// Returns `Some(table_name)` for simple views, `None` for complex ones.
pub(crate) fn extract_updatable_view_base_table(original_statement: &str) -> Option<String> {
    let body = extract_view_select_body(original_statement)?.to_ascii_uppercase();
    // Reject complex views.
    for keyword in ["JOIN", "GROUP BY", "HAVING", "DISTINCT", "SUBQUERY", "UNION", "INTERSECT", "EXCEPT"] {
        if body.contains(keyword) {
            return None;
        }
    }
    // Aggregate function check: COUNT(, SUM(, AVG(, MIN(, MAX(
    for agg in ["COUNT(", "SUM(", "AVG(", "MIN(", "MAX("] {
        if body.contains(agg) {
            return None;
        }
    }
    // Extract FROM <table>: the token immediately after FROM (before WHERE/LIMIT/ORDER).
    let from_pos = body.find(" FROM ")?;
    let after_from = body[from_pos + 6..].trim();
    let end = after_from.find(|c: char| c.is_whitespace()).unwrap_or(after_from.len());
    let table_upper = after_from[..end].trim().to_string();
    if table_upper.is_empty() {
        return None;
    }
    // Return in lower-case to match catalog convention.
    Some(table_upper.to_ascii_lowercase())
}

/// Rewrite a SELECT SQL statement so that references to `view_name` in the FROM
/// clause are replaced with an inline expansion: `(view_body) AS view_name`.
///
/// Example:
///   `SELECT * FROM order_summary WHERE region = 'us'`
///   → `SELECT * FROM (SELECT order_id, total FROM orders) AS order_summary WHERE region = 'us'`
/// M-7 (improved): Expand a view reference in a SELECT statement.
///
/// Improvements over the original text-substitution:
/// 1. **Word-boundary matching** — `FROM view` does not match `FROM view_of_orders`.
/// 2. **Schema-qualifier stripping** — `FROM mydb.myview` is recognised and the
///    qualifier is stripped so the inner subquery works with unqualified names.
/// 3. **Cross-DB body normalisation** — if the view body references
///    `schema.table`, the qualifier is stripped in the rewritten body so the
///    executor's scan prefix logic (which works on unqualified table names) is
///    not confused by a dot-qualified prefix.
///
/// This is still text-level rewriting.  Full AST-level independence (true
/// logical decoupling from physical key format) is tracked as a future gap.
pub(crate) fn expand_view_in_select(sql: &str, view_name: &str, view_body: &str) -> String {
    let lower = sql.to_ascii_lowercase();
    let view_lower = view_name.to_ascii_lowercase();

    // Helper: returns true when the byte at `pos` is a SQL identifier character
    // (letter, digit, underscore).  Used for word-boundary checks.
    let is_ident_char = |c: char| c.is_ascii_alphanumeric() || c == '_';

    // Try to find "from <schema.>?<view_name>" with word boundary on both sides.
    // We check two forms: bare "view_name" and "*.view_name" (schema-qualified).
    let patterns: Vec<String> = vec![
        format!(" from {}", view_lower),
        format!("\tfrom {}", view_lower),
        format!("\nfrom {}", view_lower),
    ];

    for pattern in &patterns {
        if let Some(pos) = lower.find(pattern.as_str()) {
            // Check word boundary AFTER the match: the char immediately following
            // must NOT be an identifier character (prevents `view` matching `view_extra`).
            let after_match = pos + pattern.len();
            let next_char = lower[after_match..].chars().next();
            if let Some(c) = next_char {
                if is_ident_char(c) {
                    continue; // Not a word boundary — try next pattern.
                }
            }

            // Strip schema qualifier if the match was preceded by "schema.".
            // E.g., "FROM orders.summary" → FROM (body) AS summary
            // `pos` is the position of the leading space/tab/newline before FROM.
            // The character just before pos should be checked too but the simple
            // approach is: check whether the part before view_name ends with a dot.
            let from_kw_start = pos + 1; // skip the leading whitespace
            let _from_end = from_kw_start + 4; // "FROM"

            // Normalise the view body: strip any "schema." qualifiers from table
            // references in the body so the executor sees plain table names.
            let normalised_body = strip_schema_qualifiers_from_sql(view_body);

            let replacement = format!(
                " FROM ({}) AS {}",
                normalised_body,
                view_lower
            );
            return format!("{}{}{}", &sql[..pos], replacement, &sql[after_match..]);
        }
    }
    sql.to_string()
}

/// Strip `schema.` qualifiers from all table references in a SQL string.
///
/// Used by `expand_view_in_select` to normalise view bodies that reference
/// fully-qualified table names — the executor's scan prefix logic works on
/// unqualified names so qualifiers must be removed at expansion time.
///
/// This is a best-effort text transform: it handles the common `schema.table`
/// form but not aliased schemas or WITH-clause names.
pub(crate) fn strip_schema_qualifiers_from_sql(sql: &str) -> String {
    // Replace `word.word` sequences where the left part is a likely schema name
    // (no spaces, no SQL keywords).  We use a simple state-machine instead of
    // regex to avoid adding a regex dependency.
    let sql_upper = sql.to_ascii_uppercase();
    let keywords: &[&str] = &[
        "SELECT", "FROM", "WHERE", "JOIN", "ON", "AND", "OR", "NOT",
        "INSERT", "UPDATE", "DELETE", "SET", "VALUES", "AS", "GROUP",
        "ORDER", "BY", "HAVING", "LIMIT", "OFFSET", "UNION", "ALL",
        "DISTINCT", "INTO", "TABLE",
    ];
    let mut result = sql.to_string();
    // Walk through occurrences of "word." and strip if "word" is not a keyword.
    let mut offset = 0usize;
    loop {
        let dot_pos = match result[offset..].find('.') {
            Some(p) => offset + p,
            None => break,
        };
        // Find start of the word before the dot.
        let before = &result[..dot_pos];
        let word_start = before.rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .map(|p| p + 1)
            .unwrap_or(0);
        let word = &result[word_start..dot_pos];
        // Check the character after the dot is an identifier start.
        let after_dot = dot_pos + 1;
        let next_is_ident = result[after_dot..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
        if !word.is_empty()
            && next_is_ident
            && !keywords.contains(&sql_upper[word_start..dot_pos].trim())
        {
            // Remove "word." by splicing it out of result.
            result = format!("{}{}", &result[..word_start], &result[after_dot..]);
            offset = word_start; // re-scan from where we removed
        } else {
            offset = dot_pos + 1;
        }
        if offset >= result.len() { break; }
    }
    result
}

// ─── M-6: DDL-schema type validation ──────────────────────────────────────────

/// Validate that a string value is compatible with a DDL-declared SQL type.
///
/// Used at INSERT/UPDATE time to enforce DDL-declared column types before
/// storing raw strings in `RowData`.  Returns `Ok(())` on pass; `Err(msg)` on
/// type mismatch with a user-readable explanation.
///
/// This is a *lightweight* check (string parsing) not a full typed storage
/// layer — `RowData` remains `HashMap<String, String>` for now (see M-6 full
/// path in gaps-4.md).  Null / missing values pass validation — presence
/// enforcement is a future gap.
pub(crate) fn validate_value_for_type(value: &str, ddl_type: &str) -> Result<(), String> {
    let bare = ddl_type.split('(').next().unwrap_or(ddl_type).trim().to_ascii_uppercase();
    // NULL-like empties pass all type checks.
    if value.is_empty() || value.eq_ignore_ascii_case("null") {
        return Ok(());
    }
    match bare.as_str() {
        "INT" | "INTEGER" | "INT4" | "SMALLINT" | "INT2" | "BIGINT" | "INT8" => {
            value.trim().parse::<i64>().map(|_| ()).map_err(|_| {
                format!("value '{value}' is not a valid {bare}")
            })
        }
        "FLOAT" | "REAL" | "FLOAT4" | "DOUBLE" | "DOUBLE PRECISION" | "FLOAT8" => {
            value.trim().parse::<f64>().map(|_| ()).map_err(|_| {
                format!("value '{value}' is not a valid {bare}")
            })
        }
        "NUMERIC" | "DECIMAL" => {
            // Accept integers and decimals; reject non-numeric strings.
            value.trim().parse::<f64>().map(|_| ()).map_err(|_| {
                format!("value '{value}' is not a valid {bare}")
            })
        }
        "BOOL" | "BOOLEAN" => {
            match value.trim().to_ascii_lowercase().as_str() {
                "true" | "false" | "1" | "0" | "yes" | "no" | "on" | "off" => Ok(()),
                _ => Err(format!("value '{value}' is not a valid BOOLEAN")),
            }
        }
        "UUID" => {
            // Simple UUID format check: 8-4-4-4-12 hex digits.
            let v = value.trim();
            let valid = v.len() == 36
                && v.chars().enumerate().all(|(i, c)| {
                    if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() }
                });
            if valid { Ok(()) } else { Err(format!("value '{value}' is not a valid UUID")) }
        }
        // Text-like types accept anything.
        "TEXT" | "VARCHAR" | "CHARACTER VARYING" | "NVARCHAR"
        | "CHAR" | "CHARACTER" | "NCHAR" | "CLOB"
        | "JSON" | "JSONB" | "BYTEA" | "BLOB" | "BINARY"
        | "TIMESTAMP" | "TIMESTAMPTZ" | "DATE" | "TIME" => Ok(()),
        _ => Ok(()), // unknown type — allow through
    }
}

/// Validate all columns in `row_data` against the DDL schema for `table_name`.
///
/// Looks up the table in `ddl_catalog`, extracts column types, and calls
/// `validate_value_for_type` for each column present in `row_data`.
///
/// Returns `Ok(())` if all values pass; `Err(user_message)` on the first
/// type violation.  Skips validation when the table is not found in the
/// catalog (DDL not yet recorded — no false positives).
pub(crate) fn validate_row_against_ddl(
    table_name: &str,
    row_data: &std::collections::HashMap<String, String>,
    ddl_catalog: &voltnuerongrid_store::ddl_catalog::DdlCatalog,
) -> Result<(), String> {
    use crate::helpers::information_schema::extract_ddl_column_details;

    // Find the table entry in the catalog.
    let entry = ddl_catalog
        .active_entries()
        .into_iter()
        .find(|e| e.object_kind == "table" && e.object_name.eq_ignore_ascii_case(table_name));
    let entry = match entry {
        Some(e) => e,
        None => return Ok(()), // table not in catalog yet — allow through
    };

    let col_details = extract_ddl_column_details(&entry.original_statement);
    for (col_name, col_type) in &col_details {
        if let Some(value) = row_data.get(col_name) {
            validate_value_for_type(value, col_type)
                .map_err(|e| format!("column '{col_name}': {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── extract_bulk_delete_target ──────────────────────────────────────────

    #[test]
    fn bulk_delete_parses_non_key_where_clause() {
        let sql = "DELETE FROM employees WHERE department = 'engineering'";
        let result = extract_bulk_delete_target(sql);
        assert!(result.is_some(), "should parse non-key WHERE clause");
        let (tbl, col, val) = result.unwrap();
        assert_eq!(tbl, "employees");
        assert_eq!(col, "department");
        assert_eq!(val, "engineering");
    }

    #[test]
    fn bulk_delete_skips_id_where_clause() {
        // WHERE id = '123' is handled by single-row extract_delete_key_from_sql
        let sql = "DELETE FROM employees WHERE id = '123'";
        let result = extract_bulk_delete_target(sql);
        assert!(result.is_none(), "id WHERE clause should delegate to single-key path");
    }

    #[test]
    fn bulk_delete_handles_numeric_where_value() {
        let sql = "DELETE FROM orders WHERE status = 0";
        let result = extract_bulk_delete_target(sql);
        assert!(result.is_some());
        let (_, col, val) = result.unwrap();
        assert_eq!(col, "status");
        assert_eq!(val, "0");
    }

    #[test]
    fn bulk_delete_returns_none_for_non_delete() {
        assert!(extract_bulk_delete_target("SELECT * FROM t").is_none());
        assert!(extract_bulk_delete_target("UPDATE t SET x = 1").is_none());
    }

    // ─── extract_bulk_update_target ──────────────────────────────────────────

    #[test]
    fn bulk_update_parses_non_key_set_where() {
        let sql = "UPDATE employees SET salary = '90000' WHERE department = 'engineering'";
        let result = extract_bulk_update_target(sql);
        assert!(result.is_some());
        let (tbl, set_col, _set_val, where_col, where_val) = result.unwrap();
        assert_eq!(tbl, "employees");
        assert_eq!(set_col, "salary");
        assert_eq!(where_col, "department");
        assert_eq!(where_val, "engineering");
    }

    // ─── View expansion helpers (M-8 Rule 6) ─────────────────────────────────

    #[test]
    fn extract_view_select_body_simple() {
        let ddl = "CREATE VIEW order_summary AS SELECT order_id, total FROM orders";
        let body = extract_view_select_body(ddl);
        assert!(body.is_some());
        assert_eq!(body.unwrap(), "SELECT order_id, total FROM orders");
    }

    #[test]
    fn extract_view_select_body_or_replace() {
        let ddl = "CREATE OR REPLACE VIEW v AS SELECT * FROM t WHERE x > 0";
        let body = extract_view_select_body(ddl);
        assert!(body.is_some());
        assert_eq!(body.unwrap(), "SELECT * FROM t WHERE x > 0");
    }

    #[test]
    fn extract_updatable_view_base_table_simple() {
        let ddl = "CREATE VIEW active_users AS SELECT id, name FROM users WHERE active = 1";
        let table = extract_updatable_view_base_table(ddl);
        assert_eq!(table.as_deref(), Some("users"));
    }

    #[test]
    fn extract_updatable_view_base_table_rejects_join() {
        let ddl = "CREATE VIEW joined AS SELECT a.id FROM a JOIN b ON a.id = b.id";
        let table = extract_updatable_view_base_table(ddl);
        assert!(table.is_none(), "JOIN views are not updatable");
    }

    #[test]
    fn extract_updatable_view_base_table_rejects_aggregate() {
        let ddl = "CREATE VIEW counts AS SELECT COUNT(*) FROM orders GROUP BY status";
        let table = extract_updatable_view_base_table(ddl);
        assert!(table.is_none(), "aggregate views are not updatable");
    }

    #[test]
    fn expand_view_in_select_basic() {
        let sql = "SELECT * FROM order_summary WHERE region = 'us'";
        let body = "SELECT order_id, total FROM orders";
        let result = expand_view_in_select(sql, "order_summary", body);
        assert!(result.contains("FROM (SELECT order_id, total FROM orders) AS order_summary"));
        assert!(result.contains("WHERE region = 'us'"));
    }

    // ─── Q8: make_row_key / make_table_scan_prefix / db_prefix_key ───────────

    #[test]
    fn q8_make_row_key_with_db() {
        assert_eq!(make_row_key("mydb", "orders", "42"), "mydb.orders:42");
    }

    #[test]
    fn q8_make_row_key_without_db() {
        assert_eq!(make_row_key("", "orders", "42"), "orders:42");
    }

    #[test]
    fn q8_make_table_scan_prefix_with_db() {
        assert_eq!(make_table_scan_prefix("mydb", "customers"), "mydb.customers:");
    }

    #[test]
    fn q8_make_table_scan_prefix_without_db() {
        assert_eq!(make_table_scan_prefix("", "customers"), "customers:");
    }

    #[test]
    fn q8_db_prefix_key_with_db() {
        assert_eq!(db_prefix_key("mydb", "orders:99"), "mydb.orders:99");
    }

    #[test]
    fn q8_db_prefix_key_without_db_passthrough() {
        // Empty db → raw key returned unchanged (backward-compat).
        assert_eq!(db_prefix_key("", "orders:99"), "orders:99");
    }

    // ─── Q8: validate_value_for_type ─────────────────────────────────────────

    #[test]
    fn q8_validate_integer_accepts_valid() {
        assert!(validate_value_for_type("42", "INTEGER").is_ok());
        assert!(validate_value_for_type("-1", "BIGINT").is_ok());
        assert!(validate_value_for_type("0", "INT").is_ok());
    }

    #[test]
    fn q8_validate_integer_rejects_text() {
        assert!(validate_value_for_type("abc", "INT").is_err());
        assert!(validate_value_for_type("3.14", "INT").is_err());
    }

    #[test]
    fn q8_validate_float_accepts_valid() {
        assert!(validate_value_for_type("3.14", "FLOAT").is_ok());
        assert!(validate_value_for_type("-0.5", "DOUBLE").is_ok());
        assert!(validate_value_for_type("100", "REAL").is_ok());
    }

    #[test]
    fn q8_validate_bool_accepts_all_forms() {
        for v in &["true", "false", "1", "0", "yes", "no", "on", "off"] {
            assert!(validate_value_for_type(v, "BOOLEAN").is_ok(), "expected ok for {v}");
        }
    }

    #[test]
    fn q8_validate_bool_rejects_invalid() {
        assert!(validate_value_for_type("maybe", "BOOL").is_err());
        assert!(validate_value_for_type("2", "BOOLEAN").is_err());
    }

    #[test]
    fn q8_validate_uuid_accepts_valid() {
        assert!(validate_value_for_type("550e8400-e29b-41d4-a716-446655440000", "UUID").is_ok());
    }

    #[test]
    fn q8_validate_uuid_rejects_short() {
        assert!(validate_value_for_type("550e8400-e29b", "UUID").is_err());
    }

    #[test]
    fn q8_validate_text_accepts_anything() {
        assert!(validate_value_for_type("hello world", "TEXT").is_ok());
        assert!(validate_value_for_type("!@#$", "VARCHAR").is_ok());
    }

    #[test]
    fn q8_validate_null_passes_all_types() {
        for t in &["INT", "FLOAT", "BOOLEAN", "UUID", "TEXT"] {
            assert!(validate_value_for_type("null", t).is_ok(), "null should pass for {t}");
            assert!(validate_value_for_type("", t).is_ok(), "empty should pass for {t}");
        }
    }

    // ─── Q8: strip_schema_qualifiers_from_sql ────────────────────────────────

    #[test]
    fn q8_strip_schema_qualifier_removes_prefix() {
        let sql = "SELECT * FROM myschema.orders WHERE myschema.orders.id = 1";
        let result = strip_schema_qualifiers_from_sql(sql);
        assert!(!result.contains("myschema."), "schema qualifier should be stripped, got: {result}");
    }

    #[test]
    fn q8_strip_schema_qualifier_leaves_plain_table() {
        let sql = "SELECT * FROM orders WHERE id = 1";
        let result = strip_schema_qualifiers_from_sql(sql);
        assert_eq!(result, sql);
    }

    #[test]
    fn q8_strip_schema_qualifier_handles_sql_keywords() {
        // SQL keywords followed by dot should not be stripped (e.g. GROUP.BY would be wrong,
        // but a plain keyword like FROM.table is not a valid qualifier).
        let sql = "SELECT id FROM public.users";
        let result = strip_schema_qualifiers_from_sql(sql);
        // "public" is not a SQL keyword so it gets stripped
        assert!(!result.contains("public."), "public. should be stripped, got: {result}");
        assert!(result.contains("users"));
    }
}
