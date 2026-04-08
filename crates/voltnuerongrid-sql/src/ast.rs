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
    /// Optional JOIN clause (S3-WS1-04).
    pub join: Option<JoinClause>,
    /// True when the raw SQL contains a UNION / UNION ALL set operation (S3-WS1-04).
    pub has_union: bool,
    /// True when the raw SQL contains a correlated or scalar subquery `(SELECT …)` (S3-WS1-04).
    pub has_subquery: bool,
    /// True when the raw SQL contains a window function call `OVER (` (S3-WS1-04).
    pub has_window_fn: bool,
    /// True when the raw SQL contains an aggregate function call (COUNT, SUM, AVG, MIN, MAX) (S3-WS1-04).
    pub has_agg_fn: bool,
    /// True when `SELECT DISTINCT` is used (S3-WS1-04).
    pub is_distinct: bool,
    /// LIMIT value, if present.
    pub limit: Option<u64>,
    /// OFFSET value for pagination (S3-WS1-04).
    pub offset: Option<u64>,
    /// True when the WHERE clause contains IS NULL or IS NOT NULL (S3-WS1-06).
    pub has_null_literal: bool,
    /// True when the query contains a GROUP BY clause (S3-WS1-06).
    pub has_group_by: bool,
    /// True when the query contains an ORDER BY clause (S3-WS1-06).
    pub has_order_by: bool,
    /// True when the query contains a HAVING clause (S3-WS1-06).
    pub has_having: bool,
    /// True when the WHERE clause contains an IN (list) predicate (S3-WS1-07).
    pub has_in_list: bool,
    /// True when the WHERE clause contains a BETWEEN ... AND predicate (S3-WS1-08).
    pub has_between: bool,
    /// True when the WHERE clause contains a LIKE or ILIKE predicate (S3-WS1-09).
    pub has_like: bool,
    /// True when the WHERE clause contains a NOT keyword predicate (S3-WS1-10).
    pub has_not: bool,
    /// True when the query contains a CASE WHEN expression (S3-WS1-11).
    pub has_case: bool,
    /// True when the query contains a COALESCE() expression (S3-WS1-12).
    pub has_coalesce: bool,
    /// True when the query contains a CAST() or :: type-cast expression (S3-WS1-13).
    pub has_cast: bool,
    /// True when the query contains a NULLIF() expression (S3-WS1-14).
    pub has_nullif: bool,
    /// True when the query contains a string function (LENGTH, UPPER, LOWER, SUBSTR) (S3-WS1-15).
    pub has_string_fn: bool,
    /// True when the query contains a date/time function (NOW, DATE_TRUNC, EXTRACT) (S3-WS1-16).
    pub has_date_fn: bool,
    /// True when the query contains a string concatenation (CONCAT() or || operator) (S3-WS1-17).
    pub has_concat: bool,
    /// True when the query contains a math function (ABS, ROUND, CEIL, FLOOR) (S3-WS1-18).
    pub has_math_fn: bool,
    /// True when the query contains an EXISTS subquery predicate (S3-WS1-19).
    pub has_exists: bool,
    /// True when the query uses ANY or ALL quantifiers with a subquery/list (S3-WS1-20).
    pub has_any_all: bool,
    /// True when the query contains a NOT IN (...) predicate (S3-WS1-21).
    pub has_not_in: bool,
    /// True when the query contains a TRIM / LTRIM / RTRIM function call (S3-WS1-22).
    pub has_trim: bool,
    /// True when the query uses an INTERVAL expression (date arithmetic) (S3-WS1-23).
    pub has_interval: bool,
    /// True when the query uses an IN (SELECT ...) subquery predicate (S3-WS1-24).
    pub has_in_subquery: bool,
    /// True when the query tests a column for NULL (`IS NULL` / `IS NOT NULL`) (S3-WS1-25).
    pub has_is_null: bool,
    /// True when the query uses a REGEXP / RLIKE / SIMILAR TO pattern match (S3-WS1-26).
    pub has_regexp: bool,
    /// True when the query uses a JSON operator (`->`, `->>`, `JSON_EXTRACT`, `JSON_VALUE`) (S3-WS1-27).
    pub has_json_op: bool,
    /// True when the query uses a window aggregate function (COUNT/SUM/AVG/ROW_NUMBER OVER ...) (S3-WS1-28).
    pub has_window_agg: bool,
    /// True when the query uses a LATERAL join or LATERAL subquery (S3-WS1-29).
    pub has_lateral: bool,
    /// True when the query uses a PIVOT or UNPIVOT clause (S3-WS1-30).
    pub has_pivot: bool,
    /// True when the query uses a FETCH NEXT/FIRST pagination clause (S3-WS1-31).
    pub has_fetch: bool,
    /// True when the query uses a VALUES clause as a row source / VALUES CTE (S3-WS1-32).
    pub has_values: bool,
    /// True when the query uses a CROSS JOIN expression (S3-WS1-33).
    pub has_cross_join: bool,
    /// True when the query uses a full-text search predicate (MATCH/AGAINST or @@) (S3-WS1-34).
    pub has_full_text_search: bool,
    /// True when the query uses GROUPING SETS in GROUP BY (S3-WS1-35).
    pub has_grouping_sets: bool,
    /// True when the query uses a NATURAL JOIN clause (S3-WS1-36).
    pub has_natural_join: bool,
    /// True when the query uses a JOIN ... USING (...) clause (S3-WS1-37).
    pub has_using_join: bool,
    /// True when the query uses an EXCEPT set operation (S3-WS1-38).
    pub has_except: bool,
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

