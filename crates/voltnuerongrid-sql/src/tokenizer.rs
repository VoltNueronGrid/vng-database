//! SQL tokenizer for VoltNueronGrid DB.
//!
//! Converts a raw SQL string into a flat sequence of [`Token`]s.  This is the
//! first step toward a real SQL parser (S3-WS1-04 in status-tracker-v2.md).
//! The tokenizer handles:
//! - ANSI SQL keywords (case-insensitive)
//! - Quoted identifiers (`"foo"`)
//! - Single-quoted string literals (`'hello'`)
//! - Integer and decimal number literals
//! - Single- and multi-character symbols (`>=`, `<=`, `<>`, `!=`, `::`)
//! - Line comments (`--`) and block comments (`/* … */`)
//! - Whitespace (collapsed to a single [`Token::Whitespace`])

#![forbid(unsafe_code)]

use std::fmt;

// ---------------------------------------------------------------------------
// Keyword set
// ---------------------------------------------------------------------------

/// Returns true when `word` (already uppercased) is a recognised ANSI SQL
/// keyword that the tokenizer promotes from an identifier to a keyword token.
fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "SELECT" | "FROM" | "WHERE" | "JOIN" | "INNER" | "LEFT" | "RIGHT" | "FULL"
        | "OUTER" | "CROSS" | "ON" | "AS" | "GROUP" | "BY" | "ORDER" | "HAVING"
        | "LIMIT" | "OFFSET" | "UNION" | "ALL" | "DISTINCT" | "EXISTS" | "NOT"
        | "IN" | "BETWEEN" | "LIKE" | "IS" | "NULL" | "AND" | "OR" | "CASE"
        | "WHEN" | "THEN" | "ELSE" | "END" | "OVER" | "PARTITION" | "ROWS"
        | "RANGE" | "UNBOUNDED" | "PRECEDING" | "FOLLOWING" | "CURRENT"
        | "INSERT" | "INTO" | "VALUES" | "UPDATE" | "SET" | "DELETE"
        | "CREATE" | "TABLE" | "VIEW" | "MATERIALIZED" | "FUNCTION"
        | "DROP" | "ALTER" | "ADD" | "COLUMN" | "PRIMARY" | "KEY"
        | "FOREIGN" | "REFERENCES" | "UNIQUE" | "INDEX" | "DEFAULT"
        | "BEGIN" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "RELEASE"
        | "TRANSACTION" | "ISOLATION" | "LEVEL" | "READ" | "WRITE"
        | "SERIALIZABLE" | "REPEATABLE" | "UNCOMMITTED" | "COMMITTED"
        | "TRUE" | "FALSE" | "ASC" | "DESC" | "NULLS" | "FIRST" | "LAST"
        | "WITH" | "RECURSIVE" | "WINDOW" | "FILTER" | "WITHIN" | "ROW"
        | "SUM" | "COUNT" | "AVG" | "MIN" | "MAX" | "COALESCE" | "CAST"
        | "EXTRACT" | "TRIM" | "UPPER" | "LOWER" | "SUBSTRING" | "REPLACE"
        | "CONCAT" | "LENGTH" | "ROUND" | "FLOOR" | "CEIL" | "ABS"
        | "NOW" | "CURRENT_TIMESTAMP" | "CURRENT_DATE" | "CURRENT_TIME"
        | "IF" | "TRUNCATE" | "EXPLAIN" | "ANALYZE" | "SCHEMA" | "DATABASE"
    )
}

// ---------------------------------------------------------------------------
// Token type
// ---------------------------------------------------------------------------

