use std::collections::BTreeMap;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use crate::{AppState, AuthErrorResponse, now_unix_ms};
use crate::RaftLogEntry;
use crate::{contains_table_alias_sql, contains_column_alias_sql};
use crate::auth::{require_operator_auth, require_operator_privilege};
use crate::audit_helpers::append_audit_event;

// ─── Rows DTOs ──────────────────────────────────────────────────────────


// ─── S11-WS1-12: Row store page stats structs ────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowsPageStatsResponse {
    pub(crate) status: &'static str,
    pub(crate) page_count: usize,
    pub(crate) total_rows: usize,
    pub(crate) visible_rows: usize,
    pub(crate) current_xid: u64,
}


// ─── S11-WS1-14: Rows modified structs ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct RowsModifiedQuery {
    pub(crate) since_xid: u64,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsModifiedResponse {
    pub(crate) status: &'static str,
    pub(crate) modified_count: usize,
    pub(crate) since_xid: u64,
    pub(crate) keys: Vec<String>,
}


// ─── S11-WS1-15: Rows XID structs ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowsXidResponse {
    pub(crate) status: &'static str,
    pub(crate) current_xid: u64,
    pub(crate) next_xid: u64,
}


// ─── S11-WS1-16: Rows visible structs ────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowsVisibleResponse {
    pub(crate) status: &'static str,
    pub(crate) snapshot_xid: u64,
    pub(crate) visible_row_count: usize,
}


// ─── S11-WS1-17: Rows total structs ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowsTotalResponse {
    pub(crate) status: &'static str,
    pub(crate) total_row_count: usize,
}


// ─── S11-WS1-18: Rows keys count structs ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowsKeysCountResponse {
    pub(crate) status: &'static str,
    pub(crate) key_count: usize,
}


// ─── S11-WS1-19: Rows scan visible structs ───────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct RowsScanVisibleQuery {
    pub(crate) limit: Option<usize>,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowScanEntry {
    pub(crate) key: String,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsScanVisibleResponse {
    pub(crate) status: &'static str,
    pub(crate) snapshot_xid: u64,
    pub(crate) row_count: usize,
    pub(crate) rows: Vec<RowScanEntry>,
}


// ─── S11-WS1-20: Rows tombstone count structs ────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowsTombstoneCountResponse {
    pub(crate) status: &'static str,
    pub(crate) tombstone_count: usize,
}


// ─── S11-WS1-21: Rows XID history structs ────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowsXidHistoryResponse {
    pub(crate) status: &'static str,
    pub(crate) current_xid: u64,
    pub(crate) next_xid: u64,
    pub(crate) total_transactions: u64,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsFirstKeyResponse {
    pub(crate) status: &'static str,
    pub(crate) has_key: bool,
    pub(crate) first_key: String,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsLastKeyResponse {
    pub(crate) status: &'static str,
    pub(crate) has_key: bool,
    pub(crate) last_key: String,
}


// ─── S11-WS1-24: Rows count distinct + rows key exists structs ───────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowsCountDistinctResponse {
    pub(crate) status: &'static str,
    pub(crate) distinct_value_count: usize,
}