/// A JOIN clause: `[INNER] JOIN <table> ON <condition>` (S3-WS1-04).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JoinClause {
    /// The name of the joined table.
    pub join_table: String,
    /// Raw ON condition text (everything after ON up to WHERE/GROUP/HAVING/ORDER/LIMIT).
    pub on_condition: Option<String>,
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
            "SELECT" => {
                let mut stmt = parse_select(tokens);
                // Detect UNION / UNION ALL set operations (S3-WS1-04).
                let up = raw.to_ascii_uppercase();
                if up.contains(" UNION ") {
                    stmt.has_union = true;
                }
                // Detect correlated / scalar subqueries: any `(SELECT …)` (S3-WS1-04).
                if up.contains("(SELECT") || up.contains("( SELECT") {
                    stmt.has_subquery = true;
                }
                // Detect window function calls: any `OVER (` or `OVER(` (S3-WS1-04).
                if up.contains("OVER (") || up.contains("OVER(") {
                    stmt.has_window_fn = true;
                }
                // Detect aggregate function calls: COUNT(, SUM(, AVG(, MIN(, MAX( (S3-WS1-04).
                let up_trim = up.replace(' ', "");
                if up_trim.contains("COUNT(") || up_trim.contains("SUM(")
                    || up_trim.contains("AVG(") || up_trim.contains("MIN(")
                    || up_trim.contains("MAX(")
                {
                    stmt.has_agg_fn = true;
                }
                // Detect SELECT DISTINCT keyword (S3-WS1-04).
                if up_trim.starts_with("SELECTDISTINCT") {
                    stmt.is_distinct = true;
                }
                // Detect IS NULL / IS NOT NULL predicates (S3-WS1-06).
                if up.contains("IS NULL") || up.contains("IS NOT NULL") {
                    stmt.has_null_literal = true;
                }
                // Detect GROUP BY clause (S3-WS1-06).
                if up.contains("GROUP BY") {
                    stmt.has_group_by = true;
                }
                // Detect ORDER BY clause (S3-WS1-06).
                if up.contains("ORDER BY") {
                    stmt.has_order_by = true;
                }
                // Detect HAVING clause (S3-WS1-06).
                if up.contains("HAVING") {
                    stmt.has_having = true;
                }
                // Detect IN list predicate in WHERE (S3-WS1-07).
                // Exclude subquery form "IN (SELECT ..." so has_subquery stays exclusive.
                if up.contains(" IN (") && !up.contains("(SELECT") {
                    stmt.has_in_list = true;
                }
                // Detect BETWEEN ... AND predicate in WHERE (S3-WS1-08).
                if up.contains(" BETWEEN ") && up.contains(" AND ") {
                    stmt.has_between = true;
                }
                // Detect LIKE / ILIKE predicate in WHERE (S3-WS1-09).
                if up.contains(" LIKE ") || up.contains(" ILIKE ") {
                    stmt.has_like = true;
                }
                // Detect NOT keyword predicate in WHERE (S3-WS1-10); exclude IS NOT NULL and NOT IN patterns.
                if up.contains(" NOT ") && !up.contains("IS NOT") && !up.contains("NOT IN") {
                    stmt.has_not = true;
                }
                // Detect CASE WHEN expression anywhere in the query (S3-WS1-11).
                if up.contains("CASE WHEN") {
                    stmt.has_case = true;
                }
                // Detect COALESCE() expression anywhere in the query (S3-WS1-12).
                if up_trim.contains("COALESCE(") {
                    stmt.has_coalesce = true;
                }
                // Detect CAST() or :: type-cast expression anywhere in the query (S3-WS1-13).
                if up_trim.contains("CAST(") || up.contains("::") {
                    stmt.has_cast = true;
                }
                // Detect NULLIF() expression anywhere in the query (S3-WS1-14).
                if up_trim.contains("NULLIF(") {
                    stmt.has_nullif = true;
                }
                // Detect string functions anywhere in the query (S3-WS1-15).
                if up_trim.contains("LENGTH(") || up_trim.contains("UPPER(")
                    || up_trim.contains("LOWER(") || up_trim.contains("SUBSTR(") {
                    stmt.has_string_fn = true;
                }
                // Detect date/time functions anywhere in the query (S3-WS1-16).
                if up_trim.contains("NOW(") || up_trim.contains("DATE_TRUNC(")
                    || up_trim.contains("EXTRACT(") {
                    stmt.has_date_fn = true;
                }
                // Detect string concatenation anywhere in the query (S3-WS1-17).
                if up_trim.contains("CONCAT(") || up.contains(" || ") {
                    stmt.has_concat = true;
                }
                // Detect math functions anywhere in the query (S3-WS1-18).
                if up_trim.contains("ABS(") || up_trim.contains("ROUND(")
                    || up_trim.contains("CEIL(") || up_trim.contains("FLOOR(") {
                    stmt.has_math_fn = true;
                }
                // Detect EXISTS subquery predicate (S3-WS1-19).
                if up_trim.contains("EXISTS(") || up.contains("EXISTS (") {
                    stmt.has_exists = true;
                }
                // Detect ANY / ALL quantifiers (S3-WS1-20).
                if up_trim.contains("ANY(") || up_trim.contains("ALL(") {
                    stmt.has_any_all = true;
                }
                // Detect NOT IN predicate (S3-WS1-21).
                if up.contains("NOT IN (") || up.contains("NOT IN(") {
                    stmt.has_not_in = true;
                }
                // Detect TRIM / LTRIM / RTRIM function calls (S3-WS1-22).
                if up_trim.contains("TRIM(") || up_trim.contains("LTRIM(") || up_trim.contains("RTRIM(") {
                    stmt.has_trim = true;
                }
                // Detect INTERVAL date arithmetic expressions (S3-WS1-23).
                if up.contains("INTERVAL") {
                    stmt.has_interval = true;
                }
                // Detect IN (SELECT ...) subquery predicate (S3-WS1-24).
                if up.contains("IN (SELECT") || up.contains("IN(SELECT") {
                    stmt.has_in_subquery = true;
                }
                // Detect IS NULL / IS NOT NULL predicate (S3-WS1-25).
                if up.contains("IS NULL") || up.contains("IS NOT NULL") {
                    stmt.has_is_null = true;
                }
                // Detect REGEXP / RLIKE / SIMILAR TO pattern match (S3-WS1-26).
                if up.contains("REGEXP") || up.contains("RLIKE") || up.contains("SIMILAR TO") {
                    stmt.has_regexp = true;
                }
                // Detect JSON operator access -> / ->> / JSON_EXTRACT / JSON_VALUE (S3-WS1-27).
                if up.contains("->") || up.contains("JSON_EXTRACT") || up.contains("JSON_VALUE") {
                    stmt.has_json_op = true;
                }
                // Detect window aggregate function COUNT/SUM/AVG/ROW_NUMBER OVER (...) (S3-WS1-28).
                if (up.contains("COUNT(") || up.contains("SUM(") || up.contains("AVG(") || up.contains("ROW_NUMBER")) && up.contains("OVER") {
                    stmt.has_window_agg = true;
                }
                // Detect LATERAL join or LATERAL subquery (S3-WS1-29).
                if up.contains("LATERAL") {
                    stmt.has_lateral = true;
                }
                // Detect PIVOT / UNPIVOT clause (S3-WS1-30).
                if up.contains("PIVOT") {
                    stmt.has_pivot = true;
                }
                // Detect FETCH NEXT / FETCH FIRST pagination clause (S3-WS1-31).
                if up.contains("FETCH NEXT") || up.contains("FETCH FIRST") {
                    stmt.has_fetch = true;
                }
                // Detect VALUES clause used as a row source / VALUES CTE (S3-WS1-32).
                if up.contains("VALUES (") {
                    stmt.has_values = true;
                }
                // Detect CROSS JOIN expression (S3-WS1-33).
                if up.contains("CROSS JOIN") {
                    stmt.has_cross_join = true;
                }
                // Detect full-text search predicate MATCH/AGAINST or @@ (S3-WS1-34).
                if up.contains("MATCH (") || up.contains(" @@ ") || up.contains("MATCH(") {
                    stmt.has_full_text_search = true;
                }
                // Detect GROUPING SETS construct in GROUP BY (S3-WS1-35).
                if up.contains("GROUPING SETS") {
                    stmt.has_grouping_sets = true;
                }
                // Detect NATURAL JOIN clause (S3-WS1-36).
                if up.contains("NATURAL JOIN") {
                    stmt.has_natural_join = true;
                }
                // Detect JOIN ... USING (...) clause (S3-WS1-37).
                if up.contains(" USING (") || up.contains("USING(") {
                    stmt.has_using_join = true;
                }
                // Detect EXCEPT set operation (S3-WS1-38).
                if up.contains(" EXCEPT ") {
                    stmt.has_except = true;
                }
                Ok(Statement::Select(stmt))
            }
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

    // [INNER] JOIN <table> [ON <condition>]  (S3-WS1-04)
    if let Some(jp) = find_keyword_from(tokens, "JOIN", pos) {
        pos = jp + 1;
        if let Some(Token::Identifier(t) | Token::Keyword(t)) = tokens.get(pos) {
            let join_table = t.clone();
            pos += 1;
            let on_condition = if matches!(
                tokens.get(pos),
                Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("ON")
            ) {
                pos += 1;
                let on_end = find_any_keyword_from(
                    tokens,
                    &["WHERE", "GROUP", "HAVING", "ORDER", "LIMIT"],
                    pos,
                )
                .unwrap_or(tokens.len());
                let cond = tokens_to_raw(&tokens[pos..on_end]);
                pos = on_end;
                if cond.is_empty() { None } else { Some(cond) }
            } else {
                None
            };
            stmt.join = Some(JoinClause { join_table, on_condition });
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
        // OFFSET immediately after LIMIT value (S3-WS1-04).
        if let Some(Token::Keyword(k)) = tokens.get(lp + 2) {
            if k.eq_ignore_ascii_case("OFFSET") {
                if let Some(Token::Number(n)) = tokens.get(lp + 3) {
                    stmt.offset = n.parse::<u64>().ok();
                }
            }
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

    // HAVING clause on a GROUP BY aggregate query
    #[test]
    fn ansi_select_with_having_clause_parses() {
        let sql = "SELECT dept, COUNT(*) FROM employees GROUP BY dept HAVING COUNT(*) > 5";
        let result = parse_one(sql);
        assert!(result.is_ok(), "parse_one must not error on HAVING query");
        if let Ok(Statement::Select(s)) = result {
            assert!(s.group_by.contains(&"dept".to_string()),
                "GROUP BY dept must be parsed; got: {:?}", s.group_by);
        } else {
            panic!("expected Select statement for HAVING query");
        }
    }

    // LIMIT + OFFSET on ordered SELECT
    #[test]
    fn ansi_select_with_limit_and_offset_parses() {
        let sql = "SELECT id, name FROM users ORDER BY created_at DESC LIMIT 25 OFFSET 100";
        let result = parse_one(sql);
        assert!(result.is_ok(), "parse_one must not error on LIMIT+OFFSET");
        if let Ok(Statement::Select(s)) = result {
            assert_eq!(s.limit, Some(25), "LIMIT 25 must be parsed");
            assert!(!s.order_by.is_empty(), "ORDER BY must be captured");
        } else {
            panic!("expected Select statement for LIMIT+OFFSET query");
        }
    }

    // Subquery detection via has_subquery flag
    #[test]
    fn ansi_select_with_subquery_sets_has_subquery_flag() {
        let sql = "SELECT * FROM (SELECT id FROM users WHERE active = 1) AS active_users";
        let result = parse_one(sql);
        assert!(result.is_ok(), "parse_one must not error on subquery-style SELECT");
        if let Ok(Statement::Select(s)) = result {
            assert!(s.has_subquery,
                "SELECT containing (SELECT … must set has_subquery=true");
        } else {
            panic!("expected Select statement for subquery SELECT");
        }
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

// ─── S3-WS1-04: JOIN clause parsing tests ────────────────────────────────────

#[cfg(test)]
mod join_tests {
    use super::*;

    #[test]
    fn select_with_inner_join_and_on_condition() {
        let sql = "SELECT id, name FROM orders JOIN customers ON orders.customer_id = customers.id";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert_eq!(s.table.as_deref(), Some("orders"));
            let j = s.join.expect("join clause must be present");
            assert_eq!(j.join_table, "customers");
            assert!(j.on_condition.is_some(), "ON condition must be parsed");
            let cond = j.on_condition.unwrap();
            assert!(cond.contains("customer_id"), "ON condition must contain 'customer_id'");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn select_with_join_no_on_parses_table() {
        let sql = "SELECT * FROM orders JOIN shipments";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            let j = s.join.expect("join clause must be present");
            assert_eq!(j.join_table, "shipments");
            assert!(j.on_condition.is_none());
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn select_without_join_leaves_join_none() {
        let sql = "SELECT * FROM users WHERE id = 1";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.join.is_none(), "no JOIN in query — field must be None");
        } else {
            panic!("expected Select statement");
        }
    }
}
/// S3-WS1-04: Subquery / EXISTS detection tests.
#[cfg(test)]
mod subquery_detection_tests {
    use super::*;

    #[test]
    fn select_with_scalar_subquery_sets_has_subquery_true() {
        let sql = "SELECT id, (SELECT MAX(price) FROM products) AS max_price FROM orders";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.has_subquery, "scalar (SELECT …) must set has_subquery = true");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn select_with_in_subquery_sets_has_subquery_true() {
        let sql = "SELECT * FROM users WHERE id IN (SELECT user_id FROM admins)";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.has_subquery, "IN (SELECT …) must set has_subquery = true");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn plain_select_has_subquery_is_false() {
        let sql = "SELECT * FROM users WHERE active = 1";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(!s.has_subquery, "plain SELECT must have has_subquery = false");
        } else {
            panic!("expected Select statement");
        }
    }
}

/// S3-WS1-04: UNION / set-operation detection tests.
#[cfg(test)]
mod subquery_tests {
    use super::*;

    #[test]
    fn select_union_sets_has_union_true() {
        let sql = "SELECT id FROM users UNION SELECT id FROM admins";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.has_union, "UNION keyword must set has_union = true");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn select_union_all_sets_has_union_true() {
        let sql = "SELECT name FROM products UNION ALL SELECT name FROM archived_products";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.has_union, "UNION ALL must set has_union = true");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn plain_select_has_union_is_false() {
        let sql = "SELECT * FROM users WHERE id = 1";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(!s.has_union, "plain SELECT must have has_union = false");
        } else {
            panic!("expected Select statement");
        }
    }
}

/// S3-WS1-04: Window function detection tests (S3-WS1-06 conformance).
#[cfg(test)]
mod window_fn_tests {
    use super::*;

    #[test]
    fn select_rank_over_sets_has_window_fn_true() {
        let sql = "SELECT id, RANK() OVER (PARTITION BY dept ORDER BY salary DESC) AS rnk FROM employees";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.has_window_fn, "RANK() OVER (...) must set has_window_fn = true");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn select_sum_over_partition_by_sets_has_window_fn_true() {
        let sql = "SELECT region, SUM(revenue) OVER(PARTITION BY region) AS region_total FROM sales";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.has_window_fn, "SUM() OVER(...) must set has_window_fn = true");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn plain_aggregate_has_window_fn_false() {
        let sql = "SELECT region, SUM(revenue) FROM sales GROUP BY region";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(!s.has_window_fn, "plain GROUP BY aggregate must NOT set has_window_fn");
        } else {
            panic!("expected Select statement");
        }
    }
}

/// S3-WS1-04: Aggregate function detection tests (S3-WS1-06 conformance).
#[cfg(test)]
mod agg_fn_tests {
    use super::*;

    #[test]
    fn select_count_sets_has_agg_fn_true() {
        let sql = "SELECT COUNT(id) FROM orders WHERE status = 'open'";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.has_agg_fn, "COUNT( must set has_agg_fn = true");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn select_sum_and_avg_set_has_agg_fn_true() {
        let sql = "SELECT SUM(amount), AVG(quantity) FROM line_items";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(s.has_agg_fn, "SUM( and AVG( must set has_agg_fn = true");
        } else {
            panic!("expected Select statement");
        }
    }

    #[test]
    fn plain_select_has_agg_fn_false() {
        let sql = "SELECT id, name FROM customers WHERE id = 1";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(!s.has_agg_fn, "plain SELECT without aggregates must NOT set has_agg_fn");
        } else {
            panic!("expected Select statement");
        }
    }
}

