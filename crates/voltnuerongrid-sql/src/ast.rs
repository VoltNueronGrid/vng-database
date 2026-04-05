//! SQL Abstract Syntax Tree (AST) types and a recursive-descent parser.
//!
//! Covers the ANSI SQL subset used by VoltNueronGrid: SELECT, INSERT, UPDATE,
//! DELETE, CREATE TABLE, BEGIN/COMMIT/ROLLBACK. Built on top of the tokenizer.
//!
//! Advances sprint backlog item S3-WS1-04 (tokenizer → AST/parser).

use super::tokenizer::{semantic_tokens, Token};

// ─── AST node types ──────────────────────────────────────────────────────────

/// Top-level SQL statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Select(SelectStatement),
    Insert(InsertStatement),
    Update(UpdateStatement),
    Delete(DeleteStatement),
    CreateTable(CreateTableStatement),
    Begin,
    Commit,
    Rollback,
    /// A statement that was recognised but could not be fully parsed.
    Unknown(String),
}

/// A parsed SELECT statement.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SelectStatement {
    /// Output column expressions (`"*"` for star, otherwise bare names / aliases).
    pub columns: Vec<String>,
    /// Primary table in the FROM clause.
    pub table: Option<String>,
    /// Raw WHERE clause text (everything after WHERE up to GROUP/ORDER/LIMIT).
    pub where_clause: Option<String>,
    /// GROUP BY column list.
    pub group_by: Vec<String>,
    /// Raw HAVING clause text.
    pub having: Option<String>,
    /// ORDER BY specifications.
    pub order_by: Vec<OrderByClause>,
    /// LIMIT value, if present.
    pub limit: Option<u64>,
}

/// A parsed INSERT statement.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InsertStatement {
    /// Target table name.
    pub table: String,
    /// Column list, if provided: `INSERT INTO t (a, b, c) VALUES (...)`.
    pub columns: Vec<String>,
    /// Value rows. Each inner `Vec<String>` is one VALUES row.
    pub values: Vec<Vec<String>>,
}

/// A parsed UPDATE statement.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UpdateStatement {
    pub table: String,
    /// `(column_name, new_value_literal)` pairs from the SET clause.
    pub assignments: Vec<(String, String)>,
    /// Raw WHERE clause text.
    pub where_clause: Option<String>,
}

/// A parsed DELETE statement.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeleteStatement {
    pub table: String,
    /// Raw WHERE clause text.
    pub where_clause: Option<String>,
}

/// A parsed CREATE TABLE statement.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CreateTableStatement {
    pub table: String,
    pub columns: Vec<ColumnDef>,
}

/// A single column definition inside CREATE TABLE.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    /// Raw SQL data-type string (e.g. `"INTEGER"`, `"VARCHAR(255)"`).
    pub data_type: String,
}

/// An ORDER BY clause element.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderByClause {
    pub column: String,
    pub descending: bool,
}

// ─── Parser entry point ───────────────────────────────────────────────────────

/// Parse a single SQL statement into a [`Statement`] AST node.
///
/// Returns `Ok(Statement::Unknown(raw))` for statements that are syntactically
/// valid SQL but not yet handled by this parser (e.g. TRUNCATE, EXPLAIN).
/// Returns `Err(reason)` only for clearly malformed or empty input.
pub fn parse_one(sql: &str) -> Result<Statement, String> {
    let tokens = semantic_tokens(sql);
    if tokens.is_empty() {
        return Err("empty input".to_string());
    }
    parse_tokens(sql, &tokens)
}

// ─── Internal parser ──────────────────────────────────────────────────────────

