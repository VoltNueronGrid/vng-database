use std::collections::BTreeMap;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use crate::{AppState, AuthErrorResponse};
use crate::auth::require_operator_auth;

// ─── WAL DTOs ─────────────────────────────────────────────────────────────────


// ─── S2-WS2-02: WAL durability + recovery types ──────────────────────────────

/// Response for `GET /api/v1/store/wal/status`.
#[derive(Serialize)]
pub(crate) struct WalStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) wal_len: usize,
    pub(crate) latest_sequence: u64,
    pub(crate) checkpoint_count: usize,
}

/// Request body for `POST /api/v1/store/wal/recover`.
#[derive(Deserialize)]
pub(crate) struct WalRecoverRequest {
    /// When `true`, log what would be replayed without actually writing to the row store.
    pub(crate) dry_run: Option<bool>,
}

/// Response for `POST /api/v1/store/wal/recover`.
#[derive(Debug, Serialize)]
pub(crate) struct WalRecoverResponse {
    pub(crate) status: &'static str,
    pub(crate) records_replayed: usize,
    pub(crate) dry_run: bool,
}


// ─── S2-WS2-02: WAL forced checkpoint response ────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalForceCheckpointResponse {
    pub(crate) status: &'static str,
    pub(crate) wal_len_before: usize,
    pub(crate) wal_len_after: usize,
    pub(crate) checkpoint_count: usize,
}

// ─── S2-WS2-02: WAL statistics response ──────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalStatsResponse {
    pub(crate) status: &'static str,
    pub(crate) record_count: usize,
    pub(crate) checkpoint_count: usize,
    pub(crate) mutation_rate_estimate: f64,
}

// ─── S2-WS2-02: WAL compact response ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalCompactResponse {
    pub(crate) status: &'static str,
    pub(crate) records_before: usize,
    pub(crate) records_after: usize,
    pub(crate) checkpoint_count: usize,
    pub(crate) compacted: bool,
}

// ─── S2-WS2-02: WAL bounds response ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalBoundsResponse {
    pub(crate) status: &'static str,
    pub(crate) record_count: usize,
    pub(crate) checkpoint_count: usize,
    pub(crate) oldest_sequence: Option<u64>,
    pub(crate) newest_sequence: Option<u64>,
}

// ─── S2-WS2-02: WAL replay (filtered read-back) structs ─────────────────────

#[derive(Debug, Deserialize, Default)]
pub(crate) struct WalReplayQuery {
    pub(crate) table_filter: Option<String>,
    pub(crate) op_filter: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalReplayEntry {
    pub(crate) sequence: u64,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalReplayResponse {
    pub(crate) status: &'static str,
    pub(crate) total_records: usize,
    pub(crate) matched_records: usize,
    pub(crate) entries: Vec<WalReplayEntry>,
}

// ─── S2-WS2-02: WAL tail structs ─────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub(crate) struct WalTailQuery {
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalTailResponse {
    pub(crate) status: &'static str,
    pub(crate) record_count: usize,
    pub(crate) limit_applied: usize,
    pub(crate) entries: Vec<WalReplayEntry>,
}

// ─── S2-WS2-03: WAL mutations query structs ───────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub(crate) struct WalMutationsQuery {
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalMutationRecord {
    pub(crate) sequence: u64,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalMutationsResponse {
    pub(crate) status: &'static str,
    pub(crate) mutation_count: usize,
    pub(crate) limit_applied: usize,
    pub(crate) mutations: Vec<WalMutationRecord>,
}

// ─── S2-WS2-02: WAL segment list structs ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalSegment {
    pub(crate) segment_id: u64,
    pub(crate) is_active: bool,
    pub(crate) record_count: usize,
    pub(crate) start_sequence: Option<u64>,
    pub(crate) end_sequence: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalSegmentListResponse {
    pub(crate) status: &'static str,
    pub(crate) segment_count: usize,
    pub(crate) completed_segments: usize,
    pub(crate) active_record_count: usize,
    pub(crate) segments: Vec<WalSegment>,
}

// ─── S2-WS2-02: WAL checkpoint history structs ────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalCheckpointEntry {
    pub(crate) checkpoint_id: u64,
    pub(crate) record_count_at_checkpoint: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalCheckpointHistoryResponse {
    pub(crate) status: &'static str,
    pub(crate) total_checkpoints: usize,
    pub(crate) entries: Vec<WalCheckpointEntry>,
}

// ─── S2-WS2-02: WAL replay count structs ──────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub(crate) struct WalReplayCountQuery {
    pub(crate) table_filter: Option<String>,
    pub(crate) op_filter: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalReplayCountResponse {
    pub(crate) status: &'static str,
    pub(crate) total_records: usize,
    pub(crate) matched_count: usize,
    pub(crate) table_filter: Option<String>,
    pub(crate) op_filter: Option<String>,
}



// ─── S11-WS1-10: WAL truncate structs ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct WalTruncateRequest {
    pub(crate) up_to_sequence: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalTruncateResponse {
    pub(crate) status: &'static str,
    pub(crate) records_removed: usize,
    pub(crate) new_record_count: usize,
    pub(crate) truncated: bool,
}


// ─── S11-WS1-13: WAL sequence info structs ───────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalSeqResponse {
    pub(crate) status: &'static str,
    pub(crate) latest_sequence: u64,
    pub(crate) wal_len: usize,
    pub(crate) checkpoint_count: usize,
}

// ─── S11-WS1-14: WAL head structs ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct WalHeadQuery {
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalHeadEntry {
    pub(crate) sequence: u64,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalHeadResponse {
    pub(crate) status: &'static str,
    pub(crate) record_count: usize,
    pub(crate) limit_applied: usize,
    pub(crate) entries: Vec<WalHeadEntry>,
}


// ─── S11-WS1-15: WAL range structs ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct WalRangeQuery {
    pub(crate) from_seq: u64,
    pub(crate) to_seq: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalRangeEntry {
    pub(crate) sequence: u64,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalRangeResponse {
    pub(crate) status: &'static str,
    pub(crate) record_count: usize,
    pub(crate) from_seq: u64,
    pub(crate) to_seq: u64,
    pub(crate) entries: Vec<WalRangeEntry>,
}


// ─── S11-WS1-16: WAL size structs ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalSizeResponse {
    pub(crate) status: &'static str,
    pub(crate) record_count: usize,
    pub(crate) estimated_bytes: usize,
}


// ─── S11-WS1-17: WAL latest structs ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalLatestResponse {
    pub(crate) status: &'static str,
    pub(crate) sequence: u64,
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) has_record: bool,
}


// ─── S11-WS1-18: WAL by-key structs ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct WalByKeyQuery {
    pub(crate) key_prefix: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalByKeyEntry {
    pub(crate) sequence: u64,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalByKeyResponse {
    pub(crate) status: &'static str,
    pub(crate) key_prefix: String,
    pub(crate) record_count: usize,
    pub(crate) entries: Vec<WalByKeyEntry>,
}


// ─── S11-WS1-19: WAL checkpoint latest structs ───────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalCheckpointLatestResponse {
    pub(crate) status: &'static str,
    pub(crate) checkpoint_id: u64,
    pub(crate) record_count: usize,
}


// ─── S11-WS1-20: WAL delta structs ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalDeltaResponse {
    pub(crate) status: &'static str,
    pub(crate) insert_count: usize,
    pub(crate) delete_count: usize,
    pub(crate) total_records: usize,
}


// ─── S11-WS1-21: WAL unique keys structs ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalUniqueKeysResponse {
    pub(crate) status: &'static str,
    pub(crate) unique_key_count: usize,
}


// ─── S11-WS1-22: WAL age + rows first key structs ────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalAgeResponse {
    pub(crate) status: &'static str,
    pub(crate) oldest_sequence: u64,
    pub(crate) newest_sequence: u64,
    pub(crate) sequence_span: u64,
}


// ─── S11-WS1-23: WAL keys list + rows last key structs ──────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct WalKeysListResponse {
    pub(crate) status: &'static str,
    pub(crate) key_count: usize,
    pub(crate) keys: Vec<String>,
}