/// S3-WS1-06: Extended ANSI conformance tests — ORDER BY + LIMIT combos and
/// multi-column table operations.
#[cfg(test)]
mod extended_conformance_tests {
    use super::*;

    #[test]
    fn select_order_by_with_limit_parses_both() {
        let sql = "SELECT id, name FROM products ORDER BY name ASC LIMIT 50";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Select(s) = stmt {
            assert!(!s.order_by.is_empty(), "ORDER BY must be captured");
            assert_eq!(s.limit, Some(50), "LIMIT 50 must be captured");
        } else {
            panic!("expected Select");
        }
    }

    #[test]
    fn insert_multi_row_parses_correct_column_count() {
        let sql = "INSERT INTO log (ts, msg, level) VALUES (1, 'boot', 'info'), (2, 'ready', 'info')";
        let stmt = parse_one(sql).unwrap();
        if let Statement::Insert(ins) = stmt {
            assert_eq!(ins.table, "log");
            assert_eq!(ins.columns, vec!["ts", "msg", "LEVEL"]);
            assert_eq!(ins.values.len(), 2, "two rows must be parsed");
        } else {
            panic!("expected Insert");
        }
    }

    #[test]
    fn create_table_with_five_columns_parses_all_columns() {
        let sql = "CREATE TABLE metrics (id BIGINT, name TEXT, value FLOAT, recorded_at TIMESTAMP, source VARCHAR(64))";
        let stmt = parse_one(sql).unwrap();
        if let Statement::CreateTable(ct) = stmt {
            assert_eq!(ct.table, "metrics");
            assert_eq!(ct.columns.len(), 5, "all 5 columns must be parsed");
        } else {
            panic!("expected CreateTable");
        }
    }
}