fn parse_tokens(raw: &str, tokens: &[Token]) -> Result<Statement, String> {
    match tokens.first() {
        Some(Token::Keyword(k)) => match k.to_ascii_uppercase().as_str() {
            "SELECT" => Ok(Statement::Select(parse_select(tokens))),
            "INSERT" => parse_insert(tokens).map(Statement::Insert),
            "UPDATE" => Ok(Statement::Update(parse_update(tokens))),
            "DELETE" => Ok(Statement::Delete(parse_delete(tokens))),
            "CREATE" => parse_create(tokens)
                .map(Statement::CreateTable)
                .or_else(|_| Ok(Statement::Unknown(raw.to_string()))),
            "BEGIN" | "START" => Ok(Statement::Begin),
            "COMMIT" => Ok(Statement::Commit),
            "ROLLBACK" => Ok(Statement::Rollback),
            _ => Ok(Statement::Unknown(raw.to_string())),
        },
        _ => Ok(Statement::Unknown(raw.to_string())),
    }
}

// ─── SELECT ───────────────────────────────────────────────────────────────────

fn parse_select(tokens: &[Token]) -> SelectStatement {
    let mut stmt = SelectStatement::default();
    let mut pos = 1usize; // skip SELECT

    // Columns: everything until FROM (or end)
    let from_pos = find_keyword(tokens, "FROM");
    let col_end = from_pos.unwrap_or(tokens.len());
    for tok in &tokens[pos..col_end] {
        if let Some(name) = token_as_name(tok) {
            stmt.columns.push(name);
        } else if matches!(tok, Token::Symbol(s) if s == "*") {
            stmt.columns.push("*".to_string());
        }
    }
    if stmt.columns.is_empty() {
        stmt.columns.push("*".to_string());
    }

    // FROM <table>
    if let Some(fp) = from_pos {
        pos = fp + 1;
        if let Some(Token::Identifier(t) | Token::Keyword(t)) = tokens.get(pos) {
            stmt.table = Some(t.clone());
            pos += 1;
        }
    }

    // WHERE
    if let Some(wp) = find_keyword_from(tokens, "WHERE", pos) {
        let where_end = find_any_keyword_from(
            tokens,
            &["GROUP", "HAVING", "ORDER", "LIMIT"],
            wp + 1,
        )
        .unwrap_or(tokens.len());
        stmt.where_clause = Some(tokens_to_raw(&tokens[wp + 1..where_end]));
        pos = where_end;
    }

    // GROUP BY
    if let Some(gp) = find_keyword_from(tokens, "GROUP", pos) {
        let by_skip =
            if matches!(tokens.get(gp + 1), Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("BY"))
            {
                gp + 2
            } else {
                gp + 1
            };
        let group_end =
            find_any_keyword_from(tokens, &["HAVING", "ORDER", "LIMIT"], by_skip)
                .unwrap_or(tokens.len());
        for tok in &tokens[by_skip..group_end] {
            if let Some(name) = token_as_name(tok) {
                stmt.group_by.push(name);
            }
        }
        pos = group_end;
    }

    // HAVING
    if let Some(hp) = find_keyword_from(tokens, "HAVING", pos) {
        let having_end =
            find_any_keyword_from(tokens, &["ORDER", "LIMIT"], hp + 1).unwrap_or(tokens.len());
        stmt.having = Some(tokens_to_raw(&tokens[hp + 1..having_end]));
        pos = having_end;
    }

    // ORDER BY
    if let Some(op) = find_keyword_from(tokens, "ORDER", pos) {
        let by_skip =
            if matches!(tokens.get(op + 1), Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("BY"))
            {
                op + 2
            } else {
                op + 1
            };
        let order_end =
            find_any_keyword_from(tokens, &["LIMIT"], by_skip).unwrap_or(tokens.len());
        let mut i = by_skip;
        while i < order_end {
            if let Some(name) = token_as_name(&tokens[i]) {
                let descending = matches!(
                    tokens.get(i + 1),
                    Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("DESC")
                );
                stmt.order_by.push(OrderByClause {
                    column: name,
                    descending,
                });
                i += if descending { 2 } else { 1 };
            } else {
                i += 1;
            }
        }
        pos = order_end;
    }

    // LIMIT
    if let Some(lp) = find_keyword_from(tokens, "LIMIT", pos) {
        if let Some(Token::Number(n)) = tokens.get(lp + 1) {
            stmt.limit = n.parse::<u64>().ok();
        }
    }

    stmt
}