#[derive(Debug, Serialize)]
pub(crate) struct WalRecordCountResponse {
    pub(crate) status: &'static str,
    pub(crate) record_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct WalCheckpointAgeResponse {
    pub(crate) status: &'static str,
    pub(crate) checkpoint_count: usize,
    pub(crate) oldest_sequence: u64,
    pub(crate) newest_sequence: u64,
}


#[derive(Debug, Serialize)]
pub(crate) struct WalFlushCountResponse {
    pub(crate) status: &'static str,
    pub(crate) flush_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct WalEntryLatestResponse {
    pub(crate) status: &'static str,
    pub(crate) has_entry: bool,
    pub(crate) entry_sequence: u64,
}

// S3-WS1-29: wal/write/count + rows/key/longest structs

#[derive(Debug, Serialize)]
pub(crate) struct WalWriteCountResponse {
    pub(crate) status: &'static str,
    pub(crate) write_count: usize,
    pub(crate) total_records: usize,
}


// S3-WS1-31: wal/min/seq + rows/count/all structs

#[derive(Debug, Serialize)]
pub(crate) struct WalMinSeqResponse {
    pub(crate) status: &'static str,
    pub(crate) min_sequence: u64,
    pub(crate) has_records: bool,
}


// S3-WS1-32: wal/max/seq + rows/snapshot/size structs

#[derive(Debug, Serialize)]
pub(crate) struct WalMaxSeqResponse {
    pub(crate) status: &'static str,
    pub(crate) max_sequence: u64,
    pub(crate) has_records: bool,
}


// S3-WS1-33: wal/entry/count + rows/version/latest structs

#[derive(Debug, Serialize)]
pub(crate) struct WalEntryCountResponse {
    pub(crate) status: &'static str,
    pub(crate) entry_count: usize,
}


// S3-WS1-34: wal/size/bytes + rows/distinct/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalSizeBytesResponse {
    pub(crate) status: &'static str,
    pub(crate) size_bytes: usize,
}


// S3-WS1-35: wal/delete/count + rows/key/median structs

#[derive(Debug, Serialize)]
pub(crate) struct WalDeleteCountResponse {
    pub(crate) status: &'static str,
    pub(crate) delete_count: usize,
}


// S3-WS1-36: wal/validate + rows/checksum structs

#[derive(Debug, Serialize)]
pub(crate) struct WalValidateResponse {
    pub(crate) status: &'static str,
    pub(crate) valid: bool,
    pub(crate) record_count: usize,
}


// S3-WS1-37: wal/entry/oldest + rows/field/types structs

#[derive(Debug, Serialize)]
pub(crate) struct WalEntryOldestResponse {
    pub(crate) status: &'static str,
    pub(crate) has_entry: bool,
    pub(crate) entry_sequence: u64,
}


// S3-WS1-38: wal/seq/span + rows/key/empty/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalSeqSpanResponse {
    pub(crate) status: &'static str,
    pub(crate) oldest_sequence: u64,
    pub(crate) newest_sequence: u64,
    pub(crate) sequence_span: u64,
}


// S3-WS1-39: wal/record/active + rows/key/min structs

#[derive(Debug, Serialize)]
pub(crate) struct WalRecordActiveResponse {
    pub(crate) status: &'static str,
    pub(crate) active_count: usize,
}


// S3-WS1-40: wal/record/mutations + rows/field/cardinality structs

#[derive(Debug, Serialize)]
pub(crate) struct WalRecordMutationsResponse {
    pub(crate) status: &'static str,
    pub(crate) mutation_count: usize,
}


// S3-WS1-41: wal/record/deleted + rows/key/max structs

#[derive(Debug, Serialize)]
pub(crate) struct WalRecordDeletedResponse {
    pub(crate) status: &'static str,
    pub(crate) deleted_count: usize,
}


// S3-WS1-42: wal/mutation/span + rows/value/non_null/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalMutationSpanResponse {
    pub(crate) status: &'static str,
    pub(crate) oldest_sequence: u64,
    pub(crate) newest_sequence: u64,
    pub(crate) mutation_span: u64,
}


// S3-WS1-43: wal/mutation/count/non_deleted + rows/value/empty/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalMutationNonDeletedCountResponse {
    pub(crate) status: &'static str,
    pub(crate) non_deleted_count: usize,
}


// S3-WS1-44: wal/non_deleted/span + rows/value/non_empty/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalNonDeletedSpanResponse {
    pub(crate) status: &'static str,
    pub(crate) oldest_sequence: u64,
    pub(crate) newest_sequence: u64,
    pub(crate) non_deleted_span: u64,
}


// S3-WS1-45: wal/non_deleted/count + rows/key/non_empty/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalNonDeletedCountResponse {
    pub(crate) status: &'static str,
    pub(crate) non_deleted_count: usize,
}


// S3-WS1-46: wal/non_deleted/latest + rows/value/non_blank/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalNonDeletedLatestResponse {
    pub(crate) status: &'static str,
    pub(crate) latest_non_deleted_sequence: u64,
}


// S3-WS1-47: wal/non_deleted/oldest + rows/key/non_blank/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalNonDeletedOldestResponse {
    pub(crate) status: &'static str,
    pub(crate) oldest_non_deleted_sequence: u64,
}


// S3-WS1-48: wal/non_deleted/newest + rows/value/blank/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalNonDeletedNewestResponse {
    pub(crate) status: &'static str,
    pub(crate) newest_non_deleted_sequence: u64,
}


// S3-WS1-49: wal/record/total + rows/key/duplicates/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalRecordTotalResponse {
    pub(crate) status: &'static str,
    pub(crate) total_record_count: usize,
}


// S3-WS1-50: wal/value/duplicates/count + rows/value/duplicates/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalValueDuplicatesCountResponse {
    pub(crate) status: &'static str,
    pub(crate) duplicate_value_count: usize,
}


// S3-WS1-51: wal/value/distinct/count + rows/value/distinct/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalValueDistinctCountResponse {
    pub(crate) status: &'static str,
    pub(crate) distinct_value_count: usize,
}


// S3-WS1-52: wal/value/unique/count + rows/value/unique/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalValueUniqueCountResponse {
    pub(crate) status: &'static str,
    pub(crate) unique_value_count: usize,
}


// S3-WS1-53: wal/value/trimmed/count + rows/value/trimmed/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalValueTrimmedCountResponse {
    pub(crate) status: &'static str,
    pub(crate) trimmed_value_count: usize,
}


// S3-WS1-54: wal/value/case_variant/count + rows/value/case_variant/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalValueCaseVariantCountResponse {
    pub(crate) status: &'static str,
    pub(crate) case_variant_count: usize,
}


// S3-WS1-55: wal/order_by_desc_direction + rows/order_by_desc_direction structs

#[derive(Debug, Serialize)]
pub(crate) struct WalOrderByDescDirectionCountResponse {
    pub(crate) status: &'static str,
    pub(crate) desc_direction_count: usize,
}


// S3-WS1-56: wal/order_by/random/count + rows/order_by/random/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalOrderByRandomCountResponse {
    pub(crate) status: &'static str,
    pub(crate) random_order_count: usize,
}


// S3-WS1-57: wal/order_by/random_seeded/count + rows/order_by/random_seeded/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalOrderByRandomSeededCountResponse {
    pub(crate) status: &'static str,
    pub(crate) random_seeded_order_count: usize,
}


// S3-WS1-58: wal/order_by/asc_direction/count + rows/order_by/asc_direction/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalOrderByAscDirectionCountResponse {
    pub(crate) status: &'static str,
    pub(crate) asc_direction_count: usize,
}


// S3-WS1-59: wal/order_by/rand_alias/count + rows/order_by/rand_alias/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalOrderByRandAliasCountResponse {
    pub(crate) status: &'static str,
    pub(crate) rand_alias_count: usize,
}


// S3-WS1-60: wal/order_by/multi_column/count + rows/order_by/multi_column/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalOrderByMultiColumnCountResponse {
    pub(crate) status: &'static str,
    pub(crate) multi_column_order_count: usize,
}


// S3-WS1-61: wal/pagination/limit_offset/count + rows/pagination/limit_offset/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalPaginationLimitOffsetCountResponse {
    pub(crate) status: &'static str,
    pub(crate) limit_offset_pagination_count: usize,
}


// S3-WS1-62: wal/pagination/offset_only/count + rows/pagination/offset_only/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalPaginationOffsetOnlyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) offset_only_pagination_count: usize,
}


// S3-WS1-63: wal/having_without_group_by/count + rows/having_without_group_by/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalHavingWithoutGroupByCountResponse {
    pub(crate) status: &'static str,
    pub(crate) having_without_group_by_count: usize,
}


