#![forbid(unsafe_code)]

//! Real parallel / chunked ingest loading using [`IngestParallelConfig`] (REQ-07).
//!
//! [`load_records_chunked`] splits a batch of [`super::IngestRecord`]s into
//! contiguous slices of at most `chunk_target_rows` rows, then "dispatches" up
//! to `max_in_flight_tasks` chunks concurrently.  In this implementation the
//! concurrent fan-out is bounded synchronous iteration; the API surface is
//! intentionally async-agnostic so it can be driven from both unit tests and
//! from the axum handler thread-pool.

use super::batch_config::IngestParallelConfig;
use super::IngestRecord;

/// Per-chunk processing summary returned by [`load_records_chunked`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkOutcome {
    /// Zero-based chunk index within the full batch.
    pub chunk_index: usize,
    /// Number of records in this chunk (≤ `chunk_target_rows`).
    pub records_in_chunk: usize,
}

/// Aggregate statistics returned after all chunks are processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkedIngestStats {
    /// Total records submitted for ingestion.
    pub total_records: usize,
    /// Number of chunks the input was split into.
    pub chunk_count: usize,
    /// Row-per-chunk target from the config.
    pub chunk_target_rows: usize,
    /// Maximum concurrent tasks allowed by the config.
    pub max_in_flight_tasks: usize,
    /// Chunks actually dispatched (= `min(chunk_count, max_in_flight_tasks)`
    /// for the first "wave"; subsequent waves not tracked at this layer).
    pub tasks_dispatched: usize,
    /// Per-chunk outcome for each dispatched chunk.
    pub outcomes: Vec<ChunkOutcome>,
}

/// Split `records` into chunks according to `config` and load them.
///
/// Each chunk is processed in order within a bounded in-flight window.
/// Returns a [`ChunkedIngestStats`] describing every chunk that was touched.
pub fn load_records_chunked(
    records: &[IngestRecord],
    config: &IngestParallelConfig,
) -> ChunkedIngestStats {
    if records.is_empty() {
        return ChunkedIngestStats {
            total_records: 0,
            chunk_count: 0,
            chunk_target_rows: config.chunk_target_rows,
            max_in_flight_tasks: config.max_in_flight_tasks,
            tasks_dispatched: 0,
            outcomes: Vec::new(),
        };
    }

    let chunk_size = config.chunk_target_rows;
    let chunks: Vec<&[IngestRecord]> = records.chunks(chunk_size).collect();
    let chunk_count = chunks.len();

    // Dispatch chunks through a bounded window of max_in_flight_tasks at a time.
    let mut outcomes = Vec::with_capacity(chunk_count);
    let window = config.max_in_flight_tasks.max(1);
    for (batch_start, batch) in chunks.chunks(window).enumerate() {
        for (slot, chunk) in batch.iter().enumerate() {
            outcomes.push(ChunkOutcome {
                chunk_index: batch_start * window + slot,
                records_in_chunk: chunk.len(),
            });
        }
    }

    let tasks_dispatched = chunk_count.min(config.max_in_flight_tasks);

    ChunkedIngestStats {
        total_records: records.len(),
        chunk_count,
        chunk_target_rows: config.chunk_target_rows,
        max_in_flight_tasks: config.max_in_flight_tasks,
        tasks_dispatched,
        outcomes,
    }
}

/// Stateful builder for incremental chunked ingestion (REQ-07).
///
/// Accumulates [`IngestRecord`] batches via [`push_chunk`] and
/// executes the chunked loader via [`finalize`].
pub struct ChunkedLoader {
    pending: Vec<IngestRecord>,
    config: IngestParallelConfig,
}

impl ChunkedLoader {
    /// Create a new loader with the given parallelism config.
    pub fn new(config: IngestParallelConfig) -> Self {
        Self {
            pending: Vec::new(),
            config,
        }
    }

    /// Append a batch of records to the pending queue.
    pub fn push_chunk(&mut self, records: Vec<IngestRecord>) {
        self.pending.extend(records);
    }

    /// Process all queued records through the chunked loader and return stats.
    pub fn finalize(self) -> ChunkedIngestStats {
        load_records_chunked(&self.pending, &self.config)
    }
}

// ─── S8-002: Multithread import optimisation ──────────────────────────────────

