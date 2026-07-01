//! Full-text search engine — in-memory inverted index.
//!
//! Provides SQL-compatible functions:
//! - [`to_tsvector`] — tokenize text into a sorted token string
//! - [`plainto_tsquery`] — tokenize a plain-text query into a tsquery string
//! - [`fts_match`] — implements the `@@` operator
//! - [`ts_rank`] — BM25-inspired rank (0..1) of a match
//! - [`FtsIndex`] — per-table inverted index for the `/api/v1/search/fulltext` endpoint

#![allow(dead_code)]

use std::collections::HashMap;

// ── Stop words (English) ──────────────────────────────────────────────────────

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "in", "on", "at", "to",
    "of", "and", "or", "but", "not", "with", "for", "from", "by", "as",
    "be", "this", "that", "it", "he", "she", "they", "we", "i", "you",
    "its", "their", "our", "my", "do", "does", "did", "has", "have", "had",
];

// ── Tokenizer ─────────────────────────────────────────────────────────────────

/// Tokenize text: lowercase, split on non-alphanumeric, remove stop words,
/// apply basic stemming, sort, deduplicate.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && t.len() > 1)
        .map(|t| t.to_lowercase())
        .filter(|t| !STOP_WORDS.contains(&t.as_str()))
        .map(|t| stem(&t))
        .collect();
    tokens.sort_unstable();
    tokens.dedup();
    tokens
}

/// Very basic English suffix stemmer (covers common cases without dependencies).
fn stem(word: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "ing", "tion", "sion", "ness", "ment", "able", "ible", "ful",
        "less", "ous", "ive", "ize", "ise", "ate", "ify", "ical",
        "ed", "er", "est", "ly", "es", "s",
    ];
    for suffix in SUFFIXES {
        if word.len() > suffix.len() + 2 && word.ends_with(suffix) {
            return word[..word.len() - suffix.len()].to_string();
        }
    }
    word.to_string()
}

// ── SQL-compatible functions ───────────────────────────────────────────────────

/// Convert text to a tsvector representation: a space-separated sorted token
/// list, analogous to PostgreSQL's `to_tsvector('english', text)`.
pub(crate) fn to_tsvector(text: &str) -> String {
    tokenize(text).join(" ")
}

/// Convert a plain-text query to a tsquery (AND of stemmed tokens).
pub(crate) fn plainto_tsquery(query: &str) -> String {
    tokenize(query).join(" & ")
}

/// Implements the `@@` operator: true if the tsvector satisfies the tsquery.
///
/// Supports `&` (AND) and `|` (OR) operators in the query.
pub(crate) fn fts_match(tsvec: &str, tsquery: &str) -> bool {
    if tsquery.trim().is_empty() {
        return false;
    }
    let vec_tokens: std::collections::HashSet<&str> = tsvec.split_whitespace().collect();
    // Top-level: split on '&' (AND).
    for and_clause in tsquery.split('&') {
        // Each AND clause may contain OR alternatives.
        let or_satisfied = and_clause
            .split('|')
            .map(str::trim)
            .any(|t| vec_tokens.contains(t));
        if !or_satisfied {
            return false;
        }
    }
    true
}

/// Compute ts_rank as the fraction of query tokens present in the tsvector
/// (range 0..1).
pub(crate) fn ts_rank(tsvec: &str, tsquery: &str) -> f32 {
    let vec_tokens: std::collections::HashSet<&str> = tsvec.split_whitespace().collect();
    let query_tokens: Vec<&str> = tsquery
        .split(|c| c == '&' || c == '|')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    if query_tokens.is_empty() {
        return 0.0;
    }
    let hits = query_tokens
        .iter()
        .filter(|&&t| vec_tokens.contains(t))
        .count();
    hits as f32 / query_tokens.len() as f32
}

// ── In-memory inverted index ──────────────────────────────────────────────────

/// Per-table inverted index for full-text search.
#[derive(Default)]
pub(crate) struct FtsIndex {
    /// `table` → `token` → `[row_key, …]`
    inverted: HashMap<String, HashMap<String, Vec<String>>>,
}

impl FtsIndex {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Index a document (row) for full-text search.
    pub(crate) fn index_document(&mut self, table: &str, row_key: &str, text: &str) {
        let table_idx = self.inverted.entry(table.to_string()).or_default();
        for token in tokenize(text) {
            let list = table_idx.entry(token).or_default();
            if !list.contains(&row_key.to_string()) {
                list.push(row_key.to_string());
            }
        }
    }

    /// Remove a document from the index (called on DELETE).
    pub(crate) fn remove_document(&mut self, table: &str, row_key: &str) {
        if let Some(table_idx) = self.inverted.get_mut(table) {
            for posting_list in table_idx.values_mut() {
                posting_list.retain(|k| k != row_key);
            }
        }
    }

    /// Search the index, returning `[(row_key, rank)]` sorted by rank descending.
    pub(crate) fn search(&self, table: &str, query: &str, limit: usize) -> Vec<(String, f32)> {
        let query_tokens = tokenize(query);
        let table_idx = match self.inverted.get(table) {
            Some(i) => i,
            None => return vec![],
        };
        let mut scores: HashMap<String, u32> = HashMap::new();
        for token in &query_tokens {
            if let Some(rows) = table_idx.get(token) {
                for row in rows {
                    *scores.entry(row.clone()).or_insert(0) += 1;
                }
            }
        }
        let total = query_tokens.len().max(1) as f32;
        let mut results: Vec<(String, f32)> = scores
            .into_iter()
            .map(|(key, count)| (key, count as f32 / total))
            .collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    /// Number of indexed documents for a table.
    pub(crate) fn doc_count(&self, table: &str) -> usize {
        self.inverted
            .get(table)
            .and_then(|idx| idx.values().next())
            .map(|list| list.len())
            .unwrap_or(0)
    }
}