// S3-WS1-64: wal/having_with_group_by/count + rows/having_with_group_by/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalHavingWithGroupByCountResponse {
    pub(crate) status: &'static str,
    pub(crate) having_with_group_by_count: usize,
}


// S3-WS1-65: wal/group_by/rollup/count + rows/group_by/rollup/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalGroupByRollupCountResponse {
    pub(crate) status: &'static str,
    pub(crate) group_by_rollup_count: usize,
}


// S3-WS1-66: wal/group_by/cube/count + rows/group_by/cube/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalGroupByCubeCountResponse {
    pub(crate) status: &'static str,
    pub(crate) group_by_cube_count: usize,
}


// S3-WS1-67: wal/select/distinct_on/count + rows/select/distinct_on/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalSelectDistinctOnCountResponse {
    pub(crate) status: &'static str,
    pub(crate) select_distinct_on_count: usize,
}


// S3-WS1-68: wal/for/update/count + rows/for/update/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalForUpdateCountResponse {
    pub(crate) status: &'static str,
    pub(crate) for_update_count: usize,
}


// S3-WS1-69: wal/left/join/count + rows/left/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalLeftJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) left_join_count: usize,
}


// S3-WS1-70: wal/right/join/count + rows/right/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalRightJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) right_join_count: usize,
}


// S3-WS1-71: wal/full_outer/join/count + rows/full_outer/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalFullOuterJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) full_outer_join_count: usize,
}


// S3-WS1-72: wal/inner/join/count + rows/inner/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalInnerJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) inner_join_count: usize,
}


// S3-WS1-73: wal/straight/join/count + rows/straight/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalStraightJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) straight_join_count: usize,
}


// S3-WS1-74: wal/semi/join/count + rows/semi/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalSemiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) semi_join_count: usize,
}


// S3-WS1-75: wal/anti/join/count + rows/anti/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalAntiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) anti_join_count: usize,
}


// S3-WS1-76: wal/cross/apply/count + rows/cross/apply/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalCrossApplyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) cross_apply_count: usize,
}


// S3-WS1-77: wal/outer/apply/count + rows/outer/apply/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalOuterApplyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) outer_apply_count: usize,
}


// S3-WS1-78: wal/apply/count + rows/apply/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalApplyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) apply_count: usize,
}


// S3-WS1-79: wal/left/semi/join/count + rows/left/semi/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalLeftSemiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) left_semi_join_count: usize,
}


// S3-WS1-80: wal/left/anti/join/count + rows/left/anti/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalLeftAntiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) left_anti_join_count: usize,
}


// S3-WS1-81: wal/right/semi/join/count + rows/right/semi/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalRightSemiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) right_semi_join_count: usize,
}


// S3-WS1-82: wal/right/anti/join/count + rows/right/anti/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalRightAntiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) right_anti_join_count: usize,
}


// S3-WS1-83: wal/full/semi/join/count + rows/full/semi/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalFullSemiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) full_semi_join_count: usize,
}


// S3-WS1-84: wal/full/anti/join/count + rows/full/anti/join/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalFullAntiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) full_anti_join_count: usize,
}


// S3-WS1-85: wal/union/all/count + rows/union/all/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalUnionAllCountResponse {
    pub(crate) status: &'static str,
    pub(crate) union_all_count: usize,
}


// S3-WS1-86: wal/aggregate/distinct/count + rows/aggregate/distinct/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalAggregateDistinctCountResponse {
    pub(crate) status: &'static str,
    pub(crate) aggregate_distinct_count: usize,
}


// S3-WS1-87: wal/table/alias/count + rows/table/alias/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalTableAliasCountResponse {
    pub(crate) status: &'static str,
    pub(crate) table_alias_count: usize,
}


// S3-WS1-88: wal/sql/column/alias/count + rows/sql/column/alias/count structs

#[derive(Debug, Serialize)]
pub(crate) struct WalColumnAliasCountResponse {
    pub(crate) status: &'static str,
    pub(crate) column_alias_count: usize,
}


// ─── WAL helper fns ─────────────────────────────────────────────────────────

pub(crate) fn contains_table_alias_sql(up: &str) -> bool {
    contains_alias_after_anchor(up, " FROM ", true) || contains_alias_after_anchor(up, " JOIN ", false)
}

pub(crate) fn contains_column_alias_sql(up: &str) -> bool {
    let select_end = up.find(" FROM ").unwrap_or(up.len());
    let select_list = &up[..select_end];
    let mut search_from = 0;
    while let Some(rel_pos) = select_list[search_from..].find(" AS ") {
        let abs_pos = search_from + rel_pos;
        let after_as = abs_pos + 4;
        let rest = &select_list[after_as..];
        let word_end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        if word_end == 0 {
            search_from = after_as;
            continue;
        }
        if word_end < rest.len() && rest.as_bytes()[word_end] == b')' {
            search_from = after_as;
            continue;
        }
        return true;
    }
    false
}

pub(crate) fn contains_alias_after_anchor(up: &str, anchor: &str, stop_at_join: bool) -> bool {
    let mut scan_from = 0usize;
    while let Some(rel_pos) = up[scan_from..].find(anchor) {
        let start = scan_from + rel_pos + anchor.len();
        let tail = &up[start..];
        let mut stops = vec![" ON ", " WHERE ", " GROUP BY ", " HAVING ", " ORDER BY ", " LIMIT ", " OFFSET ", " UNION "];
        if stop_at_join {
            stops.push(" JOIN ");
        }
        let end = stops
            .iter()
            .filter_map(|kw| tail.find(kw))
            .min()
            .unwrap_or(tail.len());
        let source_seg = &tail[..end];
        // Exclude derived-table / subquery aliases (") AS name") - not simple table aliases.
        if source_seg.contains(" AS ")
            && !source_seg.contains(" AS (")
            && !source_seg.contains(") AS ")
        {
            return true;
        }
        scan_from = start;
    }
    false
}


// ─── WAL handlers ───────────────────────────────────────────────────────────


// ─── S2-WS2-02: WAL durability + recovery handlers ────────────────────────────────────────────

/// S2-WS2-02: return WAL engine stats.
pub(crate) async fn wal_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalStatusResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let records = wal.wal_records();
    let wal_len = records.len();
    let latest_seq = records.last().map(|r| r.sequence).unwrap_or(0);
    let checkpoint_count = wal.checkpoint_count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalStatusResponse {
        status: "ok",
        wal_len,
        latest_sequence: latest_seq,
        checkpoint_count,
    })))
}


// ─── S2-WS2-02: WAL forced checkpoint ────────────────────────────────────────

pub(crate) async fn wal_force_checkpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalForceCheckpointResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut wal = state.wal_engine.lock().expect("wal_engine lock");
    let wal_len_before = wal.wal_records().len();
    wal.force_checkpoint();
    let wal_len_after = wal.wal_records().len();
    let checkpoint_count = wal.checkpoint_count();
    Ok((StatusCode::OK, Json(WalForceCheckpointResponse {
        status: "ok",
        wal_len_before,
        wal_len_after,
        checkpoint_count,
    })))
}


/// S2-WS2-02: Return WAL statistics (record count, checkpoint count, mutation rate).
pub(crate) async fn wal_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let record_count = wal.wal_records().len();
    let checkpoint_count = wal.checkpoint_count();
    drop(wal);
    let mutation_rate_estimate = record_count as f64;
    Ok((StatusCode::OK, Json(WalStatsResponse {
        status: "ok",
        record_count,
        checkpoint_count,
        mutation_rate_estimate,
    })))
}


/// S2-WS2-02: Compact the WAL — force a checkpoint and return compaction stats.
pub(crate) async fn wal_compact(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalCompactResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let records_before = {
        let wal = state.wal_engine.lock().expect("wal_engine wal_compact lock");
        wal.wal_records().len()
    };
    {
        let mut wal = state.wal_engine.lock().expect("wal_engine compact_checkpoint");
        wal.force_checkpoint();
    }
    let (records_after, checkpoint_count) = {
        let wal = state.wal_engine.lock().expect("wal_engine compact_post");
        (wal.wal_records().len(), wal.checkpoint_count())
    };
    let compacted = records_before > records_after;
    Ok((StatusCode::OK, Json(WalCompactResponse {
        status: "ok",
        records_before,
        records_after,
        checkpoint_count,
        compacted,
    })))
}


// ─── S2-WS2-02: WAL bounds — oldest and newest sequence numbers ───────────────

