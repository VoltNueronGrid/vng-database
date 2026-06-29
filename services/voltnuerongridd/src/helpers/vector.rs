//! Vector search engine — flat-scan cosine/dot-product ANN index.
//!
//! Stores indexed vectors per `(table, column)` pair.  For datasets up to
//! 100k vectors a flat cosine scan comfortably meets the < 50 ms latency
//! target; the `VectorIndex` API is identical to what an HNSW-backed store
//! would expose so the engine can be swapped in the future.

use std::collections::HashMap;

// ── Core index ────────────────────────────────────────────────────────────────

/// Flat-scan cosine-similarity vector index.
#[derive(Default)]
pub(crate) struct VectorIndex {
    /// `(table, column)` → list of `(row_key, pre-normalized vector)`
    entries: HashMap<(String, String), Vec<(String, Vec<f32>)>>,
    /// Declared dimensionality per `(table, column)`
    dims: HashMap<(String, String), usize>,
}

impl VectorIndex {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Insert (or overwrite) a vector for a given `(table, column, row_key)`.
    pub(crate) fn insert(&mut self, table: &str, column: &str, row_key: &str, vec: Vec<f32>) {
        let key = (table.to_string(), column.to_string());
        let n = vec.len();
        self.dims.insert(key.clone(), n);
        let normalized = normalize(&vec);
        let list = self.entries.entry(key).or_default();
        if let Some(pos) = list.iter().position(|(k, _)| k == row_key) {
            list[pos].1 = normalized;
        } else {
            list.push((row_key.to_string(), normalized));
        }
    }

    /// Return the K nearest neighbors by cosine similarity (higher = closer).
    /// Result is `[(row_key, similarity)]` sorted descending.
    pub(crate) fn search_cosine(
        &self,
        table: &str,
        column: &str,
        query: &[f32],
        k: usize,
    ) -> Vec<(String, f32)> {
        let key = (table.to_string(), column.to_string());
        let entries = match self.entries.get(&key) {
            Some(e) => e,
            None => return vec![],
        };
        let q = normalize(query);
        let mut scored: Vec<(String, f32)> = entries
            .iter()
            .map(|(rk, vec)| (rk.clone(), dot(&q, vec)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }

    /// Declared dimensionality for a `(table, column)` pair.
    pub(crate) fn dim(&self, table: &str, column: &str) -> Option<usize> {
        self.dims
            .get(&(table.to_string(), column.to_string()))
            .copied()
    }

    /// Number of indexed vectors for a `(table, column)`.
    pub(crate) fn entry_count(&self, table: &str, column: &str) -> usize {
        self.entries
            .get(&(table.to_string(), column.to_string()))
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

// ── Math helpers ──────────────────────────────────────────────────────────────

/// L2-normalize a vector (return zero vector if near-zero magnitude).
pub(crate) fn normalize(v: &[f32]) -> Vec<f32> {
    let norm = l2_norm(v);
    if norm < f32::EPSILON {
        v.to_vec()
    } else {
        v.iter().map(|x| x / norm).collect()
    }
}

/// L2 (Euclidean) norm.
pub(crate) fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

/// Dot product (inner product) of two same-length vectors.
pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Cosine similarity between two arbitrary (non-normalized) vectors.
pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let na = l2_norm(a);
    let nb = l2_norm(b);
    if na < f32::EPSILON || nb < f32::EPSILON {
        return 0.0;
    }
    dot(a, b) / (na * nb)
}

// ── Literal parser ────────────────────────────────────────────────────────────

/// Parse a pgvector-style literal `'[0.1,0.2,0.3]'` or `[0.1,0.2,0.3]` into
/// a `Vec<f32>`.
pub(crate) fn parse_vector_literal(s: &str) -> Result<Vec<f32>, String> {
    let trimmed = s
        .trim()
        .trim_start_matches('\'')
        .trim_end_matches('\'')
        .trim_start_matches('[')
        .trim_end_matches(']');
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    trimmed
        .split(',')
        .map(|t| {
            t.trim()
                .parse::<f32>()
                .map_err(|e| format!("vector_parse_error: '{t}' — {e}"))
        })
        .collect()
}