#[cfg(test)]
mod select_distinct_tests {
    use super::*;

    #[test]
    fn select_distinct_sets_is_distinct_true() {
        let stmt = parse_one("SELECT DISTINCT name FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.is_distinct, "SELECT DISTINCT must set is_distinct = true");
    }

    #[test]
    fn select_without_distinct_has_is_distinct_false() {
        let stmt = parse_one("SELECT name FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.is_distinct, "plain SELECT must have is_distinct = false");
    }

    #[test]
    fn select_distinct_with_where_sets_flag_and_table() {
        let stmt = parse_one("SELECT DISTINCT status FROM orders WHERE active = 'true'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.is_distinct, "SELECT DISTINCT with WHERE must set is_distinct = true");
        assert_eq!(s.table.as_deref(), Some("orders"));
    }
}

#[cfg(test)]
mod offset_tests {
    use super::*;

    #[test]
    fn select_with_limit_and_offset_parses_both() {
        let stmt = parse_one("SELECT * FROM t LIMIT 10 OFFSET 5").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert_eq!(s.limit, Some(10), "LIMIT 10 must be captured");
        assert_eq!(s.offset, Some(5), "OFFSET 5 must be captured");
    }

    #[test]
    fn select_with_offset_zero_is_stored() {
        let stmt = parse_one("SELECT id FROM users LIMIT 100 OFFSET 0").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert_eq!(s.offset, Some(0), "OFFSET 0 must be stored");
    }

    #[test]
    fn select_without_offset_is_none() {
        let stmt = parse_one("SELECT name FROM employees").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert_eq!(s.offset, None, "plain SELECT must have offset = None");
    }
}

#[cfg(test)]
mod null_literal_tests {
    use super::*;

    #[test]
    fn where_is_null_sets_has_null_literal_true() {
        let stmt = parse_one("SELECT * FROM t WHERE col IS NULL").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_null_literal, "IS NULL must set has_null_literal");
    }

    #[test]
    fn where_is_not_null_sets_has_null_literal_true() {
        let stmt = parse_one("SELECT id FROM orders WHERE deleted_at IS NOT NULL").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_null_literal, "IS NOT NULL must set has_null_literal");
    }

    #[test]
    fn plain_select_has_null_literal_false() {
        let stmt = parse_one("SELECT name FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_null_literal, "plain SELECT must have has_null_literal = false");
    }
}

#[cfg(test)]
mod group_by_detection_tests {
    use super::*;

    #[test]
    fn select_with_group_by_sets_has_group_by_true() {
        let stmt = parse_one("SELECT dept, COUNT(*) FROM employees GROUP BY dept").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_group_by, "GROUP BY must set has_group_by = true");
    }

    #[test]
    fn select_without_group_by_has_group_by_false() {
        let stmt = parse_one("SELECT name FROM users WHERE active = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_group_by, "query without GROUP BY must have has_group_by = false");
    }

    #[test]
    fn select_group_by_with_having_sets_has_group_by_true() {
        let stmt = parse_one("SELECT region, SUM(sales) FROM orders GROUP BY region HAVING SUM(sales) > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_group_by, "GROUP BY ... HAVING must set has_group_by = true");
    }
}

#[cfg(test)]
mod order_by_detection_tests {
    use super::*;

    #[test]
    fn select_with_order_by_sets_has_order_by_true() {
        let stmt = parse_one("SELECT id, name FROM users ORDER BY name").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_order_by, "ORDER BY must set has_order_by = true");
    }

    #[test]
    fn select_without_order_by_has_order_by_false() {
        let stmt = parse_one("SELECT name FROM users WHERE active = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_order_by, "query without ORDER BY must have has_order_by = false");
    }

    #[test]
    fn select_order_by_with_limit_sets_has_order_by_true() {
        let stmt = parse_one("SELECT * FROM orders ORDER BY created_at DESC LIMIT 10").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_order_by, "ORDER BY ... LIMIT must set has_order_by = true");
        assert_eq!(s.limit, Some(10), "LIMIT 10 must be parsed");
    }
}

#[cfg(test)]
mod having_flag_tests {
    use super::*;

    #[test]
    fn select_with_having_sets_has_having_true() {
        let stmt = parse_one("SELECT dept, COUNT(*) FROM employees GROUP BY dept HAVING COUNT(*) > 5").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_having, "HAVING clause must set has_having = true");
    }

    #[test]
    fn select_without_having_has_having_false() {
        let stmt = parse_one("SELECT name FROM users WHERE active = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_having, "query without HAVING must have has_having = false");
    }

    #[test]
    fn select_having_also_sets_has_group_by_true() {
        let stmt = parse_one("SELECT region, SUM(sales) FROM orders GROUP BY region HAVING SUM(sales) > 1000").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_having, "HAVING must set has_having = true");
        assert!(s.has_group_by, "GROUP BY ... HAVING must also set has_group_by = true");
    }
}

#[cfg(test)]
mod in_list_tests {
    use super::*;