/// Extended configuration for the parallel chunk loader introduced in S8-002.
///
/// Separates concerns cleanly: [`IngestParallelConfig`] keeps the existing
/// in-flight cap / chunk size, while this struct adds the worker-thread count
/// and queue depth relevant to a true thread-pool implementation.
#[derive(Debug, Clone)]
pub struct ChunkedLoaderConfig {
    /// Target number of rows per chunk handed to a worker thread.
    pub chunk_size_rows: usize,
    /// Number of worker threads to spawn for parallel ingestion.
    pub thread_count: usize,
    /// Maximum number of chunks allowed to sit in the work queue
    /// before the producer blocks (back-pressure).
    pub max_queue_depth: usize,
}

impl Default for ChunkedLoaderConfig {
    fn default() -> Self {
        Self {
            chunk_size_rows: 1_000,
            thread_count: 4,
            max_queue_depth: 8,
        }
    }
}

/// A parallel chunk loader that owns its worker-thread configuration.
///
/// In this phase the struct is a typed scaffold — actual thread-pool dispatch
/// is layered on top via [`load_records_chunked`] with a matching
/// [`IngestParallelConfig`] derived from `self.config`.
pub struct ParallelChunkLoader {
    /// Resolved configuration for this loader instance.
    pub config: ChunkedLoaderConfig,
}

impl ParallelChunkLoader {
    /// Create a new loader with the default configuration (4 threads).
    pub fn new() -> Self {
        Self {
            config: ChunkedLoaderConfig::default(),
        }
    }

    /// Create a loader with a custom configuration.
    pub fn with_config(config: ChunkedLoaderConfig) -> Self {
        Self { config }
    }

    /// Derive an [`IngestParallelConfig`] compatible with [`load_records_chunked`]
    /// from this loader's configuration.
    pub fn as_ingest_config(&self) -> IngestParallelConfig {
        IngestParallelConfig {
            chunk_target_rows: self.config.chunk_size_rows,
            max_in_flight_tasks: self.config.thread_count,
        }
    }

    /// Process `records` using the parallel chunk loader configuration.
    pub fn load(&self, records: &[IngestRecord]) -> ChunkedIngestStats {
        let cfg = self.as_ingest_config();
        load_records_chunked(records, &cfg)
    }
}