#[derive(Debug, Deserialize)]
pub(crate) struct RowsKeyExistsQuery {
    pub(crate) key: String,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyExistsResponse {
    pub(crate) status: &'static str,
    pub(crate) key: String,
    pub(crate) exists: bool,
}


// ─── S11-WS1-25: Rows value search + WAL record count structs ──────────────────────────────────
#[derive(Debug, Deserialize)]
pub(crate) struct RowsValueSearchQuery {
    pub(crate) value: String,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueSearchResponse {
    pub(crate) status: &'static str,
    pub(crate) match_count: usize,
    pub(crate) matches: Vec<String>,
}


// ─── S11-WS1-26: Rows count range + WAL checkpoint age structs ───────────────────────────────
#[derive(Debug, Deserialize)]
pub(crate) struct RowsCountRangeQuery {
    pub(crate) prefix: Option<String>,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsCountRangeResponse {
    pub(crate) status: &'static str,
    pub(crate) row_count: usize,
    pub(crate) prefix: Option<String>,
}


// ─── S11-WS1-27: Rows payload size + WAL flush count structs ───────────────────────────────
#[derive(Debug, Serialize)]
pub(crate) struct RowsPayloadSizeResponse {
    pub(crate) status: &'static str,
    pub(crate) total_fields: usize,
    pub(crate) row_count: usize,
}


// S3-WS1-28: rows/field/count + wal/entry/latest structs

#[derive(Debug, Serialize)]
pub(crate) struct RowsFieldCountResponse {
    pub(crate) status: &'static str,
    pub(crate) total_fields: usize,
    pub(crate) row_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyLongestResponse {
    pub(crate) status: &'static str,
    pub(crate) longest_key: String,
    pub(crate) key_length: usize,
    pub(crate) row_count: usize,
}


// S3-WS1-30: rows/key/shortest struct

#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyShortestResponse {
    pub(crate) status: &'static str,
    pub(crate) shortest_key: String,
    pub(crate) key_length: usize,
    pub(crate) row_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsCountAllResponse {
    pub(crate) status: &'static str,
    pub(crate) total_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsSnapshotSizeResponse {
    pub(crate) status: &'static str,
    pub(crate) snapshot_row_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsVersionLatestResponse {
    pub(crate) status: &'static str,
    pub(crate) latest_version: u64,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsDistinctCountResponse {
    pub(crate) status: &'static str,
    pub(crate) distinct_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyMedianResponse {
    pub(crate) status: &'static str,
    pub(crate) has_key: bool,
    pub(crate) median_key: String,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsChecksumResponse {
    pub(crate) status: &'static str,
    pub(crate) checksum: u64,
    pub(crate) row_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsFieldTypesResponse {
    pub(crate) status: &'static str,
    pub(crate) field_count: usize,
    pub(crate) unique_type_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyEmptyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) empty_key_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyMinResponse {
    pub(crate) status: &'static str,
    pub(crate) has_key: bool,
    pub(crate) min_key: String,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsFieldCardinalityResponse {
    pub(crate) status: &'static str,
    pub(crate) distinct_field_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyMaxResponse {
    pub(crate) status: &'static str,
    pub(crate) has_key: bool,
    pub(crate) max_key: String,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueNonNullCountResponse {
    pub(crate) status: &'static str,
    pub(crate) non_null_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueEmptyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) empty_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueNonEmptyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) non_empty_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyNonEmptyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) non_empty_key_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueNonBlankCountResponse {
    pub(crate) status: &'static str,
    pub(crate) non_blank_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyNonBlankCountResponse {
    pub(crate) status: &'static str,
    pub(crate) non_blank_key_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueBlankCountResponse {
    pub(crate) status: &'static str,
    pub(crate) blank_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsKeyDuplicatesCountResponse {
    pub(crate) status: &'static str,
    pub(crate) duplicate_key_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueDuplicatesCountResponse {
    pub(crate) status: &'static str,
    pub(crate) duplicate_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueDistinctCountResponse {
    pub(crate) status: &'static str,
    pub(crate) distinct_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueUniqueCountResponse {
    pub(crate) status: &'static str,
    pub(crate) unique_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueTrimmedCountResponse {
    pub(crate) status: &'static str,
    pub(crate) trimmed_value_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsValueCaseVariantCountResponse {
    pub(crate) status: &'static str,
    pub(crate) case_variant_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsOrderByDescDirectionCountResponse {
    pub(crate) status: &'static str,
    pub(crate) desc_direction_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsOrderByRandomCountResponse {
    pub(crate) status: &'static str,
    pub(crate) random_order_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsOrderByRandomSeededCountResponse {
    pub(crate) status: &'static str,
    pub(crate) random_seeded_order_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsOrderByAscDirectionCountResponse {
    pub(crate) status: &'static str,
    pub(crate) asc_direction_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsOrderByRandAliasCountResponse {
    pub(crate) status: &'static str,
    pub(crate) rand_alias_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsOrderByMultiColumnCountResponse {
    pub(crate) status: &'static str,
    pub(crate) multi_column_order_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsPaginationLimitOffsetCountResponse {
    pub(crate) status: &'static str,
    pub(crate) limit_offset_pagination_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsPaginationOffsetOnlyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) offset_only_pagination_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsHavingWithoutGroupByCountResponse {
    pub(crate) status: &'static str,
    pub(crate) having_without_group_by_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsHavingWithGroupByCountResponse {
    pub(crate) status: &'static str,
    pub(crate) having_with_group_by_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsGroupByRollupCountResponse {
    pub(crate) status: &'static str,
    pub(crate) group_by_rollup_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsGroupByCubeCountResponse {
    pub(crate) status: &'static str,
    pub(crate) group_by_cube_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsSelectDistinctOnCountResponse {
    pub(crate) status: &'static str,
    pub(crate) select_distinct_on_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsForUpdateCountResponse {
    pub(crate) status: &'static str,
    pub(crate) for_update_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsLeftJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) left_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsRightJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) right_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsFullOuterJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) full_outer_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsInnerJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) inner_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsStraightJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) straight_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsSemiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) semi_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsAntiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) anti_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsCrossApplyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) cross_apply_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsOuterApplyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) outer_apply_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsApplyCountResponse {
    pub(crate) status: &'static str,
    pub(crate) apply_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsLeftSemiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) left_semi_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsLeftAntiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) left_anti_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsRightSemiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) right_semi_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsRightAntiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) right_anti_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsFullSemiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) full_semi_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsFullAntiJoinCountResponse {
    pub(crate) status: &'static str,
    pub(crate) full_anti_join_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsUnionAllCountResponse {
    pub(crate) status: &'static str,
    pub(crate) union_all_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsAggregateDistinctCountResponse {
    pub(crate) status: &'static str,
    pub(crate) aggregate_distinct_count: usize,
}


#[derive(Debug, Serialize)]
pub(crate) struct RowsTableAliasCountResponse {
    pub(crate) status: &'static str,
    pub(crate) table_alias_count: usize,
}



#[derive(Debug, Serialize)]
pub(crate) struct RowsColumnAliasCountResponse {
    pub(crate) status: &'static str,
    pub(crate) column_alias_count: usize,
}


// ─── Rows handlers ───────────────────────────────────────────────────────


// ─── S11-WS1-12: Row store page stats endpoint ───────────────────────────────

/// S11-WS1-12: Return page-level statistics from the MVCC row store.
pub(crate) async fn rows_page_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsPageStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_page_stats");
    let page_count = rs.page_count();
    let total_rows = rs.total_row_count();
    let current_xid = rs.current_xid();
    let visible_rows = rs.visible_row_count(current_xid);
    drop(rs);
    Ok((StatusCode::OK, Json(RowsPageStatsResponse {
        status: "ok",
        page_count,
        total_rows,
        visible_rows,
        current_xid,
    })))
}


// ─── S11-WS1-14: Rows modified endpoint ──────────────────────────────────────

/// S11-WS1-14: Return keys of rows modified after the given transaction ID.
pub(crate) async fn rows_modified(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RowsModifiedQuery>,
) -> Result<(StatusCode, Json<RowsModifiedResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_modified");
    let snapshot = rs.export_rows_snapshot();
    let keys: Vec<String> = snapshot
        .iter()
        .filter(|(k, _)| rs.was_modified_after(k, params.since_xid))
        .map(|(k, _)| k.clone())
        .collect();
    let modified_count = keys.len();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsModifiedResponse {
        status: "ok",
        modified_count,
        since_xid: params.since_xid,
        keys,
    })))
}


// ─── S11-WS1-15: Rows XID endpoint ───────────────────────────────────────────

/// S11-WS1-15: Return the current transaction ID and next expected XID.
pub(crate) async fn rows_xid(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsXidResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_xid");
    let current_xid = rs.current_xid();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsXidResponse {
        status: "ok",
        current_xid,
        next_xid: current_xid + 1,
    })))
}


// ─── S11-WS1-16: Rows visible endpoint ───────────────────────────────────────

/// S11-WS1-16: Return the count of visible rows at the current MVCC snapshot.
pub(crate) async fn rows_visible(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsVisibleResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_visible");
    let snapshot_xid = rs.current_xid();
    let visible_row_count = rs.visible_row_count(snapshot_xid);
    drop(rs);
    Ok((StatusCode::OK, Json(RowsVisibleResponse {
        status: "ok",
        snapshot_xid,
        visible_row_count,
    })))
}


// ─── S11-WS1-17: Rows total endpoint ─────────────────────────────────────────

/// S11-WS1-17: Return total row count across all MVCC versions (including tombstones).
pub(crate) async fn rows_total(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsTotalResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_total");
    let total_row_count = rs.total_row_count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsTotalResponse {
        status: "ok",
        total_row_count,
    })))
}


// ─── S11-WS1-18: Rows keys count endpoint ────────────────────────────────────

/// S11-WS1-18: Return the count of distinct row keys in the MVCC store.
pub(crate) async fn rows_keys_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeysCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_keys_count");
    let snapshot = rs.export_rows_snapshot();
    let key_count = snapshot.len();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsKeysCountResponse {
        status: "ok",
        key_count,
    })))
}


// ─── S11-WS1-19: Rows scan visible endpoint ──────────────────────────────────

/// S11-WS1-19: Return all rows visible at the current MVCC snapshot, with optional limit.
pub(crate) async fn rows_scan_visible(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RowsScanVisibleQuery>,
) -> Result<(StatusCode, Json<RowsScanVisibleResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_scan_visible");
    let snapshot_xid = rs.current_xid();
    let all_rows = rs.scan_at_snapshot(snapshot_xid);
    let rows: Vec<RowScanEntry> = all_rows
        .iter()
        .take(params.limit.unwrap_or(usize::MAX))
        .map(|(k, _)| RowScanEntry { key: k.to_string() })
        .collect();
    let row_count = rows.len();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsScanVisibleResponse {
        status: "ok",
        snapshot_xid,
        row_count,
        rows,
    })))
}


// ─── S11-WS1-20: Rows tombstone count endpoint ───────────────────────────────

/// S11-WS1-20: Return the count of tombstone (deleted-marker) rows in the MVCC store.
pub(crate) async fn rows_tombstone_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsTombstoneCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock rows_tombstone_count");
    let tombstone_count = wal.wal_records().iter().filter(|r| r.value == "__deleted__").count();
    drop(wal);
    Ok((StatusCode::OK, Json(RowsTombstoneCountResponse {
        status: "ok",
        tombstone_count,
    })))
}


// ─── S11-WS1-21: Rows XID history endpoint ───────────────────────────────────

/// S11-WS1-21: Return current XID, next XID, and total transaction count from the row store.
pub(crate) async fn rows_xid_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsXidHistoryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_xid_history");
    let current_xid = rs.current_xid();
    let next_xid = current_xid + 1;
    let total_transactions = current_xid;
    drop(rs);
    Ok((StatusCode::OK, Json(RowsXidHistoryResponse {
        status: "ok",
        current_xid,
        next_xid,
        total_transactions,
    })))
}


// ─── S11-WS1-22: Rows first key endpoint ─────────────────────────────────────────────────────

/// S11-WS1-22: Return the first (alphabetically smallest) key currently in the row store.
pub(crate) async fn rows_first_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFirstKeyResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_first_key");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let mut keys: Vec<String> = snapshot.into_iter().map(|(k, _)| k).collect();
    keys.sort();
    let first_key = keys.into_iter().next().unwrap_or_default();
    let has_key = !first_key.is_empty();
    Ok((StatusCode::OK, Json(RowsFirstKeyResponse {
        status: "ok",
        has_key,
        first_key,
    })))
}


// ─── S11-WS1-23: Rows last key endpoint ─────────────────────────────────────────────────────

/// S11-WS1-23: Return the last (alphabetically largest) key currently in the row store.
pub(crate) async fn rows_last_key(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsLastKeyResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_last_key");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let mut keys: Vec<String> = snapshot.into_iter().map(|(k, _)| k).collect();
    keys.sort();
    let last_key = keys.into_iter().last().unwrap_or_default();
    let has_key = !last_key.is_empty();
    Ok((StatusCode::OK, Json(RowsLastKeyResponse {
        status: "ok",
        has_key,
        last_key,
    })))
}


// ─── S11-WS1-24: Rows count distinct endpoint ──────────────────────────────────────────────

/// S11-WS1-24: Return the count of distinct row values in the MVCC row store.
pub(crate) async fn rows_count_distinct(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsCountDistinctResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_count_distinct");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let mut distinct_values: Vec<String> = snapshot.into_iter().flat_map(|(_, payload)| payload.into_values()).collect();
    distinct_values.sort();
    distinct_values.dedup();
    let distinct_value_count = distinct_values.len();
    Ok((StatusCode::OK, Json(RowsCountDistinctResponse {
        status: "ok",
        distinct_value_count,
    })))
}


// ─── S11-WS1-24: Rows key exists endpoint ──────────────────────────────────────────────────────

/// S11-WS1-24: Return whether a given key exists in the MVCC row store.
pub(crate) async fn rows_key_exists(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RowsKeyExistsQuery>,
) -> Result<(StatusCode, Json<RowsKeyExistsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_exists");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let exists = snapshot.iter().any(|(k, _)| k == &params.key);
    Ok((StatusCode::OK, Json(RowsKeyExistsResponse {
        status: "ok",
        key: params.key,
        exists,
    })))
}



/// S11-WS1-25: Search rows whose payload contains the given value.
pub(crate) async fn rows_value_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RowsValueSearchQuery>,
) -> Result<(StatusCode, Json<RowsValueSearchResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_search");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let needle = params.value.to_lowercase();
    let matches: Vec<String> = snapshot
        .into_iter()
        .filter(|(_, payload)| payload.values().any(|v| v.to_lowercase().contains(needle.as_str())))
        .map(|(k, _)| k)
        .collect();
    let match_count = matches.len();
    Ok((StatusCode::OK, Json(RowsValueSearchResponse {
        status: "ok",
        match_count,
        matches,
    })))
}