    #[test]
    fn select_with_in_predicate_sets_has_in_list_true() {
        let stmt = parse_one("SELECT id FROM users WHERE id IN (1, 2, 3)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_in_list, "IN (list) predicate must set has_in_list = true");
    }

    #[test]
    fn select_with_in_list_string_values() {
        let stmt = parse_one("SELECT name FROM products WHERE category IN ('A', 'B', 'C')").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_in_list, "IN with string literals must set has_in_list = true");
    }

    #[test]
    fn plain_select_has_in_list_is_false() {
        let stmt = parse_one("SELECT * FROM orders WHERE total > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_in_list, "plain SELECT without IN must have has_in_list = false");
    }
}

#[cfg(test)]
mod between_tests {
    use super::*;

    #[test]
    fn select_with_between_sets_has_between_true() {
        let stmt = parse_one("SELECT id FROM users WHERE age BETWEEN 18 AND 65").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_between, "BETWEEN ... AND predicate must set has_between = true");
    }

    #[test]
    fn select_with_between_string_range() {
        let stmt = parse_one("SELECT id FROM orders WHERE order_date BETWEEN '2024-01-01' AND '2024-12-31'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_between, "BETWEEN with date strings must set has_between = true");
    }

    #[test]
    fn plain_select_has_between_is_false() {
        let stmt = parse_one("SELECT * FROM transactions WHERE amount > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_between, "plain SELECT without BETWEEN must have has_between = false");
    }
}

#[cfg(test)]
mod like_tests {
    use super::*;

    #[test]
    fn select_with_like_predicate_sets_has_like_true() {
        let stmt = parse_one("SELECT name FROM users WHERE name LIKE '%Alice%'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_like, "LIKE predicate must set has_like = true");
    }

    #[test]
    fn select_with_ilike_predicate_sets_has_like_true() {
        let stmt = parse_one("SELECT email FROM users WHERE email ILIKE '%@example.com'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_like, "ILIKE predicate must set has_like = true");
    }

    #[test]
    fn plain_select_has_like_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_like, "plain SELECT without LIKE must have has_like = false");
    }
}

#[cfg(test)]
mod not_tests {
    use super::*;

    #[test]
    fn select_with_not_exists_sets_has_not_true() {
        let stmt = parse_one("SELECT id FROM users WHERE NOT (id = 0)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_not, "NOT (...) predicate must set has_not = true");
    }

    #[test]
    fn select_with_not_like_sets_has_not_true() {
        let stmt = parse_one("SELECT name FROM users WHERE name NOT LIKE '%admin%'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_not, "NOT LIKE predicate must set has_not = true");
    }

    #[test]
    fn plain_select_has_not_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_not, "plain SELECT without NOT must have has_not = false");
    }
}

#[cfg(test)]
mod case_tests {
    use super::*;

    #[test]
    fn select_with_case_when_sets_has_case_true() {
        let stmt = parse_one("SELECT id, CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END AS category FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_case, "CASE WHEN expression must set has_case = true");
    }

    #[test]
    fn select_with_case_when_in_where_sets_has_case_true() {
        let stmt = parse_one("SELECT id FROM orders WHERE CASE WHEN status = 'active' THEN 1 ELSE 0 END = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_case, "CASE WHEN in WHERE clause must set has_case = true");
    }

    #[test]
    fn plain_select_has_case_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_case, "plain SELECT without CASE WHEN must have has_case = false");
    }
}

#[cfg(test)]
mod coalesce_tests {
    use super::*;

    #[test]
    fn select_with_coalesce_sets_has_coalesce_true() {
        let stmt = parse_one("SELECT COALESCE(name, 'unknown') FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_coalesce, "COALESCE() expression must set has_coalesce = true");
    }

    #[test]
    fn select_with_coalesce_in_where_sets_has_coalesce_true() {
        let stmt = parse_one("SELECT id FROM orders WHERE COALESCE(status, 'pending') = 'active'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_coalesce, "COALESCE() in WHERE clause must set has_coalesce = true");
    }

    #[test]
    fn plain_select_has_coalesce_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_coalesce, "plain SELECT without COALESCE must have has_coalesce = false");
    }
}

#[cfg(test)]
mod cast_tests {
    use super::*;

    #[test]
    fn select_with_cast_sets_has_cast_true() {
        let stmt = parse_one("SELECT CAST(amount AS TEXT) FROM orders").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_cast, "CAST() expression must set has_cast = true");
    }

    #[test]
    fn select_with_pg_cast_operator_sets_has_cast_true() {
        let stmt = parse_one("SELECT amount::TEXT FROM orders").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_cast, ":: cast operator must set has_cast = true");
    }

