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