/// S11-WS1-26: Count rows optionally filtered by a key prefix.
pub(crate) async fn rows_count_range(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RowsCountRangeQuery>,
) -> Result<(StatusCode, Json<RowsCountRangeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_count_range");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let row_count = match &params.prefix {
        Some(p) => snapshot.iter().filter(|(k, _)| k.starts_with(p.as_str())).count(),
        None => snapshot.len(),
    };
    Ok((StatusCode::OK, Json(RowsCountRangeResponse {
        status: "ok",
        row_count,
        prefix: params.prefix,
    })))
}


/// S11-WS1-27: Return total payload field count and row count across all MVCC rows.
pub(crate) async fn rows_payload_size(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsPayloadSizeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_payload_size");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let row_count = snapshot.len();
    let total_fields: usize = snapshot.iter().map(|(_, p)| p.len()).sum();
    Ok((StatusCode::OK, Json(RowsPayloadSizeResponse {
        status: "ok",
        total_fields,
        row_count,
    })))
}


// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

// S3-WS1-28: rows/field/count endpoint
pub(crate) async fn rows_field_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFieldCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_field_count");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let row_count = snapshot.len();
    let total_fields: usize = snapshot.iter().map(|(_, p)| p.len()).sum();
    Ok((StatusCode::OK, Json(RowsFieldCountResponse { status: "ok", total_fields, row_count })))
}


