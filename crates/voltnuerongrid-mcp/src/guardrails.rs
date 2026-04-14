//! MCP safety guardrails and validation

use thiserror::Error;
use crate::tools::QueryToolRequest;
use regex::Regex;

#[derive(Debug, Error)]
pub enum GuardrailError {
    #[error("Query exceeds maximum size: {0} > {1} bytes")]
    QueryTooLarge(usize, usize),

    #[error("Query contains prohibited keywords: {0}")]
    ProhibitedKeywords(String),

    #[error("Result size would exceed limit: {0} > {1} bytes")]
    ResultSizeExceeded(usize, usize),

    #[error("Query timeout is too large: {0}ms > {1}ms")]
    TimeoutTooLarge(u64, u64),

    #[error("Suspicious query pattern detected: {0}")]
    SuspiciousPattern(String),
}

/// Query guardrails enforce safety constraints
pub struct QueryGuardrails;

impl QueryGuardrails {
    const MAX_QUERY_SIZE_BYTES: usize = 64 * 1024; // 64 KB
    const MAX_RESULT_SIZE_BYTES: usize = 10 * 1024; // 10 KB
    const MAX_TIMEOUT_MS: u64 = 300_000; // 5 minutes
    const PROHIBITED_KEYWORDS: &'static [&'static str] = &[
        "DROP", "TRUNCATE", "DELETE", "INSERT", "UPDATE", "CREATE", "ALTER",
        "GRANT", "REVOKE", "PRAGMA", "ATTACH", "DETACH", "VACUUM", "OPTIMIZE",
    ];

    /// Validate a query request against safety guardrails
    pub fn validate(req: &QueryToolRequest) -> Result<(), GuardrailError> {
        Self::check_query_size(req)?;
        Self::check_prohibited_keywords(req)?;
        Self::check_timeout(req)?;
        Self::check_suspicious_patterns(req)?;
        Ok(())
    }

    fn check_query_size(req: &QueryToolRequest) -> Result<(), GuardrailError> {
        let size = req.sql_query.len();
        if size > Self::MAX_QUERY_SIZE_BYTES {
            return Err(GuardrailError::QueryTooLarge(
                size,
                Self::MAX_QUERY_SIZE_BYTES,
            ));
        }
        Ok(())
    }

    fn check_prohibited_keywords(req: &QueryToolRequest) -> Result<(), GuardrailError> {
        let upper_query = req.sql_query.to_uppercase();

        for keyword in Self::PROHIBITED_KEYWORDS {
            // Simple check for prohibited keywords (not word-boundary aware to avoid false negatives)
            if upper_query.contains(keyword) {
                // Avoid simple string splitting which can be bypassed
                // Instead use word boundary detection
                if Self::contains_keyword(&upper_query, keyword) {
                    return Err(GuardrailError::ProhibitedKeywords(keyword.to_string()));
                }
            }
        }
        Ok(())
    }

    fn contains_keyword(query: &str, keyword: &str) -> bool {
        // Word boundary check: keyword should be surrounded by whitespace or special chars
        let pattern = format!(r#"(^|[\s\(,])\b{}\b([\s\),;]|$)"#, regex::escape(keyword));
        if let Ok(re) = Regex::new(&pattern) {
            re.is_match(query)
        } else {
            // Fallback to simple contains if regex fails
            query.contains(keyword)
        }
    }

    fn check_timeout(req: &QueryToolRequest) -> Result<(), GuardrailError> {
        if let Some(timeout) = req.timeout_ms {
            if timeout > Self::MAX_TIMEOUT_MS {
                return Err(GuardrailError::TimeoutTooLarge(timeout, Self::MAX_TIMEOUT_MS));
            }
        }
        Ok(())
    }

    fn check_suspicious_patterns(req: &QueryToolRequest) -> Result<(), GuardrailError> {
        let lower_query = req.sql_query.to_lowercase();

        // Check for common SQL injection patterns
        if lower_query.contains("/*") || lower_query.contains("--") {
            // Comments might be OK in some contexts, but flag them for review
            if lower_query.contains("*/") && lower_query.find("/*").unwrap_or(0) < lower_query.rfind("*/").unwrap_or(0) {
                return Err(GuardrailError::SuspiciousPattern(
                    "SQL comments detected".to_string(),
                ));
            }
        }

        // Check for stacked queries
        if lower_query.matches(';').count() > 1 {
            return Err(GuardrailError::SuspiciousPattern(
                "Multiple statements detected".to_string(),
            ));
        }

        Ok(())
    }

    /// Calculate estimated result size
    pub fn estimate_result_size(row_count: usize, avg_column_width: usize, num_columns: usize) -> usize {
        row_count * (num_columns * avg_column_width)
    }

    /// Check if estimated result would exceed limit
    pub fn check_result_size(estimated_bytes: usize) -> Result<(), GuardrailError> {
        if estimated_bytes > Self::MAX_RESULT_SIZE_BYTES {
            Err(GuardrailError::ResultSizeExceeded(
                estimated_bytes,
                Self::MAX_RESULT_SIZE_BYTES,
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_query() {
        let req = QueryToolRequest {
            sql_query: "SELECT id, name FROM users WHERE id > 100".to_string(),
            timeout_ms: Some(5000),
            tenant_id: None,
            max_rows: Some(100),
        };
        assert!(QueryGuardrails::validate(&req).is_ok());
    }

    #[test]
    fn test_prohibited_keywords_drop() {
        let req = QueryToolRequest {
            sql_query: "DROP TABLE users".to_string(),
            timeout_ms: None,
            tenant_id: None,
            max_rows: None,
        };
        let result = QueryGuardrails::validate(&req);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DROP"));
    }

    #[test]
    fn test_prohibited_keywords_delete() {
        let req = QueryToolRequest {
            sql_query: "DELETE FROM users WHERE id = 1".to_string(),
            timeout_ms: None,
            tenant_id: None,
            max_rows: None,
        };
        let result = QueryGuardrails::validate(&req);
        assert!(result.is_err());
    }

    #[test]
    fn test_query_size_limit() {
        let large_query = "SELECT * FROM users WHERE ".to_string() + &"id IN (1,2,3,4,5,)".repeat(10000);
        let req = QueryToolRequest {
            sql_query: large_query,
            timeout_ms: None,
            tenant_id: None,
            max_rows: None,
        };
        let result = QueryGuardrails::validate(&req);
        assert!(result.is_err());
    }

    #[test]
    fn test_timeout_limit() {
        let req = QueryToolRequest {
            sql_query: "SELECT * FROM users".to_string(),
            timeout_ms: Some(500_000), // 500 seconds > 300 second limit
            tenant_id: None,
            max_rows: None,
        };
        let result = QueryGuardrails::validate(&req);
        assert!(result.is_err());
    }

    #[test]
    fn test_stacked_queries() {
        let req = QueryToolRequest {
            sql_query: "SELECT * FROM users; DROP TABLE users;".to_string(),
            timeout_ms: None,
            tenant_id: None,
            max_rows: None,
        };
        let result = QueryGuardrails::validate(&req);
        assert!(result.is_err());
    }

    #[test]
    fn test_estimate_result_size() {
        let size = QueryGuardrails::estimate_result_size(1000, 100, 5);
        assert_eq!(size, 500_000); // 1000 * 100 * 5
    }

    #[test]
    fn test_result_size_check() {
        // Small result should pass
        assert!(QueryGuardrails::check_result_size(1024).is_ok());

        // Large result should fail
        let large_size = 100 * 1024; // 100 KB > 10 KB limit
        let result = QueryGuardrails::check_result_size(large_size);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_allowed() {
        let req = QueryToolRequest {
            sql_query: "SELECT COUNT(*) FROM users".to_string(),
            timeout_ms: None,
            tenant_id: None,
            max_rows: None,
        };
        assert!(QueryGuardrails::validate(&req).is_ok());
    }

    #[test]
    fn test_case_insensitive_keyword_detection() {
        let req = QueryToolRequest {
            sql_query: "select * from users".to_string(), // lowercase select should be fine
            timeout_ms: None,
            tenant_id: None,
            max_rows: None,
        };
        assert!(QueryGuardrails::validate(&req).is_ok());

        let req2 = QueryToolRequest {
            sql_query: "select * from users; delete from users".to_string(), // delete is prohibited
            timeout_ms: None,
            tenant_id: None,
            max_rows: None,
        };
        assert!(QueryGuardrails::validate(&req2).is_err());
    }
}