    #[test]
    fn plain_select_has_cast_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_cast, "plain SELECT without CAST must have has_cast = false");
    }
}

// ─── S3-WS1-14: has_nullif tests ────────────────────────────────────────────

#[cfg(test)]
mod nullif_tests {
    use super::*;

    #[test]
    fn select_with_nullif_sets_has_nullif() {
        let stmt = parse_one("SELECT NULLIF(score, 0) FROM results").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_nullif, "NULLIF() expression must set has_nullif = true");
    }

    #[test]
    fn nullif_detection_is_case_insensitive() {
        let stmt = parse_one("SELECT nullif(a, b) FROM t").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_nullif, "lowercase nullif() must set has_nullif = true");
    }

    #[test]
    fn plain_select_has_nullif_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_nullif, "plain SELECT without NULLIF must have has_nullif = false");
    }
}

// ─── S3-WS1-15: has_string_fn tests ─────────────────────────────────────────

#[cfg(test)]
mod string_fn_tests {
    use super::*;

    #[test]
    fn select_with_upper_sets_has_string_fn() {
        let stmt = parse_one("SELECT UPPER(name) FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_string_fn, "UPPER() expression must set has_string_fn = true");
    }

    #[test]
    fn string_fn_detection_lower_case_insensitive() {
        let stmt = parse_one("SELECT lower(email) FROM accounts").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_string_fn, "lowercase lower() must set has_string_fn = true");
    }

    #[test]
    fn plain_select_has_string_fn_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_string_fn, "plain SELECT without string functions must have has_string_fn = false");
    }
}

// ─── S3-WS1-16: has_date_fn tests ──────────────────────────────────────────

#[cfg(test)]
mod date_fn_tests {
    use super::*;

    #[test]
    fn select_with_now_sets_has_date_fn() {
        let stmt = parse_one("SELECT NOW() FROM dual").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_date_fn, "NOW() expression must set has_date_fn = true");
    }

    #[test]
    fn date_fn_detection_date_trunc_case_insensitive() {
        let stmt = parse_one("SELECT date_trunc('day', created_at) FROM events").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_date_fn, "lowercase date_trunc() must set has_date_fn = true");
    }

    #[test]
    fn plain_select_has_date_fn_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_date_fn, "plain SELECT without date functions must have has_date_fn = false");
    }
}

// ─── S3-WS1-17: has_concat tests ────────────────────────────────────────────

#[cfg(test)]
mod concat_tests {
    use super::*;

    #[test]
    fn select_with_concat_fn_sets_has_concat() {
        let stmt = parse_one("SELECT CONCAT(first_name, ' ', last_name) FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_concat, "CONCAT() expression must set has_concat = true");
    }

    #[test]
    fn concat_detection_pipe_operator() {
        let stmt = parse_one("SELECT first_name || ' ' || last_name FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_concat, "|| concat operator must set has_concat = true");
    }

    #[test]
    fn plain_select_has_concat_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_concat, "plain SELECT without CONCAT must have has_concat = false");
    }
}

// ─── S3-WS1-18: has_math_fn tests ────────────────────────────────────────────

#[cfg(test)]
mod math_fn_tests {
    use super::*;

    #[test]
    fn select_with_abs_sets_has_math_fn() {
        let stmt = parse_one("SELECT ABS(balance) FROM accounts").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_math_fn, "ABS() expression must set has_math_fn = true");
    }

    #[test]
    fn math_fn_detection_round_case_insensitive() {
        let stmt = parse_one("SELECT round(price, 2) FROM products").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_math_fn, "lowercase round() must set has_math_fn = true");
    }

    #[test]
    fn plain_select_has_math_fn_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_math_fn, "plain SELECT without math functions must have has_math_fn = false");
    }
}

// ─── S3-WS1-19: has_exists tests ─────────────────────────────────────────────

#[cfg(test)]
mod exists_tests {
    use super::*;

    #[test]
    fn select_with_exists_sets_has_exists() {
        let stmt = parse_one("SELECT id FROM orders WHERE EXISTS (SELECT 1 FROM items WHERE items.order_id = orders.id)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_exists, "EXISTS subquery must set has_exists = true");
    }

    #[test]
    fn exists_detection_case_insensitive() {
        let stmt = parse_one("SELECT name FROM customers WHERE exists (SELECT 1 FROM orders WHERE orders.cid = customers.id)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_exists, "lowercase exists () must set has_exists = true");
    }

    #[test]
    fn plain_select_has_exists_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_exists, "plain SELECT without EXISTS must have has_exists = false");
    }
}

// ─── S3-WS1-20: has_any_all tests ─────────────────────────────────────────────

#[cfg(test)]
mod any_all_tests {
    use super::*;

    #[test]
    fn select_with_any_sets_has_any_all() {
        let stmt = parse_one("SELECT id FROM products WHERE price > ANY(SELECT price FROM discounts)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_any_all, "ANY quantifier must set has_any_all = true");
    }

    #[test]
    fn select_with_all_sets_has_any_all() {
        let stmt = parse_one("SELECT name FROM employees WHERE salary >= ALL(SELECT salary FROM managers)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_any_all, "ALL quantifier must set has_any_all = true");
    }

    #[test]
    fn plain_select_has_any_all_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_any_all, "plain SELECT without ANY/ALL must have has_any_all = false");
    }
}

// ─── S3-WS1-21: has_not_in tests ─────────────────────────────────────────────

#[cfg(test)]
mod not_in_tests {
    use super::*;

    #[test]
    fn select_with_not_in_sets_has_not_in() {
        let stmt = parse_one("SELECT id FROM orders WHERE status NOT IN ('cancelled', 'failed')").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_not_in, "NOT IN predicate must set has_not_in = true");
    }

    #[test]
    fn not_in_detection_subquery_form() {
        let stmt = parse_one("SELECT name FROM users WHERE id NOT IN (SELECT user_id FROM bans)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_not_in, "NOT IN (subquery) must set has_not_in = true");
    }

    #[test]
    fn plain_select_has_not_in_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_not_in, "plain SELECT without NOT IN must have has_not_in = false");
    }
}

// ─── S3-WS1-22: has_trim tests ─────────────────────────────────────────────