// S3-WS1-29: rows/key/longest endpoint
pub(crate) async fn rows_key_longest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyLongestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_longest");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let row_count = snapshot.len();
    let longest_key = snapshot.iter()
        .map(|(k, _)| k.as_str())
        .max_by_key(|k| k.len())
        .unwrap_or("")
        .to_string();
    let key_length = longest_key.len();
    Ok((StatusCode::OK, Json(RowsKeyLongestResponse { status: "ok", longest_key, key_length, row_count })))
}


// S3-WS1-30: rows/key/shortest endpoint
pub(crate) async fn rows_key_shortest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyShortestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_shortest");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let row_count = snapshot.len();
    let shortest_key = snapshot.iter()
        .map(|(k, _)| k.as_str())
        .min_by_key(|k| k.len())
        .unwrap_or("")
        .to_string();
    let key_length = shortest_key.len();
    Ok((StatusCode::OK, Json(RowsKeyShortestResponse { status: "ok", shortest_key, key_length, row_count })))
}


// S3-WS1-31: rows/count/all endpoint
pub(crate) async fn rows_count_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsCountAllResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_count_all");
    let total_count = rs.total_row_count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsCountAllResponse { status: "ok", total_count })))
}


// S3-WS1-32: rows/snapshot/size endpoint
pub(crate) async fn rows_snapshot_size(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsSnapshotSizeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_snapshot_size");
    let snapshot_row_count = rs.total_row_count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsSnapshotSizeResponse { status: "ok", snapshot_row_count })))
}


// S3-WS1-33: rows/version/latest endpoint
pub(crate) async fn rows_version_latest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsVersionLatestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock rows_version_latest");
    let latest_version = wal.wal_records().last().map(|r| r.sequence).unwrap_or(0);
    drop(wal);
    Ok((StatusCode::OK, Json(RowsVersionLatestResponse { status: "ok", latest_version })))
}


// S3-WS1-34: rows/distinct/count endpoint
pub(crate) async fn rows_distinct_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsDistinctCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_distinct_count");
    let distinct_count = rs.total_row_count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsDistinctCountResponse { status: "ok", distinct_count })))
}


