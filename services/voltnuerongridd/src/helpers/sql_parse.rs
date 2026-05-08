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


pub(crate) fn extract_delete_key_from_sql(sql: &str) -> Option<String> {
    use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};
    let tokens = semantic_tokens(sql);
    let upper = sql.trim_start().to_ascii_uppercase();
    if !upper.starts_with("DELETE") {
        return None;
    }
    let mut after_where = false;
    let mut past_eq = false;
    for tok in &tokens {
        match tok {
            Token::Keyword(k) if k.eq_ignore_ascii_case("WHERE") => after_where = true,
            Token::Symbol(s) if s == "=" && after_where => past_eq = true,
            Token::StringLiteral(s) if past_eq => return Some(s.clone()),
            Token::Number(n) if past_eq => return Some(n.clone()),
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

/// S3-WS1-05: parse a WHERE clause string into `VectorizedFilter` predicates.
/// Handles simple `col op val` expressions joined by ` AND `.
pub(crate) fn parse_where_predicates(
    where_clause: &str,
) -> Option<Vec<voltnuerongrid_store::columnar::VectorizedFilter>> {
    use voltnuerongrid_store::columnar::{FilterOp, VectorizedFilter};
    let preds: Vec<VectorizedFilter> = where_clause
        .split(" AND ")
        .filter_map(|clause| {
            let clause = clause.trim();
            let ops: &[(&str, FilterOp)] = &[
                (">=", FilterOp::Gte),
                ("<=", FilterOp::Lte),
                ("!=", FilterOp::Ne),
                (">",  FilterOp::Gt),
                ("<",  FilterOp::Lt),
                ("=",  FilterOp::Eq),
            ];
            for (sym, op) in ops {
                if let Some(pos) = clause.find(sym) {
                    let col = clause[..pos].trim().to_string();
                    let val = clause[pos + sym.len()..].trim()
                        .trim_matches('\'').trim_matches('"').to_string();
                    if !col.is_empty() {
                        return Some(VectorizedFilter { column: col, op: op.clone(), value: val });
                    }
                }
            }
            None
        })
        .collect();
    if preds.is_empty() { None } else { Some(preds) }
}