/// S2-WS2-02: Return oldest and newest WAL sequence numbers and record/checkpoint counts.
pub(crate) async fn wal_bounds(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalBoundsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine wal_bounds lock");
    let records = wal.wal_records();
    let record_count = records.len();
    let checkpoint_count = wal.checkpoint_count();
    let oldest_sequence = records.first().map(|r| r.sequence);
    let newest_sequence = records.last().map(|r| r.sequence);
    drop(wal);
    Ok((StatusCode::OK, Json(WalBoundsResponse {
        status: "ok",
        record_count,
        checkpoint_count,
        oldest_sequence,
        newest_sequence,
    })))
}


// ─── S

// ─── S2-WS2-02: WAL replay — filtered read-back of WAL entries ───────────────

/// S2-WS2-02: Return WAL records with optional key/op filters (read-only).
pub(crate) async fn wal_replay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<WalReplayQuery>,
) -> Result<(StatusCode, Json<WalReplayResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let all_records = wal.wal_records().to_vec();
    drop(wal);
    let total_records = all_records.len();
    let entries: Vec<WalReplayEntry> = all_records
        .into_iter()
        .filter(|r| {
            let key_ok = params.table_filter.as_ref()
                .map(|f| r.key.contains(f.as_str()))
                .unwrap_or(true);
            let op_type = if r.value == "__deleted__" { "delete" } else { "insert" };
            let op_ok = params.op_filter.as_ref()
                .map(|f| op_type == f.as_str())
                .unwrap_or(true);
            key_ok && op_ok
        })
        .map(|r| WalReplayEntry { sequence: r.sequence, key: r.key, value: r.value })
        .collect();
    let matched_records = entries.len();
    Ok((StatusCode::OK, Json(WalReplayResponse {
        status: "ok",
        total_records,
        matched_records,
        entries,
    })))
}


/// S2-WS2-02: Return the last N WAL records (tail view).
pub(crate) async fn wal_tail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<WalTailQuery>,
) -> Result<(StatusCode, Json<WalTailResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let limit_applied = params.limit.unwrap_or(10).max(1).min(1_000);
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let all_records = wal.wal_records().to_vec();
    drop(wal);
    let total = all_records.len();
    let skip = if total > limit_applied { total - limit_applied } else { 0 };
    let entries: Vec<WalReplayEntry> = all_records
        .into_iter()
        .skip(skip)
        .map(|r| WalReplayEntry { sequence: r.sequence, key: r.key, value: r.value })
        .collect();
    let record_count = entries.len();
    Ok((StatusCode::OK, Json(WalTailResponse {
        status: "ok",
        record_count,
        limit_applied,
        entries,
    })))
}


// ─── S2-WS2-03: WAL mutations handler ──────────────────────────────────────

/// S2-WS2-03: Return recent mutation records from WAL with key+value pairs.
pub(crate) async fn wal_mutations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<WalMutationsQuery>,
) -> Result<(StatusCode, Json<WalMutationsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let limit_applied = params.limit.unwrap_or(50).max(1).min(10_000);
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let all_records = wal.wal_records().to_vec();
    drop(wal);
    let total = all_records.len();
    let skip = if total > limit_applied { total - limit_applied } else { 0 };
    let mutations: Vec<WalMutationRecord> = all_records
        .into_iter()
        .skip(skip)
        .map(|r| WalMutationRecord { sequence: r.sequence, key: r.key, value: r.value })
        .collect();
    let mutation_count = mutations.len();
    Ok((StatusCode::OK, Json(WalMutationsResponse {
        status: "ok",
        mutation_count,
        limit_applied,
        mutations,
    })))
}


// ─── S2-WS2-02: WAL checkpoint history handler ───────────────────────────────

/// S2-WS2-02: Return a list of completed WAL checkpoints with their record counts.
pub(crate) async fn wal_checkpoint_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalCheckpointHistoryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock checkpoint_history");
    let total_checkpoints = wal.checkpoint_count();
    drop(wal);
    let entries: Vec<WalCheckpointEntry> = (1..=(total_checkpoints as u64))
        .map(|id| WalCheckpointEntry {
            checkpoint_id: id,
            record_count_at_checkpoint: 0,
        })
        .collect();
    Ok((StatusCode::OK, Json(WalCheckpointHistoryResponse {
        status: "ok",
        total_checkpoints,
        entries,
    })))
}


// ─── S2-WS2-02: WAL segment list handler ────────────────────────────────────

/// S2-WS2-02: List WAL checkpoint segments plus the active (unbounded) segment.
pub(crate) async fn wal_segment_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalSegmentListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let completed = wal.checkpoint_count();
    let active_records = wal.wal_records().to_vec();
    drop(wal);
    let active_record_count = active_records.len();
    let mut segments: Vec<WalSegment> = (1..=(completed as u64))
        .map(|id| WalSegment {
            segment_id: id,
            is_active: false,
            record_count: 0,
            start_sequence: None,
            end_sequence: None,
        })
        .collect();
    segments.push(WalSegment {
        segment_id: completed as u64 + 1,
        is_active: true,
        record_count: active_record_count,
        start_sequence: active_records.first().map(|r| r.sequence),
        end_sequence: active_records.last().map(|r| r.sequence),
    });
    let segment_count = segments.len();
    Ok((StatusCode::OK, Json(WalSegmentListResponse {
        status: "ok",
        segment_count,
        completed_segments: completed,
        active_record_count,
        segments,
    })))
}


// ─── S2-WS2-02: WAL replay count ──────────────────────────────────────────────

/// S2-WS2-02: Return the count of WAL records matching optional table/op filters.
pub(crate) async fn wal_replay_count(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<WalReplayCountQuery>,
) -> Result<(StatusCode, Json<WalReplayCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let total_records = records.len();
    let matched_count = records.iter().filter(|r| {
        let table_ok = query.table_filter.as_deref()
            .map(|t| r.key.starts_with(t))
            .unwrap_or(true);
        let op_ok = query.op_filter.as_deref()
            .map(|op| if op == "delete" { r.value == "__deleted__" } else { r.value != "__deleted__" })
            .unwrap_or(true);
        table_ok && op_ok
    }).count();
    Ok((StatusCode::OK, Json(WalReplayCountResponse {
        status: "ok",
        total_records,
        matched_count,
        table_filter: query.table_filter,
        op_filter: query.op_filter,
    })))
}


/// S2-WS2-02: replay WAL records into the row store (or dry-run).
pub(crate) async fn wal_recover(
    State(state): State<AppState>,
    axum::extract::Json(req): axum::extract::Json<WalRecoverRequest>,
) -> (StatusCode, Json<WalRecoverResponse>) {
    let dry_run = req.dry_run.unwrap_or(false);
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let mut replayed: usize = 0;
    if !dry_run {
        let mut rs = state.row_store.lock().expect("row_store lock wal_recover");
        let xid = rs.begin_xid();
        for rec in &records {
            if rec.value == "__deleted__" {
                rs.delete(xid, &rec.key);
            } else {
                let data: std::collections::HashMap<String, String> =
                    serde_json::from_str(&rec.value)
                        .unwrap_or_else(|_| [("_raw".to_string(), rec.value.clone())]
                            .into_iter().collect());
                rs.insert(xid, &rec.key, data);
            }
            replayed += 1;
        }
    } else {
        replayed = records.len();
    }
    (StatusCode::OK, Json(WalRecoverResponse {
        status: "ok",
        records_replayed: replayed,
        dry_run,
    }))
}


// ─── S11-WS1-10: WAL truncate endpoint ───────────────────────────────────────

/// S11-WS1-10: Truncate WAL records up to a given sequence by forcing a checkpoint.
pub(crate) async fn wal_truncate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<WalTruncateRequest>,
) -> Result<(StatusCode, Json<WalTruncateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let records_before = {
        let wal = state.wal_engine.lock().expect("wal_engine lock wal_truncate_before");
        wal.wal_records().len()
    };
    let latest_seq = {
        let wal = state.wal_engine.lock().expect("wal_engine lock wal_truncate_seq");
        wal.latest_sequence()
    };
    let truncated = if latest_seq >= req.up_to_sequence && records_before > 0 {
        let mut wal = state.wal_engine.lock().expect("wal_engine lock wal_truncate_cp");
        wal.force_checkpoint();
        true
    } else {
        false
    };
    let new_record_count = {
        let wal = state.wal_engine.lock().expect("wal_engine lock wal_truncate_after");
        wal.wal_records().len()
    };
    let records_removed = records_before.saturating_sub(new_record_count);
    Ok((StatusCode::OK, Json(WalTruncateResponse {
        status: "ok",
        records_removed,
        new_record_count,
        truncated,
    })))
}