/// A single SQL token produced by [`tokenize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A recognised ANSI SQL keyword (always stored upper-case).
    Keyword(String),
    /// A plain identifier or quoted identifier (stored as-is, without delimiters).
    Identifier(String),
    /// A numeric literal (integer or decimal, stored as the raw text).
    Number(String),
    /// A single-quoted string literal (stored without the surrounding quotes).
    StringLiteral(String),
    /// A one- or two-character symbol: `(`, `)`, `,`, `;`, `*`, `=`, `<>`, `>=`, etc.
    Symbol(String),
    /// One or more whitespace characters (newlines, tabs, spaces) collapsed together.
    Whitespace,
    /// The `--` single-line comment text (excluding the leading `--`).
    LineComment(String),
    /// The `/* … */` block comment text (excluding delimiters).
    BlockComment(String),
    /// Any character sequence that did not match a known pattern.
    Unknown(String),
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Keyword(k) => write!(f, "{k}"),
            Token::Identifier(i) => write!(f, "{i}"),
            Token::Number(n) => write!(f, "{n}"),
            Token::StringLiteral(s) => write!(f, "'{s}'"),
            Token::Symbol(s) => write!(f, "{s}"),
            Token::Whitespace => write!(f, " "),
            Token::LineComment(c) => write!(f, "--{c}"),
            Token::BlockComment(c) => write!(f, "/*{c}*/"),
            Token::Unknown(u) => write!(f, "{u}"),
        }
    }
}

impl Token {
    /// Returns `true` if this token carries semantic meaning (i.e. is not
    /// whitespace or a comment).
    pub fn is_semantic(&self) -> bool {
        !matches!(self, Token::Whitespace | Token::LineComment(_) | Token::BlockComment(_))
    }