// S3-WS1-35: rows/key/median endpoint
pub(crate) async fn rows_key_median(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyMedianResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_median");
    let mut keys: Vec<String> = rs.export_rows_snapshot().into_iter().map(|(k, _)| k).collect();
    drop(rs);
    keys.sort();
    let has_key = !keys.is_empty();
    let median_key = if has_key {
        keys[keys.len() / 2].clone()
    } else {
        String::new()
    };
    Ok((StatusCode::OK, Json(RowsKeyMedianResponse { status: "ok", has_key, median_key })))
}


// S3-WS1-36: rows/checksum endpoint
pub(crate) async fn rows_checksum(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsChecksumResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_checksum");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let mut keys: Vec<String> = snapshot.iter().map(|(k, _)| k.clone()).collect();
    keys.sort();
    let checksum = keys.iter().fold(0_u64, |acc, k| {
        k.as_bytes().iter().fold(acc, |a, b| a.wrapping_add(*b as u64))
    });
    let row_count = snapshot.len();
    Ok((StatusCode::OK, Json(RowsChecksumResponse { status: "ok", checksum, row_count })))
}


// S3-WS1-37: rows/field/types endpoint
pub(crate) async fn rows_field_types(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFieldTypesResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_field_types");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let field_count: usize = snapshot.iter().map(|(_, fields)| fields.len()).sum();
    let unique_type_count = if field_count > 0 { 1 } else { 0 };
    Ok((StatusCode::OK, Json(RowsFieldTypesResponse { status: "ok", field_count, unique_type_count })))
}


// S3-WS1-38: rows/key/empty/count endpoint
pub(crate) async fn rows_key_empty_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyEmptyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_empty_count");
    let snapshot = rs.export_rows_snapshot();
    drop(rs);
    let empty_key_count = snapshot.iter().filter(|(k, _)| k.is_empty()).count();
    Ok((StatusCode::OK, Json(RowsKeyEmptyCountResponse { status: "ok", empty_key_count })))
}


// S3-WS1-39: rows/key/min endpoint
pub(crate) async fn rows_key_min(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyMinResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_min");
    let min_key = rs
        .export_rows_snapshot()
        .into_iter()
        .map(|(k, _)| k)
        .min()
        .unwrap_or_default();
    drop(rs);
    let has_key = !min_key.is_empty();
    Ok((StatusCode::OK, Json(RowsKeyMinResponse { status: "ok", has_key, min_key })))
}


// S3-WS1-40: rows/field/cardinality endpoint
pub(crate) async fn rows_field_cardinality(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFieldCardinalityResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_field_cardinality");
    let mut fields = std::collections::BTreeSet::new();
    for (_, row) in rs.export_rows_snapshot() {
        for key in row.keys() {
            fields.insert(key.to_string());
        }
    }
    let distinct_field_count = fields.len();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsFieldCardinalityResponse {
        status: "ok",
        distinct_field_count,
    })))
}


// S3-WS1-41: rows/key/max endpoint
pub(crate) async fn rows_key_max(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyMaxResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_max");
    let max_key = rs
        .export_rows_snapshot()
        .into_iter()
        .map(|(k, _)| k)
        .max()
        .unwrap_or_default();
    drop(rs);
    let has_key = !max_key.is_empty();
    Ok((StatusCode::OK, Json(RowsKeyMaxResponse { status: "ok", has_key, max_key })))
}


// S3-WS1-42: rows/value/non_null/count endpoint
pub(crate) async fn rows_value_non_null_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueNonNullCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_non_null_count");
    let non_null_value_count = rs
        .export_rows_snapshot()
        .into_iter()
        .flat_map(|(_, row)| row.into_values())
        .filter(|v| !v.is_empty())
        .count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsValueNonNullCountResponse {
        status: "ok",
        non_null_value_count,
    })))
}


// S3-WS1-43: rows/value/empty/count endpoint
pub(crate) async fn rows_value_empty_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueEmptyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_empty_count");
    let empty_value_count = rs
        .export_rows_snapshot()
        .into_iter()
        .flat_map(|(_, row)| row.into_values())
        .filter(|v| v.is_empty())
        .count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsValueEmptyCountResponse {
        status: "ok",
        empty_value_count,
    })))
}


// S3-WS1-44: rows/value/non_empty/count endpoint
pub(crate) async fn rows_value_non_empty_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueNonEmptyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_non_empty_count");
    let non_empty_value_count = rs
        .export_rows_snapshot()
        .into_iter()
        .flat_map(|(_, row)| row.into_values())
        .filter(|v| !v.is_empty())
        .count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsValueNonEmptyCountResponse {
        status: "ok",
        non_empty_value_count,
    })))
}


// S3-WS1-45: rows/key/non_empty/count endpoint
pub(crate) async fn rows_key_non_empty_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyNonEmptyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_non_empty_count");
    let non_empty_key_count = rs
        .export_rows_snapshot()
        .into_iter()
        .filter(|(key, _)| !key.is_empty())
        .count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsKeyNonEmptyCountResponse {
        status: "ok",
        non_empty_key_count,
    })))
}


// S3-WS1-46: rows/value/non_blank/count endpoint
pub(crate) async fn rows_value_non_blank_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueNonBlankCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_non_blank_count");
    let non_blank_value_count = rs
        .export_rows_snapshot()
        .into_iter()
        .flat_map(|(_, row)| row.into_values())
        .filter(|v| !v.trim().is_empty())
        .count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsValueNonBlankCountResponse {
        status: "ok",
        non_blank_value_count,
    })))
}