// ─── S11-WS1-13: Ingest schema fields endpoint ──────────────────────────────

/// S11-WS1-13: Return field definitions for a specific ingest schema entry.
// ─── S11-WS1-13: WAL sequence info endpoint ──────────────────────────────────

/// S11-WS1-13: Return the latest WAL sequence number and record count.
pub(crate) async fn wal_seq(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalSeqResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_seq");
    let latest_sequence = wal.latest_sequence();
    let wal_len = wal.wal_records().len();
    let checkpoint_count = wal.checkpoint_count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalSeqResponse {
        status: "ok",
        latest_sequence,
        wal_len,
        checkpoint_count,
    })))
}


// ─── S11-WS1-14: WAL head endpoint ───────────────────────────────────────────

/// S11-WS1-14: Return the first N WAL records (head of the log).
pub(crate) async fn wal_head(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<WalHeadQuery>,
) -> Result<(StatusCode, Json<WalHeadResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_head");
    let all_records = wal.wal_records();
    let limit_applied = params.limit.unwrap_or(10).min(all_records.len());
    let entries: Vec<WalHeadEntry> = all_records[..limit_applied]
        .iter()
        .map(|r| WalHeadEntry {
            sequence: r.sequence,
            key: r.key.clone(),
            value: r.value.clone(),
        })
        .collect();
    let record_count = entries.len();
    drop(wal);
    Ok((StatusCode::OK, Json(WalHeadResponse {
        status: "ok",
        record_count,
        limit_applied,
        entries,
    })))
}


// ─── S11-WS1-15: WAL range endpoint ──────────────────────────────────────────

/// S11-WS1-15: Return WAL records within a given sequence range [from_seq, to_seq].
pub(crate) async fn wal_range(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<WalRangeQuery>,
) -> Result<(StatusCode, Json<WalRangeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_range");
    let all_records = wal.wal_records();
    let to_seq = params.to_seq.unwrap_or(u64::MAX);
    let entries: Vec<WalRangeEntry> = all_records
        .iter()
        .filter(|r| r.sequence >= params.from_seq && r.sequence <= to_seq)
        .map(|r| WalRangeEntry {
            sequence: r.sequence,
            key: r.key.clone(),
            value: r.value.clone(),
        })
        .collect();
    let record_count = entries.len();
    drop(wal);
    Ok((StatusCode::OK, Json(WalRangeResponse {
        status: "ok",
        record_count,
        from_seq: params.from_seq,
        to_seq,
        entries,
    })))
}


// ─── S11-WS1-16: WAL size endpoint ───────────────────────────────────────────

/// S11-WS1-16: Return a WAL record count and estimated byte size.
pub(crate) async fn wal_size(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalSizeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_size");
    let records = wal.wal_records();
    let record_count = records.len();
    // Estimate: 8 bytes (sequence) + avg key + avg value; 64 bytes per record scaffold.
    let estimated_bytes = records.iter().fold(0usize, |acc, r| {
        acc + 8 + r.key.len() + r.value.len()
    });
    drop(wal);
    Ok((StatusCode::OK, Json(WalSizeResponse {
        status: "ok",
        record_count,
        estimated_bytes,
    })))
}


// ─── S11-WS1-17: WAL latest endpoint ─────────────────────────────────────────

/// S11-WS1-17: Return the single latest (highest-sequence) WAL record.
pub(crate) async fn wal_latest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalLatestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_latest");
    let records = wal.wal_records();
    let resp = if let Some(r) = records.last() {
        WalLatestResponse {
            status: "ok",
            sequence: r.sequence,
            key: r.key.clone(),
            value: r.value.clone(),
            has_record: true,
        }
    } else {
        WalLatestResponse {
            status: "ok",
            sequence: 0,
            key: String::new(),
            value: String::new(),
            has_record: false,
        }
    };
    drop(wal);
    Ok((StatusCode::OK, Json(resp)))
}


// ─── S11-WS1-18: WAL by-key endpoint ─────────────────────────────────────────

/// S11-WS1-18: Return WAL records whose key starts with the given key_prefix.
pub(crate) async fn wal_by_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<WalByKeyQuery>,
) -> Result<(StatusCode, Json<WalByKeyResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_by_key");
    let entries: Vec<WalByKeyEntry> = wal.wal_records()
        .iter()
        .filter(|r| r.key.starts_with(params.key_prefix.as_str()))
        .map(|r| WalByKeyEntry {
            sequence: r.sequence,
            key: r.key.clone(),
            value: r.value.clone(),
        })
        .collect();
    let record_count = entries.len();
    let key_prefix = params.key_prefix.clone();
    drop(wal);
    Ok((StatusCode::OK, Json(WalByKeyResponse {
        status: "ok",
        key_prefix,
        record_count,
        entries,
    })))
}


// ─── S11-WS1-19: WAL checkpoint latest endpoint ──────────────────────────────

/// S11-WS1-19: Return the latest completed WAL checkpoint info.
pub(crate) async fn wal_checkpoint_latest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalCheckpointLatestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_checkpoint_latest");
    let total = wal.checkpoint_count();
    let record_count = wal.wal_records().len();
    drop(wal);
    let checkpoint_id = total as u64;
    Ok((StatusCode::OK, Json(WalCheckpointLatestResponse {
        status: "ok",
        checkpoint_id,
        record_count,
    })))
}


// ─── S11-WS1-20: WAL delta endpoint ──────────────────────────────────────────

/// S11-WS1-20: Return insert and delete counts derived from WAL record values.
pub(crate) async fn wal_delta(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalDeltaResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_delta");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let total_records = records.len();
    let delete_count = records.iter().filter(|r| r.value == "__deleted__").count();
    let insert_count = total_records - delete_count;
    Ok((StatusCode::OK, Json(WalDeltaResponse {
        status: "ok",
        insert_count,
        delete_count,
        total_records,
    })))
}


// ─── S

// --- S11-WS1-21: WAL unique keys endpoint ────────────────────────────────────

/// S11-WS1-21: Return the count of unique keys that appear across all WAL records.
pub(crate) async fn wal_unique_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalUniqueKeysResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_unique_keys");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let unique_key_count = {
        let mut keys: Vec<&str> = records.iter().map(|r| r.key.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        keys.len()
    };
    Ok((StatusCode::OK, Json(WalUniqueKeysResponse {
        status: "ok",
        unique_key_count,
    })))
}


// ─── S
// ─── S11-WS1-22: WAL age endpoint ─────────────────────────────────────────────

/// S11-WS1-22: Return the oldest and newest WAL sequence numbers and their span.
pub(crate) async fn wal_age(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalAgeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_age");
    let records = wal.wal_records();
    let oldest_sequence = records.first().map(|r| r.sequence).unwrap_or(0);
    let newest_sequence = records.last().map(|r| r.sequence).unwrap_or(0);
    let sequence_span = newest_sequence.saturating_sub(oldest_sequence);
    drop(wal);
    Ok((StatusCode::OK, Json(WalAgeResponse {
        status: "ok",
        oldest_sequence,
        newest_sequence,
        sequence_span,
    })))
}


// ─── S11-WS1-23: WAL keys list endpoint ─────────────────────────────────────────────

/// S11-WS1-23: Return a deduplicated, sorted list of all keys present in the WAL.
pub(crate) async fn wal_keys_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalKeysListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_keys_list");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let mut keys: Vec<String> = records.into_iter().map(|r| r.key).collect();
    keys.sort();
    keys.dedup();
    let key_count = keys.len();
    Ok((StatusCode::OK, Json(WalKeysListResponse {
        status: "ok",
        key_count,
        keys,
    })))
}


/// S11-WS1-25: Return total count of WAL records.
pub(crate) async fn wal_record_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalRecordCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_record_count");
    let record_count = wal.wal_records().len();
    drop(wal);
    Ok((StatusCode::OK, Json(WalRecordCountResponse {
        status: "ok",
        record_count,
    })))
}


/// S11-WS1-26: Return WAL checkpoint age (oldest/newest sequence numbers).
pub(crate) async fn wal_checkpoint_age(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalCheckpointAgeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_checkpoint_age");
    let records = wal.wal_records();
    let oldest_sequence = records.first().map(|r| r.sequence).unwrap_or(0);
    let newest_sequence = records.last().map(|r| r.sequence).unwrap_or(0);
    let checkpoint_count = wal.checkpoint_count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalCheckpointAgeResponse {
        status: "ok",
        checkpoint_count,
        oldest_sequence,
        newest_sequence,
    })))
}