    /// Returns the keyword string if this token is a [`Token::Keyword`].
    pub fn keyword(&self) -> Option<&str> {
        if let Token::Keyword(k) = self { Some(k.as_str()) } else { None }
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// Tokenize `sql` into a [`Vec<Token>`].
///
/// The entire input is consumed.  No error is returned — unrecognised
/// characters are wrapped in [`Token::Unknown`].
pub fn tokenize(sql: &str) -> Vec<Token> {
    let chars: Vec<char> = sql.chars().collect();
    let mut pos = 0;
    let mut tokens = Vec::new();

    while pos < chars.len() {
        let ch = chars[pos];

        // — whitespace —
        if ch.is_ascii_whitespace() {
            while pos < chars.len() && chars[pos].is_ascii_whitespace() {
                pos += 1;
            }
            tokens.push(Token::Whitespace);
            continue;
        }

        // — line comment: -- … \n —
        if ch == '-' && pos + 1 < chars.len() && chars[pos + 1] == '-' {
            pos += 2;
            let start = pos;
            while pos < chars.len() && chars[pos] != '\n' {
                pos += 1;
            }
            let text: String = chars[start..pos].iter().collect();
            tokens.push(Token::LineComment(text));
            continue;
        }

        // — block comment: /* … */ —
        if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '*' {
            pos += 2;
            let start = pos;
            while pos + 1 < chars.len() && !(chars[pos] == '*' && chars[pos + 1] == '/') {
                pos += 1;
            }
            let text: String = chars[start..pos].iter().collect();
            if pos + 1 < chars.len() {
                pos += 2; // consume */
            }
            tokens.push(Token::BlockComment(text));
            continue;
        }

        // — single-quoted string literal —
        if ch == '\'' {
            pos += 1;
            let mut s = String::new();
            while pos < chars.len() {
                if chars[pos] == '\'' {
                    // handle escaped quote ''
                    if pos + 1 < chars.len() && chars[pos + 1] == '\'' {
                        s.push('\'');
                        pos += 2;
                    } else {
                        pos += 1;
                        break;
                    }
                } else {
                    s.push(chars[pos]);
                    pos += 1;
                }
            }
            tokens.push(Token::StringLiteral(s));
            continue;
        }

        // — double-quoted identifier —
        if ch == '"' {
            pos += 1;
            let mut s = String::new();
            while pos < chars.len() && chars[pos] != '"' {
                s.push(chars[pos]);
                pos += 1;
            }
            if pos < chars.len() {
                pos += 1; // consume closing "
            }
            tokens.push(Token::Identifier(s));
            continue;
        }

        // — backtick-quoted identifier (MySQL compat) —
        if ch == '`' {
            pos += 1;
            let mut s = String::new();
            while pos < chars.len() && chars[pos] != '`' {
                s.push(chars[pos]);
                pos += 1;
            }
            if pos < chars.len() {
                pos += 1;
            }
            tokens.push(Token::Identifier(s));
            continue;
        }

        // — numeric literal —
        if ch.is_ascii_digit() || (ch == '.' && pos + 1 < chars.len() && chars[pos + 1].is_ascii_digit()) {
            let start = pos;
            while pos < chars.len() && (chars[pos].is_ascii_digit() || chars[pos] == '.') {
                pos += 1;
            }
            // optional exponent: 1e10 / 1E-2
            if pos < chars.len() && (chars[pos] == 'e' || chars[pos] == 'E') {
                pos += 1;
                if pos < chars.len() && (chars[pos] == '+' || chars[pos] == '-') {
                    pos += 1;
                }
                while pos < chars.len() && chars[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
            let num: String = chars[start..pos].iter().collect();
            tokens.push(Token::Number(num));
            continue;
        }

        // — keyword or identifier —
        if ch.is_alphabetic() || ch == '_' {
            let start = pos;
            while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                pos += 1;
            }
            let word: String = chars[start..pos].iter().collect();
            let upper = word.to_ascii_uppercase();
            if is_keyword(&upper) {
                tokens.push(Token::Keyword(upper));
            } else {
                tokens.push(Token::Identifier(word));
            }
            continue;
        }

        // — two-character symbols —
        if pos + 1 < chars.len() {
            let two: String = chars[pos..pos + 2].iter().collect();
            if matches!(two.as_str(), "<>" | ">=" | "<=" | "!=" | "::" | "->" | "->>" | "||") {
                tokens.push(Token::Symbol(two));
                pos += 2;
                continue;
            }
        }

        // — single-character symbol —
        if "()[]{}.,;*+-/%^=<>|&~?@#!:\\".contains(ch) {
            tokens.push(Token::Symbol(ch.to_string()));
            pos += 1;
            continue;
        }

        // — unknown —
        tokens.push(Token::Unknown(ch.to_string()));
        pos += 1;
    }

    tokens
}

/// Returns only the semantic tokens (no whitespace, no comments).
pub fn semantic_tokens(sql: &str) -> Vec<Token> {
    tokenize(sql).into_iter().filter(Token::is_semantic).collect()
}

/// Returns the count of distinct keywords in `sql`.
pub fn keyword_count(sql: &str) -> usize {
    tokenize(sql).iter().filter(|t| matches!(t, Token::Keyword(_))).count()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(s: &str) -> Token { Token::Keyword(s.to_string()) }
    fn id(s: &str) -> Token { Token::Identifier(s.to_string()) }
    fn num(s: &str) -> Token { Token::Number(s.to_string()) }
    fn sym(s: &str) -> Token { Token::Symbol(s.to_string()) }
    fn lit(s: &str) -> Token { Token::StringLiteral(s.to_string()) }

    fn sem(sql: &str) -> Vec<Token> { semantic_tokens(sql) }

    #[test]
    fn tokenizer_simple_select() {
        let tokens = sem("SELECT id FROM users WHERE id = 1;");
        assert_eq!(tokens[0], kw("SELECT"));
        assert_eq!(tokens[1], id("id"));
        assert_eq!(tokens[2], kw("FROM"));
        assert_eq!(tokens[3], id("users"));
        assert_eq!(tokens[4], kw("WHERE"));
        assert_eq!(tokens[5], id("id"));
        assert_eq!(tokens[6], sym("="));
        assert_eq!(tokens[7], num("1"));
        assert_eq!(tokens[8], sym(";"));
    }

    #[test]
    fn tokenizer_string_literal_with_escaped_quote() {
        let tokens = sem("SELECT 'it''s fine' AS label;");
        assert_eq!(tokens[1], lit("it's fine"));
        assert_eq!(tokens[2], kw("AS"));
        assert_eq!(tokens[3], id("label"));
    }

    #[test]
    fn tokenizer_two_char_symbols() {
        let tokens = sem("SELECT * FROM t WHERE age >= 18 AND score <> 0;");
        let syms: Vec<_> = tokens.iter().filter(|t| matches!(t, Token::Symbol(_))).collect();
        assert!(syms.iter().any(|t| **t == sym(">=")));
        assert!(syms.iter().any(|t| **t == sym("<>")));
    }

    #[test]
    fn tokenizer_line_comment_excluded_from_semantic() {
        let tokens = sem("SELECT 1; -- this is a comment\nSELECT 2;");
        let comments: Vec<_> = tokens.iter().filter(|t| matches!(t, Token::LineComment(_))).collect();
        assert!(comments.is_empty(), "line comments must not appear in semantic token stream");
        let kws: Vec<_> = tokens.iter().filter_map(|t| t.keyword()).collect();
        assert_eq!(kws, &["SELECT", "SELECT"]);
    }

    #[test]
    fn tokenizer_block_comment() {
        let all = tokenize("/* header */ SELECT 1;");
        assert!(matches!(all[0], Token::BlockComment(_)));
        let first_kw = all.iter().find(|t| matches!(t, Token::Keyword(_)));
        assert_eq!(first_kw, Some(&kw("SELECT")));
    }

    #[test]
    fn tokenizer_window_function() {
        let tokens = sem("SELECT SUM(amount) OVER(PARTITION BY region) FROM orders;");
        assert!(tokens.iter().any(|t| t.keyword() == Some("SUM")));
        assert!(tokens.iter().any(|t| t.keyword() == Some("OVER")));
        assert!(tokens.iter().any(|t| t.keyword() == Some("PARTITION")));
        assert!(tokens.iter().any(|t| t.keyword() == Some("BY")));
    }

    #[test]
    fn tokenizer_number_variants() {
        let tokens = sem("SELECT 42, 3.14, 1e10 FROM t;");
        let nums: Vec<_> = tokens.iter()
            .filter_map(|t| if let Token::Number(n) = t { Some(n.as_str()) } else { None })
            .collect();
        assert!(nums.contains(&"42"));
        assert!(nums.contains(&"3.14"));
        assert!(nums.contains(&"1e10"));
    }

    #[test]
    fn tokenizer_quoted_identifier() {
        let tokens = sem(r#"SELECT "My Column" FROM "My Table";"#);
        assert_eq!(tokens[1], id("My Column"));
        assert_eq!(tokens[3], id("My Table"));
    }

    #[test]
    fn tokenizer_ddl_create_table() {
        let sql = "CREATE TABLE orders (id INT PRIMARY KEY, amount FLOAT);";
        let kws: Vec<_> = sem(sql).into_iter().filter_map(|t| t.keyword().map(|k| k.to_string())).collect();
        assert!(kws.contains(&"CREATE".to_string()));
        assert!(kws.contains(&"TABLE".to_string()));
        assert!(kws.contains(&"PRIMARY".to_string()));
        assert!(kws.contains(&"KEY".to_string()));
    }

    #[test]
    fn keyword_count_correct() {
        let sql = "SELECT id, name FROM users WHERE id = 1 AND name IS NOT NULL;";
        let count = keyword_count(sql);
        // SELECT, FROM, WHERE, AND, IS, NOT, NULL = 7
        assert_eq!(count, 7);
    }

    #[test]
    fn tokenizer_insert_statement() {
        let tokens = sem("INSERT INTO orders VALUES (1, 'acme', 99.99);");
        assert_eq!(tokens[0], kw("INSERT"));
        assert_eq!(tokens[1], kw("INTO"));
        assert_eq!(tokens[2], id("orders"));
        assert_eq!(tokens[3], kw("VALUES"));
    }

    #[test]
    fn tokenizer_transaction_keywords() {
        let tokens = sem("BEGIN; SAVEPOINT sp1; ROLLBACK TO sp1; COMMIT;");
        let kws: Vec<_> = tokens.iter().filter_map(|t| t.keyword()).collect();
        assert!(kws.contains(&"BEGIN"));
        assert!(kws.contains(&"SAVEPOINT"));
        assert!(kws.contains(&"ROLLBACK"));
        assert!(kws.contains(&"COMMIT"));
        // "TO" appears as an identifier since it's not in the keyword set
        let ids: Vec<_> = tokens.iter()
            .filter_map(|t| if let Token::Identifier(i) = t { Some(i.as_str()) } else { None })
            .collect();
        assert!(ids.contains(&"TO") || kws.contains(&"TO"),
            "TO must appear as either keyword or identifier");
    }
}