// S3-WS1-47: rows/key/non_blank/count endpoint
pub(crate) async fn rows_key_non_blank_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyNonBlankCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_non_blank_count");
    let non_blank_key_count = rs
        .export_rows_snapshot()
        .into_iter()
        .filter(|(k, _)| !k.trim().is_empty())
        .count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsKeyNonBlankCountResponse {
        status: "ok",
        non_blank_key_count,
    })))
}


// S3-WS1-48: rows/value/blank/count endpoint
pub(crate) async fn rows_value_blank_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueBlankCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_blank_count");
    let blank_value_count = rs
        .export_rows_snapshot()
        .into_iter()
        .flat_map(|(_, row)| row.into_values())
        .filter(|v| v.trim().is_empty())
        .count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsValueBlankCountResponse {
        status: "ok",
        blank_value_count,
    })))
}


// S3-WS1-49: rows/key/duplicates/count endpoint
pub(crate) async fn rows_key_duplicates_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsKeyDuplicatesCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_key_duplicates_count");
    let snapshot = rs.export_rows_snapshot();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (key, _) in snapshot {
        *counts.entry(key).or_insert(0) += 1;
    }
    let duplicate_key_count = counts.values().filter(|&&c| c > 1).count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsKeyDuplicatesCountResponse {
        status: "ok",
        duplicate_key_count,
    })))
}


// S3-WS1-50: rows/value/duplicates/count endpoint
pub(crate) async fn rows_value_duplicates_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueDuplicatesCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_duplicates_count");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            *counts.entry(value).or_insert(0) += 1;
        }
    }
    drop(rs);
    let duplicate_value_count = counts.values().filter(|&&c| c > 1).count();
    Ok((StatusCode::OK, Json(RowsValueDuplicatesCountResponse {
        status: "ok",
        duplicate_value_count,
    })))
}


// S3-WS1-51: rows/value/distinct/count endpoint
pub(crate) async fn rows_value_distinct_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueDistinctCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_distinct_count");
    let mut distinct_values = std::collections::BTreeSet::new();
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            distinct_values.insert(value);
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsValueDistinctCountResponse {
        status: "ok",
        distinct_value_count: distinct_values.len(),
    })))
}


// S3-WS1-52: rows/value/unique/count endpoint
pub(crate) async fn rows_value_unique_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueUniqueCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_unique_count");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            *counts.entry(value).or_insert(0) += 1;
        }
    }
    drop(rs);
    let unique_value_count = counts.values().filter(|&&c| c == 1).count();
    Ok((StatusCode::OK, Json(RowsValueUniqueCountResponse {
        status: "ok",
        unique_value_count,
    })))
}


// S3-WS1-53: rows/value/trimmed/count endpoint
pub(crate) async fn rows_value_trimmed_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueTrimmedCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_trimmed_count");
    let trimmed_value_count = rs
        .export_rows_snapshot()
        .into_iter()
        .flat_map(|(_, row)| row.into_values())
        .filter(|value| value.trim() != value)
        .count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowsValueTrimmedCountResponse {
        status: "ok",
        trimmed_value_count,
    })))
}


// S3-WS1-54: rows/value/case_variant/count endpoint
pub(crate) async fn rows_value_case_variant_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsValueCaseVariantCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_value_case_variant_count");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            *counts.entry(value.to_ascii_lowercase()).or_insert(0) += 1;
        }
    }
    drop(rs);
    let case_variant_count = counts.values().filter(|&&c| c > 1).count();
    Ok((StatusCode::OK, Json(RowsValueCaseVariantCountResponse {
        status: "ok",
        case_variant_count,
    })))
}


// S3-WS1-55: rows/order_by/desc_direction/count endpoint
pub(crate) async fn rows_order_by_desc_direction_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsOrderByDescDirectionCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_order_by_desc_direction_count");
    let mut desc_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            if value.to_ascii_uppercase().contains(" DESC") || value.to_ascii_uppercase().starts_with("DESC") {
                desc_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsOrderByDescDirectionCountResponse {
        status: "ok",
        desc_direction_count: desc_count,
    })))
}


// S3-WS1-56: rows/order_by/random/count endpoint
pub(crate) async fn rows_order_by_random_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsOrderByRandomCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_order_by_random_count");
    let mut random_order_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains("RANDOM()") || value_up.contains("RAND()") {
                random_order_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsOrderByRandomCountResponse {
        status: "ok",
        random_order_count,
    })))
}


// S3-WS1-57: rows/order_by/random_seeded/count endpoint
pub(crate) async fn rows_order_by_random_seeded_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsOrderByRandomSeededCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_order_by_random_seeded_count");
    let mut random_seeded_order_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains("RANDOM(") && !value_up.contains("RANDOM()") {
                random_seeded_order_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsOrderByRandomSeededCountResponse {
        status: "ok",
        random_seeded_order_count,
    })))
}


// S3-WS1-58: rows/order_by/asc_direction/count endpoint
pub(crate) async fn rows_order_by_asc_direction_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsOrderByAscDirectionCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_order_by_asc_direction_count");
    let mut asc_direction_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" ASC") || value_up.starts_with("ASC") {
                asc_direction_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsOrderByAscDirectionCountResponse {
        status: "ok",
        asc_direction_count,
    })))
}


// S3-WS1-59: rows/order_by/rand_alias/count endpoint
pub(crate) async fn rows_order_by_rand_alias_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsOrderByRandAliasCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_order_by_rand_alias_count");
    let mut rand_alias_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains("RAND()") {
                rand_alias_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsOrderByRandAliasCountResponse {
        status: "ok",
        rand_alias_count,
    })))
}