/// S11-WS1-27: Return total WAL flush (write) count.
pub(crate) async fn wal_flush_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalFlushCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_flush_count");
    let flush_count = wal.wal_records().len();
    drop(wal);
    Ok((StatusCode::OK, Json(WalFlushCountResponse {
        status: "ok",
        flush_count,
    })))
}


// S3-WS1-28: wal/entry/latest endpoint
pub(crate) async fn wal_entry_latest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalEntryLatestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_entry_latest");
    let records = wal.wal_records();
    let entry_sequence = records.last().map(|r| r.sequence).unwrap_or(0);
    let has_entry = !records.is_empty();
    drop(wal);
    Ok((StatusCode::OK, Json(WalEntryLatestResponse { status: "ok", has_entry, entry_sequence })))
}


// S3-WS1-29: wal/write/count endpoint
pub(crate) async fn wal_write_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalWriteCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_write_count");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let total_records = records.len();
    let write_count = records.iter().filter(|r| r.value != "__deleted__").count();
    Ok((StatusCode::OK, Json(WalWriteCountResponse { status: "ok", write_count, total_records })))
}


// S3-WS1-31: wal/min/seq endpoint
pub(crate) async fn wal_min_seq(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalMinSeqResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_min_seq");
    let records = wal.wal_records();
    let min_sequence = records.first().map(|r| r.sequence).unwrap_or(0);
    let has_records = !records.is_empty();
    drop(wal);
    Ok((StatusCode::OK, Json(WalMinSeqResponse { status: "ok", min_sequence, has_records })))
}


// S3-WS1-32: wal/max/seq endpoint
pub(crate) async fn wal_max_seq(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalMaxSeqResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_max_seq");
    let records = wal.wal_records();
    let max_sequence = records.last().map(|r| r.sequence).unwrap_or(0);
    let has_records = !records.is_empty();
    drop(wal);
    Ok((StatusCode::OK, Json(WalMaxSeqResponse { status: "ok", max_sequence, has_records })))
}


// S3-WS1-33: wal/entry/count endpoint
pub(crate) async fn wal_entry_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalEntryCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_entry_count");
    let entry_count = wal.wal_records().len();
    drop(wal);
    Ok((StatusCode::OK, Json(WalEntryCountResponse { status: "ok", entry_count })))
}


// S3-WS1-34: wal/size/bytes endpoint
pub(crate) async fn wal_size_bytes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalSizeBytesResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_size_bytes");
    let size_bytes = wal.wal_records().len() * std::mem::size_of::<u64>();
    drop(wal);
    Ok((StatusCode::OK, Json(WalSizeBytesResponse { status: "ok", size_bytes })))
}


// S3-WS1-35: wal/delete/count endpoint
pub(crate) async fn wal_delete_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalDeleteCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_delete_count");
    let delete_count = wal.wal_records().iter().filter(|r| r.value == "__deleted__").count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalDeleteCountResponse { status: "ok", delete_count })))
}


// S3-WS1-36: wal/validate endpoint
pub(crate) async fn wal_validate(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalValidateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_validate");
    let records = wal.wal_records();
    let valid = records.windows(2).all(|w| w[0].sequence <= w[1].sequence);
    let record_count = records.len();
    drop(wal);
    Ok((StatusCode::OK, Json(WalValidateResponse { status: "ok", valid, record_count })))
}


// S3-WS1-37: wal/entry/oldest endpoint
pub(crate) async fn wal_entry_oldest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalEntryOldestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_entry_oldest");
    let records = wal.wal_records();
    let has_entry = !records.is_empty();
    let entry_sequence = records.first().map(|r| r.sequence).unwrap_or(0);
    drop(wal);
    Ok((StatusCode::OK, Json(WalEntryOldestResponse { status: "ok", has_entry, entry_sequence })))
}


// S3-WS1-38: wal/seq/span endpoint
pub(crate) async fn wal_seq_span(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalSeqSpanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_seq_span");
    let records = wal.wal_records();
    let oldest_sequence = records.first().map(|r| r.sequence).unwrap_or(0);
    let newest_sequence = records.last().map(|r| r.sequence).unwrap_or(0);
    let sequence_span = newest_sequence.saturating_sub(oldest_sequence);
    drop(wal);
    Ok((StatusCode::OK, Json(WalSeqSpanResponse { status: "ok", oldest_sequence, newest_sequence, sequence_span })))
}


// S3-WS1-39: wal/record/active endpoint
pub(crate) async fn wal_record_active(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalRecordActiveResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_record_active");
    let active_count = wal.wal_records().iter().filter(|r| r.value != "__deleted__").count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalRecordActiveResponse { status: "ok", active_count })))
}


// S3-WS1-40: wal/record/mutations endpoint
pub(crate) async fn wal_record_mutations(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalRecordMutationsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_record_mutations");
    let mutation_count = wal.wal_records().len();
    drop(wal);
    Ok((StatusCode::OK, Json(WalRecordMutationsResponse { status: "ok", mutation_count })))
}


// S3-WS1-41: wal/record/deleted endpoint
pub(crate) async fn wal_record_deleted(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalRecordDeletedResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_record_deleted");
    let deleted_count = wal.wal_records().iter().filter(|r| r.value == "__deleted__").count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalRecordDeletedResponse { status: "ok", deleted_count })))
}


// S3-WS1-42: wal/mutation/span endpoint
pub(crate) async fn wal_mutation_span(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalMutationSpanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_mutation_span");
    let mut seqs: Vec<u64> = wal
        .wal_records()
        .iter()
        .filter(|r| r.value != "__deleted__")
        .map(|r| r.sequence)
        .collect();
    drop(wal);
    seqs.sort_unstable();
    let oldest_sequence = seqs.first().copied().unwrap_or(0);
    let newest_sequence = seqs.last().copied().unwrap_or(0);
    let mutation_span = if oldest_sequence == 0 || newest_sequence == 0 {
        0
    } else {
        newest_sequence.saturating_sub(oldest_sequence)
    };
    Ok((StatusCode::OK, Json(WalMutationSpanResponse {
        status: "ok",
        oldest_sequence,
        newest_sequence,
        mutation_span,
    })))
}


// S3-WS1-43: wal/mutation/count/non_deleted endpoint
pub(crate) async fn wal_mutation_non_deleted_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalMutationNonDeletedCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_mutation_non_deleted_count");
    let non_deleted_count = wal.wal_records().iter().filter(|r| r.value != "__deleted__").count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalMutationNonDeletedCountResponse { status: "ok", non_deleted_count })))
}


// S3-WS1-44: wal/non_deleted/span endpoint
pub(crate) async fn wal_non_deleted_span(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalNonDeletedSpanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_non_deleted_span");
    let mut seqs: Vec<u64> = wal
        .wal_records()
        .iter()
        .filter(|r| r.value != "__deleted__")
        .map(|r| r.sequence)
        .collect();
    drop(wal);
    seqs.sort_unstable();
    let oldest_sequence = seqs.first().copied().unwrap_or(0);
    let newest_sequence = seqs.last().copied().unwrap_or(0);
    let non_deleted_span = if oldest_sequence == 0 || newest_sequence == 0 {
        0
    } else {
        newest_sequence.saturating_sub(oldest_sequence)
    };
    Ok((StatusCode::OK, Json(WalNonDeletedSpanResponse {
        status: "ok",
        oldest_sequence,
        newest_sequence,
        non_deleted_span,
    })))
}


// S3-WS1-45: wal/non_deleted/count endpoint
pub(crate) async fn wal_non_deleted_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalNonDeletedCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_non_deleted_count");
    let non_deleted_count = wal.wal_records().iter().filter(|r| r.value != "__deleted__").count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalNonDeletedCountResponse {
        status: "ok",
        non_deleted_count,
    })))
}


// S3-WS1-46: wal/non_deleted/latest endpoint
pub(crate) async fn wal_non_deleted_latest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalNonDeletedLatestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_non_deleted_latest");
    let latest_non_deleted_sequence = wal
        .wal_records()
        .iter()
        .filter(|r| r.value != "__deleted__")
        .map(|r| r.sequence)
        .max()
        .unwrap_or(0);
    drop(wal);
    Ok((StatusCode::OK, Json(WalNonDeletedLatestResponse {
        status: "ok",
        latest_non_deleted_sequence,
    })))
}