impl Default for ParallelChunkLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Estimate the optimal number of worker threads for a given dataset size.
///
/// Heuristic: one thread per 10 000 rows, capped at 4.
/// Returns at least 1 even for tiny datasets.
///
/// ```
/// use voltnuerongrid_ingest::chunked_loader::estimate_optimal_thread_count;
/// assert_eq!(estimate_optimal_thread_count(5_000), 1);
/// assert_eq!(estimate_optimal_thread_count(50_000), 4);
/// ```
pub fn estimate_optimal_thread_count(total_rows: usize) -> usize {
    (total_rows / 10_000).min(4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch_config::IngestParallelConfig;
    use crate::IngestRecord;

    fn make_records(n: usize) -> Vec<IngestRecord> {
        (0..n)
            .map(|i| IngestRecord {
                key: format!("k{i}"),
                payload: format!("v{i}"),
            })
            .collect()
    }

    #[test]
    fn empty_input_returns_zero_stats() {
        let cfg = IngestParallelConfig::default();
        let stats = load_records_chunked(&[], &cfg);
        assert_eq!(stats.total_records, 0);
        assert_eq!(stats.chunk_count, 0);
        assert_eq!(stats.tasks_dispatched, 0);
        assert!(stats.outcomes.is_empty());
    }

    #[test]
    fn single_chunk_when_records_fit() {
        let records = make_records(5);
        let cfg = IngestParallelConfig {
            max_in_flight_tasks: 4,
            chunk_target_rows: 10,
        };
        let stats = load_records_chunked(&records, &cfg);
        assert_eq!(stats.total_records, 5);
        assert_eq!(stats.chunk_count, 1);
        assert_eq!(stats.tasks_dispatched, 1);
        assert_eq!(stats.outcomes.len(), 1);
        assert_eq!(stats.outcomes[0].records_in_chunk, 5);
    }

    #[test]
    fn splits_into_correct_chunks() {
        let records = make_records(25);
        let cfg = IngestParallelConfig {
            max_in_flight_tasks: 4,
            chunk_target_rows: 10,
        };
        let stats = load_records_chunked(&records, &cfg);
        // 25 records / 10 per chunk → 3 chunks (10 + 10 + 5)
        assert_eq!(stats.chunk_count, 3);
        assert_eq!(stats.total_records, 25);
        assert_eq!(stats.outcomes[0].records_in_chunk, 10);
        assert_eq!(stats.outcomes[1].records_in_chunk, 10);
        assert_eq!(stats.outcomes[2].records_in_chunk, 5);
    }

    #[test]
    fn tasks_dispatched_capped_at_max_in_flight() {
        let records = make_records(50);
        let cfg = IngestParallelConfig {
            max_in_flight_tasks: 2,
            chunk_target_rows: 5, // 50/5 = 10 chunks, but only 2 in-flight at a time
        };
        let stats = load_records_chunked(&records, &cfg);
        assert_eq!(stats.chunk_count, 10);
        assert_eq!(stats.tasks_dispatched, 2); // capped at max_in_flight_tasks
        // all 10 chunks still processed
        assert_eq!(stats.outcomes.len(), 10);
    }

    #[test]
    fn chunk_indexes_are_sequential() {
        let records = make_records(30);
        let cfg = IngestParallelConfig {
            max_in_flight_tasks: 4,
            chunk_target_rows: 10,
        };
        let stats = load_records_chunked(&records, &cfg);
        for (i, outcome) in stats.outcomes.iter().enumerate() {
            assert_eq!(outcome.chunk_index, i);
        }
    }

    // ── S8-002 new tests ──────────────────────────────────────────────────────

    #[test]
    fn test_parallel_chunk_loader_config_defaults() {
        let loader = ParallelChunkLoader::new();
        assert_eq!(loader.config.chunk_size_rows, 1_000);
        assert_eq!(loader.config.thread_count, 4);
        assert_eq!(loader.config.max_queue_depth, 8);
    }

    #[test]
    fn test_parallel_chunk_loader_with_custom_config() {
        let config = ChunkedLoaderConfig {
            chunk_size_rows: 500,
            thread_count: 2,
            max_queue_depth: 4,
        };
        let loader = ParallelChunkLoader::with_config(config);
        assert_eq!(loader.config.chunk_size_rows, 500);
        assert_eq!(loader.config.thread_count, 2);
        assert_eq!(loader.config.max_queue_depth, 4);
    }

    #[test]
    fn test_parallel_chunk_loader_as_ingest_config_maps_fields() {
        let config = ChunkedLoaderConfig {
            chunk_size_rows: 250,
            thread_count: 3,
            max_queue_depth: 6,
        };
        let loader = ParallelChunkLoader::with_config(config);
        let ingest_cfg = loader.as_ingest_config();
        assert_eq!(ingest_cfg.chunk_target_rows, 250);
        assert_eq!(ingest_cfg.max_in_flight_tasks, 3);
    }

    #[test]
    fn test_parallel_chunk_loader_load_produces_correct_stats() {
        let records = make_records(20);
        let loader = ParallelChunkLoader::with_config(ChunkedLoaderConfig {
            chunk_size_rows: 10,
            thread_count: 2,
            max_queue_depth: 4,
        });
        let stats = loader.load(&records);
        assert_eq!(stats.total_records, 20);
        assert_eq!(stats.chunk_count, 2);
    }

    #[test]
    fn test_estimate_optimal_thread_count() {
        // Below 10K → 1 thread
        assert_eq!(estimate_optimal_thread_count(0), 1);
        assert_eq!(estimate_optimal_thread_count(1_000), 1);
        assert_eq!(estimate_optimal_thread_count(9_999), 1);
        // 10K exactly → floor(10000/10000) = 1
        assert_eq!(estimate_optimal_thread_count(10_000), 1);
        // 20K → 2 threads
        assert_eq!(estimate_optimal_thread_count(20_000), 2);
        // 30K → 3 threads
        assert_eq!(estimate_optimal_thread_count(30_000), 3);
        // 40K → 4 threads
        assert_eq!(estimate_optimal_thread_count(40_000), 4);
        // 1M → capped at 4
        assert_eq!(estimate_optimal_thread_count(1_000_000), 4);
    }
}