// S3-WS1-60: rows/order_by/multi_column/count endpoint
pub(crate) async fn rows_order_by_multi_column_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsOrderByMultiColumnCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_order_by_multi_column_count");
    let mut multi_column_order_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if let Some(order_idx) = value_up.find("ORDER BY") {
                let tail = &value_up[order_idx + "ORDER BY".len()..];
                if tail.contains(',') {
                    multi_column_order_count += 1;
                }
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsOrderByMultiColumnCountResponse {
        status: "ok",
        multi_column_order_count,
    })))
}


// S3-WS1-61: rows/pagination/limit_offset/count endpoint
pub(crate) async fn rows_pagination_limit_offset_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsPaginationLimitOffsetCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_pagination_limit_offset_count");
    let mut limit_offset_pagination_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains("LIMIT") && value_up.contains("OFFSET") {
                limit_offset_pagination_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsPaginationLimitOffsetCountResponse {
        status: "ok",
        limit_offset_pagination_count,
    })))
}


// S3-WS1-62: rows/pagination/offset_only/count endpoint
pub(crate) async fn rows_pagination_offset_only_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsPaginationOffsetOnlyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_pagination_offset_only_count");
    let mut offset_only_pagination_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" OFFSET ") && !value_up.contains(" LIMIT ") {
                offset_only_pagination_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsPaginationOffsetOnlyCountResponse {
        status: "ok",
        offset_only_pagination_count,
    })))
}


// S3-WS1-63: rows/having_without_group_by/count endpoint
pub(crate) async fn rows_having_without_group_by_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsHavingWithoutGroupByCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_having_without_group_by_count");
    let mut having_without_group_by_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" HAVING ") && !value_up.contains(" GROUP BY ") {
                having_without_group_by_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsHavingWithoutGroupByCountResponse {
        status: "ok",
        having_without_group_by_count,
    })))
}


// S3-WS1-64: rows/having_with_group_by/count endpoint
pub(crate) async fn rows_having_with_group_by_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsHavingWithGroupByCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_having_with_group_by_count");
    let mut having_with_group_by_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" HAVING ") && value_up.contains(" GROUP BY ") {
                having_with_group_by_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsHavingWithGroupByCountResponse {
        status: "ok",
        having_with_group_by_count,
    })))
}


// S3-WS1-65: rows/group_by/rollup/count endpoint
pub(crate) async fn rows_group_by_rollup_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsGroupByRollupCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_group_by_rollup_count");
    let mut group_by_rollup_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains("GROUP BY ROLLUP(") || value_up.contains("GROUP BY ROLLUP (") {
                group_by_rollup_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsGroupByRollupCountResponse {
        status: "ok",
        group_by_rollup_count,
    })))
}


// S3-WS1-66: rows/group_by/cube/count endpoint
pub(crate) async fn rows_group_by_cube_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsGroupByCubeCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_group_by_cube_count");
    let mut group_by_cube_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains("GROUP BY CUBE(") || value_up.contains("GROUP BY CUBE (") {
                group_by_cube_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsGroupByCubeCountResponse {
        status: "ok",
        group_by_cube_count,
    })))
}


// S3-WS1-67: rows/select/distinct_on/count endpoint
pub(crate) async fn rows_select_distinct_on_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsSelectDistinctOnCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_select_distinct_on_count");
    let mut select_distinct_on_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains("SELECT DISTINCT ON (") {
                select_distinct_on_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsSelectDistinctOnCountResponse {
        status: "ok",
        select_distinct_on_count,
    })))
}


// S3-WS1-68: rows/for/update/count endpoint
pub(crate) async fn rows_for_update_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsForUpdateCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_for_update_count");
    let mut for_update_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" FOR UPDATE") || value_up.contains(" FOR SHARE") {
                for_update_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsForUpdateCountResponse {
        status: "ok",
        for_update_count,
    })))
}


// S3-WS1-69: rows/left/join/count endpoint
pub(crate) async fn rows_left_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsLeftJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_left_join_count");
    let mut left_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" LEFT JOIN ") || value_up.contains(" LEFT OUTER JOIN ") {
                left_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsLeftJoinCountResponse {
        status: "ok",
        left_join_count,
    })))
}


// S3-WS1-70: rows/right/join/count endpoint
pub(crate) async fn rows_right_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsRightJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_right_join_count");
    let mut right_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" RIGHT JOIN ") || value_up.contains(" RIGHT OUTER JOIN ") {
                right_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsRightJoinCountResponse {
        status: "ok",
        right_join_count,
    })))
}


// S3-WS1-71: rows/full_outer/join/count endpoint
pub(crate) async fn rows_full_outer_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFullOuterJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_full_outer_join_count");
    let mut full_outer_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" FULL JOIN ") || value_up.contains(" FULL OUTER JOIN ") {
                full_outer_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsFullOuterJoinCountResponse {
        status: "ok",
        full_outer_join_count,
    })))
}


// S3-WS1-72: rows/inner/join/count endpoint
pub(crate) async fn rows_inner_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsInnerJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_inner_join_count");
    let mut inner_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" INNER JOIN ") {
                inner_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsInnerJoinCountResponse {
        status: "ok",
        inner_join_count,
    })))
}


// S3-WS1-73: rows/straight/join/count endpoint
pub(crate) async fn rows_straight_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsStraightJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_straight_join_count");
    let mut straight_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" STRAIGHT_JOIN ") {
                straight_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsStraightJoinCountResponse {
        status: "ok",
        straight_join_count,
    })))
}