#[cfg(test)]
mod trim_tests {
    use super::*;

    #[test]
    fn select_with_trim_sets_has_trim() {
        let stmt = parse_one("SELECT TRIM(name) FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_trim, "TRIM() call must set has_trim = true");
    }

    #[test]
    fn trim_detection_ltrim_and_rtrim() {
        let stmt = parse_one("SELECT LTRIM(RTRIM(email)) FROM contacts").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_trim, "LTRIM/RTRIM calls must set has_trim = true");
    }

    #[test]
    fn plain_select_has_trim_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_trim, "plain SELECT without TRIM must have has_trim = false");
    }
}
// ─── S3-WS1-23: has_interval tests ───────────────────────────────────────────

#[cfg(test)]
mod interval_tests {
    use super::*;

    #[test]
    fn select_with_interval_sets_has_interval() {
        let stmt = parse_one("SELECT created_at + INTERVAL '7 days' FROM events").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_interval, "INTERVAL expression must set has_interval = true");
    }

    #[test]
    fn interval_detection_alternate_form() {
        let stmt = parse_one("SELECT * FROM logs WHERE ts > NOW() - INTERVAL '1 hour'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_interval, "INTERVAL in WHERE clause must set has_interval = true");
    }

    #[test]
    fn plain_select_has_interval_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_interval, "plain SELECT without INTERVAL must have has_interval = false");
    }

// ─── S3-WS1-24: has_in_subquery tests ────────────────────────────────────────

#[cfg(test)]
mod in_subquery_tests {
    use super::*;

    #[test]
    fn select_with_in_subquery_sets_has_in_subquery() {
        let stmt = parse_one("SELECT id FROM orders WHERE user_id IN (SELECT id FROM users)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_in_subquery, "IN (SELECT ...) must set has_in_subquery = true");
    }

    #[test]
    fn in_subquery_detection_compact_form() {
        let stmt = parse_one("SELECT name FROM products WHERE cat_id IN(SELECT id FROM cats)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_in_subquery, "IN(SELECT...) compact form must set has_in_subquery = true");
    }

    #[test]
    fn plain_select_has_in_subquery_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_in_subquery, "plain SELECT without IN subquery must have has_in_subquery = false");
    }
}

#[cfg(test)]
mod is_null_tests {
    use super::*;

    #[test]
    fn select_with_is_null_sets_has_is_null() {
        let stmt = parse_one("SELECT id FROM users WHERE email IS NULL").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_is_null, "IS NULL predicate must set has_is_null = true");
    }

    #[test]
    fn is_null_detection_is_not_null_form() {
        let stmt = parse_one("SELECT name FROM customers WHERE deleted_at IS NOT NULL").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_is_null, "IS NOT NULL predicate must set has_is_null = true");
    }

    #[test]
    fn plain_select_has_is_null_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_is_null, "plain SELECT without IS NULL must have has_is_null = false");
    }
}
}

#[cfg(test)]
mod regexp_tests {
    use super::*;

    #[test]
    fn select_with_regexp_sets_has_regexp() {
        let stmt = parse_one("SELECT id FROM users WHERE email REGEXP '^[a-z]+'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_regexp, "REGEXP predicate must set has_regexp = true");
    }

    #[test]
    fn regexp_detection_rlike_form() {
        let stmt = parse_one("SELECT name FROM logs WHERE message RLIKE 'error'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_regexp, "RLIKE form must set has_regexp = true");
    }

    #[test]
    fn plain_select_has_regexp_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_regexp, "plain SELECT without pattern match must have has_regexp = false");
    }
}

#[cfg(test)]
mod json_op_tests {
    use super::*;

    #[test]
    fn select_with_arrow_sets_has_json_op() {
        let stmt = parse_one("SELECT data -> '$.name' FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_json_op, "JSON -> operator must set has_json_op = true");
    }

    #[test]
    fn json_op_detection_extract_form() {
        let stmt = parse_one("SELECT JSON_EXTRACT(data, '$.age') FROM profiles").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_json_op, "JSON_EXTRACT must set has_json_op = true");
    }

    #[test]
    fn plain_select_has_json_op_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_json_op, "plain SELECT without JSON ops must have has_json_op = false");
    }
}

#[cfg(test)]
mod window_agg_tests {
    use super::*;

    #[test]
    fn select_with_count_over_sets_has_window_agg() {
        let stmt = parse_one("SELECT COUNT(id) OVER (PARTITION BY dept) FROM employees").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_window_agg, "COUNT() OVER must set has_window_agg = true");
    }

    #[test]
    fn window_agg_detection_row_number_form() {
        let stmt = parse_one("SELECT ROW_NUMBER() OVER (ORDER BY salary DESC) FROM staff").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_window_agg, "ROW_NUMBER OVER must set has_window_agg = true");
    }

    #[test]
    fn plain_select_has_window_agg_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_window_agg, "plain SELECT without window agg must have has_window_agg = false");
    }
}

// ─── S3-WS1-29: has_lateral tests ────────────────────────────────────────────

#[cfg(test)]
mod lateral_tests {
    use super::*;

    #[test]
    fn select_with_lateral_join_sets_has_lateral() {
        let stmt = parse_one("SELECT u.name, o.total FROM users u JOIN LATERAL (SELECT SUM(amount) AS total FROM orders WHERE orders.user_id = u.id) o ON true").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_lateral, "LATERAL JOIN must set has_lateral = true");
    }

    #[test]
    fn select_with_lateral_subquery_sets_has_lateral() {
        let stmt = parse_one("SELECT a.id, b.val FROM accounts a, LATERAL (SELECT val FROM history WHERE history.acct = a.id LIMIT 1) b").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_lateral, "LATERAL subquery must set has_lateral = true");
    }

    #[test]
    fn plain_select_has_lateral_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_lateral, "plain SELECT without LATERAL must have has_lateral = false");
    }
}

// ─── S3-WS1-30: has_pivot tests ────────────────────────────────────────────

#[cfg(test)]
mod pivot_tests {
    use super::*;

    #[test]
    fn select_with_pivot_sets_has_pivot() {
        let stmt = parse_one("SELECT * FROM sales PIVOT (SUM(amount) FOR region IN ('East', 'West'))").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_pivot, "PIVOT clause must set has_pivot = true");
    }

    #[test]
    fn select_with_unpivot_sets_has_pivot() {
        let stmt = parse_one("SELECT product, region, sales FROM quarterly_sales UNPIVOT (sales FOR region IN (q1, q2, q3, q4))").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_pivot, "UNPIVOT clause must set has_pivot = true");
    }

    #[test]
    fn plain_select_has_pivot_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_pivot, "plain SELECT without PIVOT must have has_pivot = false");
    }
}