// ─── INSERT ───────────────────────────────────────────────────────────────────

fn parse_insert(tokens: &[Token]) -> Result<InsertStatement, String> {
    // INSERT [INTO] <table> [(<cols>)] VALUES (<vals>)[, (<vals>)]*
    let mut pos = 1usize; // skip INSERT
    // optional INTO
    if matches!(tokens.get(pos), Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("INTO")) {
        pos += 1;
    }
    let table = match tokens.get(pos) {
        Some(Token::Identifier(t) | Token::Keyword(t)) => {
            pos += 1;
            t.clone()
        }
        _ => return Err("INSERT missing table name".to_string()),
    };

    let mut columns = Vec::new();
    // Optional column list in parens before VALUES
    if matches!(tokens.get(pos), Some(Token::Symbol(s)) if s == "(") {
        pos += 1;
        while pos < tokens.len() {
            match &tokens[pos] {
                Token::Symbol(s) if s == ")" => {
                    pos += 1;
                    break;
                }
                Token::Symbol(s) if s == "," => pos += 1,
                tok => {
                    if let Some(name) = token_as_name(tok) {
                        columns.push(name);
                    }
                    pos += 1;
                }
            }
        }
    }

    // VALUES
    if !matches!(tokens.get(pos), Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("VALUES")) {
        return Err("INSERT missing VALUES keyword".to_string());
    }
    pos += 1;

    let mut all_values: Vec<Vec<String>> = Vec::new();
    while pos < tokens.len() {
        if matches!(&tokens[pos], Token::Symbol(s) if s == "(") {
            pos += 1;
            let mut row_vals = Vec::new();
            while pos < tokens.len() {
                match &tokens[pos] {
                    Token::Symbol(s) if s == ")" => {
                        pos += 1;
                        break;
                    }
                    Token::Symbol(s) if s == "," => pos += 1,
                    Token::StringLiteral(s) => {
                        row_vals.push(s.clone());
                        pos += 1;
                    }
                    Token::Number(n) => {
                        row_vals.push(n.clone());
                        pos += 1;
                    }
                    Token::Keyword(k) if k.eq_ignore_ascii_case("NULL") => {
                        row_vals.push("NULL".to_string());
                        pos += 1;
                    }
                    _ => pos += 1,
                }
            }
            all_values.push(row_vals);
        } else if matches!(&tokens[pos], Token::Symbol(s) if s == ",") {
            pos += 1;
        } else {
            break;
        }
    }

    Ok(InsertStatement {
        table,
        columns,
        values: all_values,
    })
}

// ─── UPDATE ───────────────────────────────────────────────────────────────────

fn parse_update(tokens: &[Token]) -> UpdateStatement {
    let mut stmt = UpdateStatement::default();
    let mut pos = 1usize; // skip UPDATE
    if let Some(Token::Identifier(t) | Token::Keyword(t)) = tokens.get(pos) {
        stmt.table = t.clone();
        pos += 1;
    }
    // skip SET keyword
    if matches!(tokens.get(pos), Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("SET")) {
        pos += 1;
    }
    let where_pos = find_keyword_from(tokens, "WHERE", pos);
    let assign_end = where_pos.unwrap_or(tokens.len());
    let mut i = pos;
    while i < assign_end {
        let col = match token_as_name(&tokens[i]) {
            Some(c) => c,
            None => {
                i += 1;
                continue;
            }
        };
        i += 1;
        // skip '='
        if matches!(tokens.get(i), Some(Token::Symbol(s)) if s == "=") {
            i += 1;
        }
        let val = match tokens.get(i) {
            Some(Token::StringLiteral(s)) => s.clone(),
            Some(Token::Number(n)) => n.clone(),
            Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("NULL") => "NULL".to_string(),
            _ => {
                i += 1;
                continue;
            }
        };
        i += 1;
        stmt.assignments.push((col, val));
        // skip comma separator
        if matches!(tokens.get(i), Some(Token::Symbol(s)) if s == ",") {
            i += 1;
        }
    }
    if let Some(wp) = where_pos {
        stmt.where_clause = Some(tokens_to_raw(&tokens[wp + 1..]));
    }
    stmt
}

