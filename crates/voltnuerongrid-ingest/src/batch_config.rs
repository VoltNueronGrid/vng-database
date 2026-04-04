#![forbid(unsafe_code)]

//! Tunables for future parallel / chunked ingest loaders (REQ-07).
//!
//! Runtime ingest paths still load connectors sequentially; this module only
//! holds validated configuration defaults for upcoming throughput work.

/// Upper bounds for splitting large ingest payloads across worker tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestParallelConfig {
    /// Maximum concurrent ingest tasks (parse + stage) per connector load.
    pub max_in_flight_tasks: usize,
    /// Target number of logical rows per chunk when a source is splittable.
    pub chunk_target_rows: usize,
}

impl Default for IngestParallelConfig {
    fn default() -> Self {
        Self {
            max_in_flight_tasks: 4,
            chunk_target_rows: 10_000,
        }
    }
}

impl IngestParallelConfig {
    pub fn validated(self) -> Result<Self, String> {
        if self.max_in_flight_tasks == 0 {
            return Err("max_in_flight_tasks must be >= 1".to_string());
        }
        if self.chunk_target_rows == 0 {
            return Err("chunk_target_rows must be >= 1".to_string());
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_validates() {
        assert!(IngestParallelConfig::default().validated().is_ok());
    }

    #[test]
    fn rejects_zero_task_cap() {
        let cfg = IngestParallelConfig {
            max_in_flight_tasks: 0,
            chunk_target_rows: 100,
        };
        assert!(cfg.validated().is_err());
    }

    #[test]
    fn rejects_zero_chunk_rows() {
        let cfg = IngestParallelConfig {
            max_in_flight_tasks: 2,
            chunk_target_rows: 0,
        };
        assert!(cfg.validated().is_err());
    }
}