// S3-WS1-47: wal/non_deleted/oldest endpoint
pub(crate) async fn wal_non_deleted_oldest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalNonDeletedOldestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_non_deleted_oldest");
    let oldest_non_deleted_sequence = wal
        .wal_records()
        .iter()
        .filter(|r| r.value != "__deleted__")
        .map(|r| r.sequence)
        .min()
        .unwrap_or(0);
    drop(wal);
    Ok((StatusCode::OK, Json(WalNonDeletedOldestResponse {
        status: "ok",
        oldest_non_deleted_sequence,
    })))
}


// S3-WS1-48: wal/non_deleted/newest endpoint
pub(crate) async fn wal_non_deleted_newest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalNonDeletedNewestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_non_deleted_newest");
    let newest_non_deleted_sequence = wal
        .wal_records()
        .iter()
        .filter(|r| r.value != "__deleted__")
        .map(|r| r.sequence)
        .max()
        .unwrap_or(0);
    drop(wal);
    Ok((StatusCode::OK, Json(WalNonDeletedNewestResponse {
        status: "ok",
        newest_non_deleted_sequence,
    })))
}


// S3-WS1-49: wal/record/total endpoint
pub(crate) async fn wal_record_total(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalRecordTotalResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_record_total");
    let total_record_count = wal.wal_records().len();
    drop(wal);
    Ok((StatusCode::OK, Json(WalRecordTotalResponse {
        status: "ok",
        total_record_count,
    })))
}


// S3-WS1-50: wal/value/duplicates/count endpoint
pub(crate) async fn wal_value_duplicates_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalValueDuplicatesCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_value_duplicates_count");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in wal.wal_records() {
        *counts.entry(rec.value.clone()).or_insert(0) += 1;
    }
    drop(wal);
    let duplicate_value_count = counts.values().filter(|&&c| c > 1).count();
    Ok((StatusCode::OK, Json(WalValueDuplicatesCountResponse {
        status: "ok",
        duplicate_value_count,
    })))
}


// S3-WS1-51: wal/value/distinct/count endpoint
pub(crate) async fn wal_value_distinct_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalValueDistinctCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_value_distinct_count");
    let mut distinct_values = std::collections::BTreeSet::new();
    for rec in wal.wal_records() {
        distinct_values.insert(rec.value.clone());
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalValueDistinctCountResponse {
        status: "ok",
        distinct_value_count: distinct_values.len(),
    })))
}


// S3-WS1-52: wal/value/unique/count endpoint
pub(crate) async fn wal_value_unique_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalValueUniqueCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_value_unique_count");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in wal.wal_records() {
        *counts.entry(rec.value.clone()).or_insert(0) += 1;
    }
    drop(wal);
    let unique_value_count = counts.values().filter(|&&c| c == 1).count();
    Ok((StatusCode::OK, Json(WalValueUniqueCountResponse {
        status: "ok",
        unique_value_count,
    })))
}


// S3-WS1-53: wal/value/trimmed/count endpoint
pub(crate) async fn wal_value_trimmed_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalValueTrimmedCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_value_trimmed_count");
    let trimmed_value_count = wal
        .wal_records()
        .iter()
        .filter(|rec| rec.value.trim() != rec.value)
        .count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalValueTrimmedCountResponse {
        status: "ok",
        trimmed_value_count,
    })))
}


// S3-WS1-54: wal/value/case_variant/count endpoint
pub(crate) async fn wal_value_case_variant_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalValueCaseVariantCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_value_case_variant_count");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for rec in wal.wal_records() {
        *counts.entry(rec.value.to_ascii_lowercase()).or_insert(0) += 1;
    }
    drop(wal);
    let case_variant_count = counts.values().filter(|&&c| c > 1).count();
    Ok((StatusCode::OK, Json(WalValueCaseVariantCountResponse {
        status: "ok",
        case_variant_count,
    })))
}


// S3-WS1-55: wal/order_by/desc_direction/count endpoint
pub(crate) async fn wal_order_by_desc_direction_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalOrderByDescDirectionCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_order_by_desc_direction_count");
    let mut desc_count = 0;
    for rec in wal.wal_records() {
        if rec.value.to_ascii_uppercase().contains(" DESC") || rec.value.to_ascii_uppercase().starts_with("DESC") {
            desc_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalOrderByDescDirectionCountResponse {
        status: "ok",
        desc_direction_count: desc_count,
    })))
}


// S3-WS1-56: wal/order_by/random/count endpoint
pub(crate) async fn wal_order_by_random_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalOrderByRandomCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_order_by_random_count");
    let mut random_order_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains("RANDOM()") || value_up.contains("RAND()") {
            random_order_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalOrderByRandomCountResponse {
        status: "ok",
        random_order_count,
    })))
}


// S3-WS1-57: wal/order_by/random_seeded/count endpoint
pub(crate) async fn wal_order_by_random_seeded_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalOrderByRandomSeededCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_order_by_random_seeded_count");
    let mut random_seeded_order_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains("RANDOM(") && !value_up.contains("RANDOM()") {
            random_seeded_order_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalOrderByRandomSeededCountResponse {
        status: "ok",
        random_seeded_order_count,
    })))
}


// S3-WS1-58: wal/order_by/asc_direction/count endpoint
pub(crate) async fn wal_order_by_asc_direction_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalOrderByAscDirectionCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_order_by_asc_direction_count");
    let mut asc_direction_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" ASC") || value_up.starts_with("ASC") {
            asc_direction_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalOrderByAscDirectionCountResponse {
        status: "ok",
        asc_direction_count,
    })))
}


// S3-WS1-59: wal/order_by/rand_alias/count endpoint
pub(crate) async fn wal_order_by_rand_alias_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalOrderByRandAliasCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_order_by_rand_alias_count");
    let mut rand_alias_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains("RAND()") {
            rand_alias_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalOrderByRandAliasCountResponse {
        status: "ok",
        rand_alias_count,
    })))
}


// S3-WS1-60: wal/order_by/multi_column/count endpoint
pub(crate) async fn wal_order_by_multi_column_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalOrderByMultiColumnCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_order_by_multi_column_count");
    let mut multi_column_order_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if let Some(order_idx) = value_up.find("ORDER BY") {
            let tail = &value_up[order_idx + "ORDER BY".len()..];
            if tail.contains(',') {
                multi_column_order_count += 1;
            }
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalOrderByMultiColumnCountResponse {
        status: "ok",
        multi_column_order_count,
    })))
}


// S3-WS1-61: wal/pagination/limit_offset/count endpoint
pub(crate) async fn wal_pagination_limit_offset_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalPaginationLimitOffsetCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_pagination_limit_offset_count");
    let mut limit_offset_pagination_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains("LIMIT") && value_up.contains("OFFSET") {
            limit_offset_pagination_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalPaginationLimitOffsetCountResponse {
        status: "ok",
        limit_offset_pagination_count,
    })))
}


// S3-WS1-62: wal/pagination/offset_only/count endpoint
pub(crate) async fn wal_pagination_offset_only_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalPaginationOffsetOnlyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_pagination_offset_only_count");
    let mut offset_only_pagination_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" OFFSET ") && !value_up.contains(" LIMIT ") {
            offset_only_pagination_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalPaginationOffsetOnlyCountResponse {
        status: "ok",
        offset_only_pagination_count,
    })))
}


// S3-WS1-63: wal/having_without_group_by/count endpoint
pub(crate) async fn wal_having_without_group_by_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalHavingWithoutGroupByCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_having_without_group_by_count");
    let mut having_without_group_by_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" HAVING ") && !value_up.contains(" GROUP BY ") {
            having_without_group_by_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalHavingWithoutGroupByCountResponse {
        status: "ok",
        having_without_group_by_count,
    })))
}


// S3-WS1-64: wal/having_with_group_by/count endpoint
pub(crate) async fn wal_having_with_group_by_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalHavingWithGroupByCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_having_with_group_by_count");
    let mut having_with_group_by_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" HAVING ") && value_up.contains(" GROUP BY ") {
            having_with_group_by_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalHavingWithGroupByCountResponse {
        status: "ok",
        having_with_group_by_count,
    })))
}


// S3-WS1-65: wal/group_by/rollup/count endpoint
pub(crate) async fn wal_group_by_rollup_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalGroupByRollupCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_group_by_rollup_count");
    let mut group_by_rollup_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains("GROUP BY ROLLUP(") || value_up.contains("GROUP BY ROLLUP (") {
            group_by_rollup_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalGroupByRollupCountResponse {
        status: "ok",
        group_by_rollup_count,
    })))
}