// ─── S3-WS1-31: has_fetch tests ────────────────────────────────────────────

#[cfg(test)]
mod fetch_tests {
    use super::*;

    #[test]
    fn select_with_fetch_next_sets_has_fetch() {
        let stmt = parse_one("SELECT id FROM orders ORDER BY id OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_fetch, "FETCH NEXT must set has_fetch = true");
    }

    #[test]
    fn select_with_fetch_first_sets_has_fetch() {
        let stmt = parse_one("SELECT name FROM employees ORDER BY salary DESC FETCH FIRST 10 ROWS ONLY").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_fetch, "FETCH FIRST must set has_fetch = true");
    }

    #[test]
    fn plain_select_has_fetch_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_fetch, "plain SELECT without FETCH must have has_fetch = false");
    }
}

// ─── S3-WS1-32: has_values tests ───────────────────────────────────────────

#[cfg(test)]
mod values_tests {
    use super::*;

    #[test]
    fn select_with_values_cte_sets_has_values() {
        let stmt = parse_one("SELECT a, b FROM (VALUES (1,2),(3,4)) AS v(a,b)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_values, "VALUES row source must set has_values = true");
    }

    #[test]
    fn select_with_inline_values_sets_has_values() {
        let stmt = parse_one("SELECT col FROM (VALUES (10),(20),(30)) AS t(col)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_values, "Inline VALUES must set has_values = true");
    }

    #[test]
    fn plain_select_has_values_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_values, "plain SELECT without VALUES must have has_values = false");
    }
}

// ─── S3-WS1-33: has_cross_join tests ─────────────────────────────────────────

#[cfg(test)]
mod cross_join_tests {
    use super::*;

    #[test]
    fn select_with_cross_join_sets_has_cross_join() {
        let stmt = parse_one("SELECT a.id, b.name FROM products a CROSS JOIN categories b").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_cross_join, "CROSS JOIN must set has_cross_join = true");
    }

    #[test]
    fn select_with_cross_join_and_filter_sets_has_cross_join() {
        let stmt = parse_one("SELECT x, y FROM t1 CROSS JOIN t2 WHERE t1.id < 10").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_cross_join, "CROSS JOIN with WHERE must set has_cross_join = true");
    }

    #[test]
    fn plain_select_has_cross_join_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_cross_join, "plain SELECT without CROSS JOIN must have has_cross_join = false");
    }
}

// ─── S3-WS1-34: has_full_text_search tests ─────────────────────────────────────

#[cfg(test)]
mod full_text_search_tests {
    use super::*;

    #[test]
    fn select_with_match_against_sets_has_full_text_search() {
        let stmt = parse_one("SELECT id, title FROM articles WHERE MATCH (title, body) AGAINST ('database engine')").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_full_text_search, "MATCH ... AGAINST must set has_full_text_search = true");
    }

    #[test]
    fn select_with_tsvector_sets_has_full_text_search() {
        let stmt = parse_one("SELECT id FROM docs WHERE to_tsvector(content) @@ plainto_tsquery('search')").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_full_text_search, "@@ full-text operator must set has_full_text_search = true");
    }

    #[test]
    fn plain_select_has_full_text_search_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_full_text_search, "plain SELECT without full-text search must have has_full_text_search = false");
    }
}

// ─── S3-WS1-35: has_grouping_sets tests ─────────────────────────────────────

#[cfg(test)]
mod grouping_sets_tests {
    use super::*;

    #[test]
    fn select_with_grouping_sets_sets_has_grouping_sets() {
        let stmt = parse_one("SELECT region, product, SUM(amount) FROM sales GROUP BY GROUPING SETS ((region), (product))").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_grouping_sets, "GROUPING SETS must set has_grouping_sets = true");
    }

    #[test]
    fn select_with_grouping_sets_and_where_sets_has_grouping_sets() {
        let stmt = parse_one("SELECT dept, role, COUNT(*) FROM staff WHERE active = 1 GROUP BY GROUPING SETS ((dept), (role))").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_grouping_sets, "GROUPING SETS with WHERE must set has_grouping_sets = true");
    }

    #[test]
    fn plain_select_has_grouping_sets_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_grouping_sets, "plain SELECT without GROUPING SETS must have has_grouping_sets = false");
    }
}

// ─── S3-WS1-36: has_natural_join tests ──────────────────────────────────────

#[cfg(test)]
mod natural_join_tests {
    use super::*;

    #[test]
    fn select_with_natural_join_sets_has_natural_join() {
        let stmt = parse_one("SELECT c.id, o.total FROM customers c NATURAL JOIN orders o").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_natural_join, "NATURAL JOIN must set has_natural_join = true");
    }

    #[test]
    fn select_with_natural_join_and_filter_sets_has_natural_join() {
        let stmt = parse_one("SELECT p.id FROM products p NATURAL JOIN inventory i WHERE p.active = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_natural_join, "NATURAL JOIN with WHERE must set has_natural_join = true");
    }

    #[test]
    fn plain_select_has_natural_join_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_natural_join, "plain SELECT without NATURAL JOIN must have has_natural_join = false");
    }
}

// ─── S3-WS1-37: has_using_join tests ────────────────────────────────────────

#[cfg(test)]
mod using_join_tests {
    use super::*;

    #[test]
    fn select_with_join_using_sets_has_using_join() {
        let stmt = parse_one("SELECT c.id, o.total FROM customers c JOIN orders o USING (customer_id)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_using_join, "JOIN ... USING must set has_using_join = true");
    }

    #[test]
    fn select_with_left_join_using_sets_has_using_join() {
        let stmt = parse_one("SELECT u.id, p.title FROM users u LEFT JOIN posts p USING (user_id)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_using_join, "LEFT JOIN ... USING must set has_using_join = true");
    }

    #[test]
    fn plain_select_has_using_join_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_using_join, "plain SELECT without USING join must have has_using_join = false");
    }
}

// ─── S3-WS1-38: has_except tests ────────────────────────────────────────────

#[cfg(test)]
mod except_tests {
    use super::*;

    #[test]
    fn select_with_except_sets_has_except() {
        let stmt = parse_one("SELECT id FROM active_users EXCEPT SELECT id FROM banned_users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_except, "EXCEPT set operation must set has_except = true");
    }

    #[test]
    fn select_with_except_all_sets_has_except() {
        let stmt = parse_one("SELECT id FROM s1 EXCEPT ALL SELECT id FROM s2").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_except, "EXCEPT ALL must set has_except = true");
    }

    #[test]
    fn plain_select_has_except_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_except, "plain SELECT without EXCEPT must have has_except = false");
    }
}