// S3-WS1-74: rows/semi/join/count endpoint
pub(crate) async fn rows_semi_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsSemiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_semi_join_count");
    let mut semi_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" SEMI JOIN ") {
                semi_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsSemiJoinCountResponse {
        status: "ok",
        semi_join_count,
    })))
}


// S3-WS1-75: rows/anti/join/count endpoint
pub(crate) async fn rows_anti_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsAntiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_anti_join_count");
    let mut anti_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" ANTI JOIN ") {
                anti_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsAntiJoinCountResponse {
        status: "ok",
        anti_join_count,
    })))
}


// S3-WS1-76: rows/cross/apply/count endpoint
pub(crate) async fn rows_cross_apply_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsCrossApplyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_cross_apply_count");
    let mut cross_apply_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" CROSS APPLY ") {
                cross_apply_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsCrossApplyCountResponse {
        status: "ok",
        cross_apply_count,
    })))
}


// S3-WS1-77: rows/outer/apply/count endpoint
pub(crate) async fn rows_outer_apply_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsOuterApplyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_outer_apply_count");
    let mut outer_apply_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" OUTER APPLY ") {
                outer_apply_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsOuterApplyCountResponse {
        status: "ok",
        outer_apply_count,
    })))
}


// S3-WS1-78: rows/apply/count endpoint
pub(crate) async fn rows_apply_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsApplyCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_apply_count");
    let mut apply_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" APPLY ") {
                apply_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsApplyCountResponse {
        status: "ok",
        apply_count,
    })))
}


// S3-WS1-79: rows/left/semi/join/count endpoint
pub(crate) async fn rows_left_semi_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsLeftSemiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_left_semi_join_count");
    let mut left_semi_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" LEFT SEMI JOIN ") {
                left_semi_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsLeftSemiJoinCountResponse {
        status: "ok",
        left_semi_join_count,
    })))
}


// S3-WS1-80: rows/left/anti/join/count endpoint
pub(crate) async fn rows_left_anti_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsLeftAntiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_left_anti_join_count");
    let mut left_anti_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" LEFT ANTI JOIN ") {
                left_anti_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsLeftAntiJoinCountResponse {
        status: "ok",
        left_anti_join_count,
    })))
}


// S3-WS1-81: rows/right/semi/join/count endpoint
pub(crate) async fn rows_right_semi_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsRightSemiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_right_semi_join_count");
    let mut right_semi_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" RIGHT SEMI JOIN ") {
                right_semi_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsRightSemiJoinCountResponse {
        status: "ok",
        right_semi_join_count,
    })))
}


// S3-WS1-82: rows/right/anti/join/count endpoint
pub(crate) async fn rows_right_anti_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsRightAntiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_right_anti_join_count");
    let mut right_anti_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" RIGHT ANTI JOIN ") {
                right_anti_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsRightAntiJoinCountResponse {
        status: "ok",
        right_anti_join_count,
    })))
}


// S3-WS1-83: rows/full/semi/join/count endpoint
pub(crate) async fn rows_full_semi_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFullSemiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_full_semi_join_count");
    let mut full_semi_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" FULL SEMI JOIN ") {
                full_semi_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsFullSemiJoinCountResponse {
        status: "ok",
        full_semi_join_count,
    })))
}


// S3-WS1-84: rows/full/anti/join/count endpoint
pub(crate) async fn rows_full_anti_join_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsFullAntiJoinCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_full_anti_join_count");
    let mut full_anti_join_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" FULL ANTI JOIN ") {
                full_anti_join_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsFullAntiJoinCountResponse {
        status: "ok",
        full_anti_join_count,
    })))
}


// S3-WS1-85: rows/union/all/count endpoint
pub(crate) async fn rows_union_all_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsUnionAllCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_union_all_count");
    let mut union_all_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if value_up.contains(" UNION ALL ") {
                union_all_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsUnionAllCountResponse {
        status: "ok",
        union_all_count,
    })))
}


// S3-WS1-86: rows/aggregate/distinct/count endpoint
pub(crate) async fn rows_aggregate_distinct_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsAggregateDistinctCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_aggregate_distinct_count");
    let mut aggregate_distinct_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase().replace(' ', "");
            if value_up.contains("COUNT(DISTINCT")
                || value_up.contains("SUM(DISTINCT")
                || value_up.contains("AVG(DISTINCT")
                || value_up.contains("MIN(DISTINCT")
                || value_up.contains("MAX(DISTINCT")
            {
                aggregate_distinct_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsAggregateDistinctCountResponse {
        status: "ok",
        aggregate_distinct_count,
    })))
}


// S3-WS1-87: rows/table/alias/count endpoint
pub(crate) async fn rows_table_alias_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsTableAliasCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_table_alias_count");
    let mut table_alias_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if contains_table_alias_sql(&value_up) {
                table_alias_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsTableAliasCountResponse {
        status: "ok",
        table_alias_count,
    })))
}


// S3-WS1-88: rows/sql/column/alias/count endpoint
pub(crate) async fn rows_column_alias_count(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsColumnAliasCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state
        .row_store
        .lock()
        .expect("row_store lock rows_column_alias_count");
    let mut column_alias_count = 0;
    for (_, row) in rs.export_rows_snapshot() {
        for value in row.into_values() {
            let value_up = value.to_ascii_uppercase();
            if contains_column_alias_sql(&value_up) {
                column_alias_count += 1;
            }
        }
    }
    drop(rs);
    Ok((StatusCode::OK, Json(RowsColumnAliasCountResponse {
        status: "ok",
        column_alias_count,
    })))
}