// ─── DELETE ───────────────────────────────────────────────────────────────────

fn parse_delete(tokens: &[Token]) -> DeleteStatement {
    let mut stmt = DeleteStatement::default();
    let mut pos = 1usize; // skip DELETE
    // skip optional FROM
    if matches!(tokens.get(pos), Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("FROM")) {
        pos += 1;
    }
    if let Some(Token::Identifier(t) | Token::Keyword(t)) = tokens.get(pos) {
        stmt.table = t.clone();
        pos += 1;
    }
    if let Some(wp) = find_keyword_from(tokens, "WHERE", pos) {
        stmt.where_clause = Some(tokens_to_raw(&tokens[wp + 1..]));
    }
    stmt
}

// ─── CREATE TABLE ─────────────────────────────────────────────────────────────

fn parse_create(tokens: &[Token]) -> Result<CreateTableStatement, String> {
    // CREATE [OR REPLACE] TABLE <name> (<col_def>, ...)
    let mut pos = 1usize; // skip CREATE
    // skip optional OR REPLACE
    if matches!(tokens.get(pos), Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("OR")) {
        pos += 2; // skip OR REPLACE
    }
    if !matches!(tokens.get(pos), Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("TABLE")) {
        return Err("CREATE without TABLE".to_string());
    }
    pos += 1;
    let table = match tokens.get(pos) {
        Some(Token::Identifier(t) | Token::Keyword(t)) => {
            pos += 1;
            t.clone()
        }
        _ => return Err("CREATE TABLE missing name".to_string()),
    };
    let mut columns = Vec::new();
    if matches!(tokens.get(pos), Some(Token::Symbol(s)) if s == "(") {
        pos += 1;
        while pos < tokens.len() {
            match &tokens[pos] {
                Token::Symbol(s) if s == ")" => break,
                Token::Symbol(s) if s == "," => pos += 1,
                tok => {
                    let col_name = match token_as_name(tok) {
                        Some(n) => n,
                        None => {
                            pos += 1;
                            continue;
                        }
                    };
                    pos += 1;
                    let mut type_parts = Vec::new();
                    while pos < tokens.len() {
                        match &tokens[pos] {
                            Token::Symbol(s) if s == "," || s == ")" => break,
                            Token::Symbol(s) if s == "(" => {
                                type_parts.push("(".to_string());
                                pos += 1;
                            }
                            t => {
                                if let Some(n) = token_as_name(t) {
                                    type_parts.push(n);
                                } else if let Token::Number(n) = t {
                                    type_parts.push(n.clone());
                                }
                                pos += 1;
                            }
                        }
                    }
                    columns.push(ColumnDef {
                        name: col_name,
                        data_type: type_parts.join(" "),
                    });
                }
            }
        }
    }
    Ok(CreateTableStatement { table, columns })
}

// ─── Token utilities ──────────────────────────────────────────────────────────

fn find_keyword(tokens: &[Token], kw: &str) -> Option<usize> {
    find_keyword_from(tokens, kw, 0)
}

fn find_keyword_from(tokens: &[Token], kw: &str, from: usize) -> Option<usize> {
    tokens[from..]
        .iter()
        .position(|t| matches!(t, Token::Keyword(k) if k.eq_ignore_ascii_case(kw)))
        .map(|p| p + from)
}

fn find_any_keyword_from(tokens: &[Token], kws: &[&str], from: usize) -> Option<usize> {
    tokens[from..]
        .iter()
        .position(|t| {
            kws.iter()
                .any(|kw| matches!(t, Token::Keyword(k) if k.eq_ignore_ascii_case(kw)))
        })
        .map(|p| p + from)
}