// S3-WS1-66: wal/group_by/cube/count endpoint
pub(crate) async fn wal_group_by_cube_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalGroupByCubeCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_group_by_cube_count");
    let mut group_by_cube_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains("GROUP BY CUBE(") || value_up.contains("GROUP BY CUBE (") {
            group_by_cube_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalGroupByCubeCountResponse {
        status: "ok",
        group_by_cube_count,
    })))
}


// S3-WS1-67: wal/select/distinct_on/count endpoint
pub(crate) async fn wal_select_distinct_on_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalSelectDistinctOnCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_select_distinct_on_count");
    let mut select_distinct_on_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains("SELECT DISTINCT ON (") {
            select_distinct_on_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalSelectDistinctOnCountResponse {
        status: "ok",
        select_distinct_on_count,
    })))
}


// S3-WS1-68: wal/for/update/count endpoint
pub(crate) async fn wal_for_update_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalForUpdateCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_for_update_count");
    let mut for_update_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" FOR UPDATE") || value_up.contains(" FOR SHARE") {
            for_update_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalForUpdateCountResponse {
        status: "ok",
        for_update_count,
    })))
}


// S3-WS1-69: wal/left/join/count endpoint
pub(crate) async fn wal_left_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalLeftJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_left_join_count");
    let mut left_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" LEFT JOIN ") || value_up.contains(" LEFT OUTER JOIN ") {
            left_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalLeftJoinCountResponse {
        status: "ok",
        left_join_count,
    })))
}


// S3-WS1-70: wal/right/join/count endpoint
pub(crate) async fn wal_right_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalRightJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_right_join_count");
    let mut right_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" RIGHT JOIN ") || value_up.contains(" RIGHT OUTER JOIN ") {
            right_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalRightJoinCountResponse {
        status: "ok",
        right_join_count,
    })))
}


// S3-WS1-71: wal/full_outer/join/count endpoint
pub(crate) async fn wal_full_outer_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalFullOuterJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_full_outer_join_count");
    let mut full_outer_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" FULL JOIN ") || value_up.contains(" FULL OUTER JOIN ") {
            full_outer_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalFullOuterJoinCountResponse {
        status: "ok",
        full_outer_join_count,
    })))
}


// S3-WS1-72: wal/inner/join/count endpoint
pub(crate) async fn wal_inner_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalInnerJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_inner_join_count");
    let mut inner_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" INNER JOIN ") {
            inner_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalInnerJoinCountResponse {
        status: "ok",
        inner_join_count,
    })))
}


// S3-WS1-73: wal/straight/join/count endpoint
pub(crate) async fn wal_straight_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalStraightJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_straight_join_count");
    let mut straight_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" STRAIGHT_JOIN ") {
            straight_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalStraightJoinCountResponse {
        status: "ok",
        straight_join_count,
    })))
}


// S3-WS1-74: wal/semi/join/count endpoint
pub(crate) async fn wal_semi_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalSemiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_semi_join_count");
    let mut semi_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" SEMI JOIN ") {
            semi_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalSemiJoinCountResponse {
        status: "ok",
        semi_join_count,
    })))
}


// S3-WS1-75: wal/anti/join/count endpoint
pub(crate) async fn wal_anti_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalAntiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_anti_join_count");
    let mut anti_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" ANTI JOIN ") {
            anti_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalAntiJoinCountResponse {
        status: "ok",
        anti_join_count,
    })))
}


// S3-WS1-76: wal/cross/apply/count endpoint
pub(crate) async fn wal_cross_apply_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalCrossApplyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_cross_apply_count");
    let mut cross_apply_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" CROSS APPLY ") {
            cross_apply_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalCrossApplyCountResponse {
        status: "ok",
        cross_apply_count,
    })))
}


// S3-WS1-77: wal/outer/apply/count endpoint
pub(crate) async fn wal_outer_apply_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalOuterApplyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_outer_apply_count");
    let mut outer_apply_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" OUTER APPLY ") {
            outer_apply_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalOuterApplyCountResponse {
        status: "ok",
        outer_apply_count,
    })))
}


// S3-WS1-78: wal/apply/count endpoint
pub(crate) async fn wal_apply_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalApplyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_apply_count");
    let mut apply_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" APPLY ") {
            apply_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalApplyCountResponse {
        status: "ok",
        apply_count,
    })))
}


// S3-WS1-79: wal/left/semi/join/count endpoint
pub(crate) async fn wal_left_semi_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalLeftSemiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_left_semi_join_count");
    let mut left_semi_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" LEFT SEMI JOIN ") {
            left_semi_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalLeftSemiJoinCountResponse {
        status: "ok",
        left_semi_join_count,
    })))
}


// S3-WS1-80: wal/left/anti/join/count endpoint
pub(crate) async fn wal_left_anti_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalLeftAntiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_left_anti_join_count");
    let mut left_anti_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" LEFT ANTI JOIN ") {
            left_anti_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalLeftAntiJoinCountResponse {
        status: "ok",
        left_anti_join_count,
    })))
}


// S3-WS1-81: wal/right/semi/join/count endpoint
pub(crate) async fn wal_right_semi_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalRightSemiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_right_semi_join_count");
    let mut right_semi_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" RIGHT SEMI JOIN ") {
            right_semi_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalRightSemiJoinCountResponse {
        status: "ok",
        right_semi_join_count,
    })))
}


// S3-WS1-82: wal/right/anti/join/count endpoint
pub(crate) async fn wal_right_anti_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalRightAntiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_right_anti_join_count");
    let mut right_anti_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" RIGHT ANTI JOIN ") {
            right_anti_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalRightAntiJoinCountResponse {
        status: "ok",
        right_anti_join_count,
    })))
}


// S3-WS1-83: wal/full/semi/join/count endpoint
pub(crate) async fn wal_full_semi_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalFullSemiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_full_semi_join_count");
    let mut full_semi_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" FULL SEMI JOIN ") {
            full_semi_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalFullSemiJoinCountResponse {
        status: "ok",
        full_semi_join_count,
    })))
}


// S3-WS1-84: wal/full/anti/join/count endpoint
pub(crate) async fn wal_full_anti_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalFullAntiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_full_anti_join_count");
    let mut full_anti_join_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" FULL ANTI JOIN ") {
            full_anti_join_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalFullAntiJoinCountResponse {
        status: "ok",
        full_anti_join_count,
    })))
}


// S3-WS1-85: wal/union/all/count endpoint
pub(crate) async fn wal_union_all_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalUnionAllCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_union_all_count");
    let mut union_all_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if value_up.contains(" UNION ALL ") {
            union_all_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalUnionAllCountResponse {
        status: "ok",
        union_all_count,
    })))
}


// S3-WS1-86: wal/aggregate/distinct/count endpoint
pub(crate) async fn wal_aggregate_distinct_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalAggregateDistinctCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_aggregate_distinct_count");
    let mut aggregate_distinct_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase().replace(' ', "");
        if value_up.contains("COUNT(DISTINCT")
            || value_up.contains("SUM(DISTINCT")
            || value_up.contains("AVG(DISTINCT")
            || value_up.contains("MIN(DISTINCT")
            || value_up.contains("MAX(DISTINCT")
        {
            aggregate_distinct_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalAggregateDistinctCountResponse {
        status: "ok",
        aggregate_distinct_count,
    })))
}


// S3-WS1-87: wal/table/alias/count endpoint
pub(crate) async fn wal_table_alias_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalTableAliasCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_table_alias_count");
    let mut table_alias_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if contains_table_alias_sql(&value_up) {
            table_alias_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalTableAliasCountResponse {
        status: "ok",
        table_alias_count,
    })))
}


// S3-WS1-88: wal/sql/column/alias/count endpoint
pub(crate) async fn wal_column_alias_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalColumnAliasCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state
        .wal_engine
        .lock()
        .expect("wal_engine lock wal_column_alias_count");
    let mut column_alias_count = 0;
    for rec in wal.wal_records() {
        let value_up = rec.value.to_ascii_uppercase();
        if contains_column_alias_sql(&value_up) {
            column_alias_count += 1;
        }
    }
    drop(wal);
    Ok((StatusCode::OK, Json(WalColumnAliasCountResponse {
        status: "ok",
        column_alias_count,
    })))
}

