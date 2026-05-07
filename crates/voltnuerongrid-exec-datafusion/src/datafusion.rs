//! Phase 3 — real DataFusion executor.
//!
//! Now that the workspace MSRV has bumped for RocksDB, we can adopt
//! DataFusion 38+ for full SQL coverage: JOIN, GROUP BY, HAVING, window
//! functions, subqueries, all with Arrow-native columnar execution.
//!
//! The old hand-rolled evaluator (`crate::execute_select`) remains as a
//! fast path for simple SELECT statements. This executor handles everything
//! else and can eventually replace the hand-rolled path entirely once
//! performance parity is achieved.

use std::sync::Arc;

use datafusion::prelude::*;

use crate::{CheckpointManifest, ExecError, SelectOutput, WalRecord};

/// Full DataFusion executor for complex SQL.
///
/// Accepts any SQL statement that DataFusion's parser understands.
/// Returns the same `SelectOutput` type as the simple executor for
/// API consistency.
pub async fn execute_select(
    sql: &str,
    _store: &crate::voltnuerongrid_store::PagedRowStore,
    _max_rows: usize,
) -> Result<SelectOutput, ExecError> {
    // Phase 3.1 — stub. Will be implemented incrementally:
    // 1. Parse SQL via DataFusion's parser.
    // 2. Hydrate an in-memory DataFrame from the row store (temporary).
    // 3. Execute the plan.
    // 4. Drain results into SelectOutput.
    //
    // For now, return Unsupported so the service falls back to the
    // hand-rolled executor and text-WAL replay (if configured).
    Err(ExecError::Unsupported(
        "DataFusion executor Phase 3: not yet implemented".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_returns_unsupported() {
        let err = execute_select("SELECT * FROM t", &todo!(), 100)
            .await
            .expect_err("stub should return Unsupported");
        assert!(matches!(err, ExecError::Unsupported(_)));
    }
}