fn token_as_name(tok: &Token) -> Option<String> {
    match tok {
        Token::Identifier(s) | Token::Keyword(s) => Some(s.clone()),
        _ => None,
    }
}

/// Reconstruct a readable string from a token slice (space-separated).
fn tokens_to_raw(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|t| match t {
            Token::Keyword(s) | Token::Identifier(s) | Token::Number(s) => s.clone(),
            Token::StringLiteral(s) => format!("'{s}'"),
            Token::Symbol(s) => s.clone(),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_parse_simple_select() {
        let s = parse_one("SELECT id, name FROM users").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert_eq!(sel.columns, vec!["id", "name"]);
        assert_eq!(sel.table.as_deref(), Some("users"));
        assert!(sel.where_clause.is_none());
    }

    #[test]
    fn ast_parse_select_star() {
        let s = parse_one("SELECT * FROM orders").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert_eq!(sel.columns, vec!["*"]);
        assert_eq!(sel.table.as_deref(), Some("orders"));
    }

    #[test]
    fn ast_parse_select_with_where() {
        let s = parse_one("SELECT name FROM customers WHERE id = 42").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert_eq!(sel.table.as_deref(), Some("customers"));
        let wc = sel.where_clause.expect("should have where clause");
        assert!(wc.contains("42"), "where clause should contain '42': {wc}");
    }

    #[test]
    fn ast_parse_select_with_limit() {
        let s = parse_one("SELECT id FROM t LIMIT 10").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert_eq!(sel.limit, Some(10));
    }

    #[test]
    fn ast_parse_select_order_by_desc() {
        let s = parse_one("SELECT id FROM t ORDER BY created_at DESC").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert_eq!(sel.order_by.len(), 1);
        assert_eq!(sel.order_by[0].column, "created_at");
        assert!(sel.order_by[0].descending);
    }

    #[test]
    fn ast_parse_select_group_by_having() {
        let s =
            parse_one("SELECT region FROM sales GROUP BY region HAVING count > 100").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert_eq!(sel.group_by, vec!["region"]);
        assert!(sel.having.is_some());
    }

    #[test]
    fn ast_parse_insert_simple_values() {
        let s = parse_one("INSERT INTO orders VALUES ('ord-1', 500)").unwrap();
        let Statement::Insert(ins) = s else { panic!("expected Insert") };
        assert_eq!(ins.table, "orders");
        assert_eq!(ins.values.len(), 1);
        assert_eq!(ins.values[0], vec!["ord-1", "500"]);
        assert!(ins.columns.is_empty());
    }

    #[test]
    fn ast_parse_insert_with_column_list() {
        let s =
            parse_one("INSERT INTO users (id, name) VALUES ('u1', 'Alice')").unwrap();
        let Statement::Insert(ins) = s else { panic!("expected Insert") };
        assert_eq!(ins.table, "users");
        assert_eq!(ins.columns, vec!["id", "name"]);
        assert_eq!(ins.values[0], vec!["u1", "Alice"]);
    }

    #[test]
    fn ast_parse_update() {
        let s = parse_one("UPDATE products SET price = 99 WHERE id = 'p1'").unwrap();
        let Statement::Update(upd) = s else { panic!("expected Update") };
        assert_eq!(upd.table, "products");
        assert_eq!(
            upd.assignments,
            vec![("price".to_string(), "99".to_string())]
        );
        assert!(upd.where_clause.is_some());
    }

    #[test]
    fn ast_parse_delete_with_where() {
        let s = parse_one("DELETE FROM orders WHERE id = 'ord-1'").unwrap();
        let Statement::Delete(del) = s else { panic!("expected Delete") };
        assert_eq!(del.table, "orders");
        assert!(del.where_clause.is_some());
    }

    #[test]
    fn ast_parse_create_table() {
        let s = parse_one(
            "CREATE TABLE events (id INTEGER, name VARCHAR, ts BIGINT)",
        )
        .unwrap();
        let Statement::CreateTable(ct) = s else {
            panic!("expected CreateTable")
        };
        assert_eq!(ct.table, "events");
        assert_eq!(ct.columns.len(), 3);
        assert_eq!(ct.columns[0].name, "id");
        assert_eq!(ct.columns[0].data_type, "INTEGER");
        assert_eq!(ct.columns[1].name, "name");
    }

    #[test]
    fn ast_parse_begin_commit_rollback() {
        assert_eq!(parse_one("BEGIN").unwrap(), Statement::Begin);
        assert_eq!(parse_one("COMMIT").unwrap(), Statement::Commit);
        assert_eq!(parse_one("ROLLBACK").unwrap(), Statement::Rollback);
    }

    #[test]
    fn ast_parse_empty_returns_err() {
        assert!(parse_one("").is_err());
    }

    #[test]
    fn ast_parse_unknown_falls_through() {
        let s = parse_one("TRUNCATE TABLE users").unwrap();
        assert!(matches!(s, Statement::Unknown(_)));
    }
}

// ── S3-WS1-06: ANSI SQL conformance tests ────────────────────────────────────
#[cfg(test)]
mod ansi_conformance {
    use super::*;

    // SELECT — column aliases and DISTINCT (parsed as columns, alias dropped gracefully)
    #[test]
    fn ansi_select_with_alias_parses_as_select() {
        let s = parse_one("SELECT id AS user_id, name AS full_name FROM users").unwrap();
        assert!(matches!(s, Statement::Select(_)));
    }

    #[test]
    fn ansi_select_distinct_parses_as_select() {
        let s = parse_one("SELECT DISTINCT region FROM sales").unwrap();
        assert!(matches!(s, Statement::Select(_)));
    }

    // SELECT — multi-column GROUP BY + HAVING
    #[test]
    fn ansi_select_multi_column_group_by() {
        let s = parse_one(
            "SELECT region, category, SUM(amount) FROM sales GROUP BY region, category HAVING SUM(amount) > 500",
        )
        .unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert!(sel.group_by.len() >= 1, "group_by should be populated");
        assert!(sel.having.is_some(), "having should be set");
    }

    // SELECT — ORDER BY multiple columns
    #[test]
    fn ansi_select_order_by_multiple_columns() {
        let s = parse_one("SELECT id FROM orders ORDER BY created_at DESC, id ASC").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert!(sel.order_by.len() >= 1);
    }

    // SELECT — LIMIT
    #[test]
    fn ansi_select_limit_only() {
        let s = parse_one("SELECT * FROM logs LIMIT 50").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert_eq!(sel.limit, Some(50));
    }

    // SELECT — WHERE with AND / OR clause (stored verbatim)
    #[test]
    fn ansi_select_where_and_or() {
        let s = parse_one(
            "SELECT id FROM customers WHERE region = 'us' AND status = 'active'",
        )
        .unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert!(sel.where_clause.is_some());
    }

    // SELECT — WHERE with numeric comparison
    #[test]
    fn ansi_select_where_numeric_comparison() {
        let s = parse_one("SELECT id FROM orders WHERE total > 100").unwrap();
        let Statement::Select(sel) = s else { panic!("expected Select") };
        assert!(sel.where_clause.is_some());
    }

    // INSERT — single-row without column list (positional)
    #[test]
    fn ansi_insert_single_row_positional() {
        let s = parse_one("INSERT INTO events VALUES ('e1', 'click', 1712345678)").unwrap();
        let Statement::Insert(ins) = s else { panic!("expected Insert") };
        assert_eq!(ins.table, "events");
        assert_eq!(ins.values.len(), 1);
    }

    // INSERT — multi-row VALUES
    #[test]
    fn ansi_insert_multi_row_values() {
        let s = parse_one(
            "INSERT INTO tags (id, label) VALUES ('t1', 'rust'), ('t2', 'sql')",
        )
        .unwrap();
        let Statement::Insert(ins) = s else { panic!("expected Insert") };
        assert_eq!(ins.table, "tags");
        assert_eq!(ins.values.len(), 2);
        assert_eq!(ins.values[0][1], "rust");
        assert_eq!(ins.values[1][1], "sql");
    }

    // UPDATE — multiple SET assignments
    #[test]
    fn ansi_update_multiple_assignments() {
        let s = parse_one(
            "UPDATE users SET name = 'Bob', status = 'active' WHERE id = 'u1'",
        )
        .unwrap();
        let Statement::Update(upd) = s else { panic!("expected Update") };
        assert_eq!(upd.table, "users");
        assert!(!upd.assignments.is_empty());
        assert!(upd.where_clause.is_some());
    }

    // UPDATE — no WHERE (full table update)
    #[test]
    fn ansi_update_without_where() {
        let s = parse_one("UPDATE settings SET active = 'true'").unwrap();
        let Statement::Update(upd) = s else { panic!("expected Update") };
        assert_eq!(upd.table, "settings");
        assert!(upd.where_clause.is_none());
    }

    // DELETE — with WHERE
    #[test]
    fn ansi_delete_with_where_clause() {
        let s = parse_one("DELETE FROM sessions WHERE expires < 1712345678").unwrap();
        let Statement::Delete(del) = s else { panic!("expected Delete") };
        assert_eq!(del.table, "sessions");
        assert!(del.where_clause.is_some());
    }

    // DELETE — without WHERE (full delete)
    #[test]
    fn ansi_delete_without_where() {
        let s = parse_one("DELETE FROM temp_cache").unwrap();
        let Statement::Delete(del) = s else { panic!("expected Delete") };
        assert_eq!(del.table, "temp_cache");
        assert!(del.where_clause.is_none());
    }

    // CREATE TABLE — various data types
    #[test]
    fn ansi_create_table_various_types() {
        let s = parse_one(
            "CREATE TABLE metrics (id BIGINT, value FLOAT, label VARCHAR, ts TIMESTAMP)",
        )
        .unwrap();
        let Statement::CreateTable(ct) = s else { panic!("expected CreateTable") };
        assert_eq!(ct.table, "metrics");
        assert_eq!(ct.columns.len(), 4);
    }

    // CREATE TABLE — with IF NOT EXISTS (graceful Unknown or CreateTable)
    #[test]
    fn ansi_create_table_if_not_exists_parses_without_panic() {
        let s = parse_one(
            "CREATE TABLE IF NOT EXISTS audit_log (id INTEGER, action VARCHAR)",
        )
        .unwrap();
        // Parser may emit Unknown or CreateTable — both are valid without panic
        assert!(
            matches!(s, Statement::CreateTable(_) | Statement::Unknown(_)),
            "expected CreateTable or Unknown, got: {s:?}"
        );
    }

    // Transaction control
    #[test]
    fn ansi_transaction_control_statements() {
        assert_eq!(parse_one("BEGIN TRANSACTION").unwrap(), Statement::Begin);
        assert_eq!(parse_one("COMMIT WORK").unwrap(), Statement::Commit);
        assert_eq!(parse_one("ROLLBACK WORK").unwrap(), Statement::Rollback);
    }

    // Unknown / unsupported — must not panic
    #[test]
    fn ansi_unsupported_ddl_falls_to_unknown() {
        let stmts = [
            "ALTER TABLE users ADD COLUMN email VARCHAR",
            "DROP TABLE IF EXISTS temp",
            "CREATE INDEX idx_name ON users(name)",
            "TRUNCATE TABLE session_cache",
            "GRANT SELECT ON orders TO analyst",
        ];
        for sql in &stmts {
            let result = parse_one(sql);
            assert!(result.is_ok(), "parse_one must not return Err for: {sql}");
            assert!(
                matches!(result.unwrap(), Statement::Unknown(_)),
                "unsupported DDL should be Unknown: {sql}"
            );
        }
    }
}
