
/// Build the full axum router with all 330+ routes wired to handler functions.
/// Extracted from main() in Slice 8 to keep main() readable.
pub(crate) fn build_router(state: crate::AppState) -> axum::Router {
    use axum::routing::{get, options, post};
    use axum::Router;
    use axum::middleware::from_fn;
    use crate::handlers::audit::*;
    use crate::handlers::rows::*;
    use crate::handlers::raft::*;
    use crate::handlers::misc::*;
    use crate::handlers::wal::*;
    use crate::handlers::sql::*;
    use crate::handlers::sre::*;
    use crate::handlers::store::*;
    use crate::handlers::cdc::*;
    use crate::handlers::catalog::*;
    use crate::handlers::autonomous::*;
    use crate::handlers::security::*;
    use crate::handlers::admin::*;
    use crate::handlers::driver::*;
    use crate::handlers::ingest::*;

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler))
        .route("/api/v1/sql/transaction", post(sql_transaction))
        .route(
            "/api/v1/sql/locks/pessimistic/acquire",
            post(sql_pessimistic_lock_acquire),
        )
        .route(
            "/api/v1/sql/locks/pessimistic/release",
            post(sql_pessimistic_lock_release),
        )
        .route(
            "/api/v1/sql/locks/pessimistic/metrics",
            get(sql_pessimistic_lock_metrics),
        )
        .route("/api/v1/sql/analyze", post(sql_analyze))
        .route("/api/v1/sql/route", post(sql_route))
        .route("/api/v1/sql/execute", post(sql_execute))
        .route("/api/v1/olap/query", post(olap_query))
        .route("/api/v1/failover/status", get(failover_status))
        .route("/api/v1/failover/simulate", post(failover_simulate))
        .route("/api/v1/admin/cluster/topology", get(admin_cluster_topology))
        .route("/api/v1/admin/schema/tree", get(admin_schema_tree))
        // Phase 1.3 — first-class database lifecycle.
        .route("/api/v1/admin/databases", get(admin_databases_list).post(admin_databases_create))
        .route("/api/v1/admin/databases/:name", axum::routing::delete(admin_databases_drop))
        .route("/api/v1/admin/databases/:name/metadata", get(admin_databases_metadata))
        .route("/api/v1/admin/databases/:name/metadata/:table", get(admin_databases_metadata_rows))
        // Phase 0 — surface the boot-time runtime config (read-only).
        .route("/api/v1/admin/runtime-config", get(admin_runtime_config))
        .route("/api/v1/admin/sql/transactions/control", post(admin_sql_transaction_control))
        .route("/api/v1/admin/sql/locks/control", post(admin_sql_lock_control))
        .route("/api/v1/admin/cluster/nodes/manage", post(admin_cluster_node_manage))
        .route("/api/v1/sre/reliability/status", get(sre_reliability_status))
        .route("/api/v1/sre/rate-limit/check", post(sre_rate_limit_check))
        .route(
            "/api/v1/sre/failure-budget/alerts",
            get(sre_failure_budget_alerts),
        )
        .route("/api/v1/sre/dr/hooks/policy", get(sre_dr_hook_policy))
        .route("/api/v1/sre/dr/hooks/retry-plan", get(sre_dr_hook_retry_plan))
        .route("/api/v1/sre/dr/hooks/schedule", post(sre_dr_hook_schedule))
        .route("/api/v1/sre/dr/hooks/trigger", post(sre_dr_hook_trigger))
        .route("/api/v1/sre/dr/hooks/status", get(sre_dr_hook_status))
        .route("/api/v1/sre/failure/signal", post(sre_failure_signal))
        .route("/api/v1/sre/failure/reconcile", post(sre_failure_reconcile))
        .route("/api/v1/sre/gate/evaluate", get(sre_gate_evaluate))
        .route("/api/v1/sre/gate/export", post(sre_gate_export))
        .route("/api/v1/sre/cache/set", post(sre_cache_set))
        .route("/api/v1/sre/cache/get", get(sre_cache_get))
        .route("/api/v1/sre/cache/invalidate", post(sre_cache_invalidate))
        .route("/api/*path", options(options_preflight))
        .layer(from_fn(add_cors))
        .layer(from_fn(track_http_metrics))
        .route("/api/v1/sre/cache/rebalance", post(sre_cache_rebalance))
        .route("/api/v1/sre/cache/metrics", get(sre_cache_metrics))
        // REQ-27: Redis-compat cache command interface
        .route("/api/v1/cache/redis/command", post(cache_redis_command))
        .route("/api/v1/sre/driver/pool/acquire", post(sre_driver_pool_acquire))
        .route("/api/v1/sre/driver/pool/release", post(sre_driver_pool_release))
        .route("/api/v1/sre/driver/pool/failure", post(sre_driver_pool_failure))
        .route("/api/v1/sre/driver/pool/recover", post(sre_driver_pool_recover))
        .route("/api/v1/sre/driver/pool/stats", get(sre_driver_pool_stats))
        .route(
            "/api/v1/security/plugins/provenance/register",
            post(security_plugins_provenance_register),
        )
        .route("/api/v1/audit/events", get(audit_events))
        // S9-WS8A-02: tamper-evident audit chain verification
        .route("/api/v1/audit/chain/verify", get(audit_chain_verify))
        .route("/api/v1/audit/snapshot", get(audit_snapshot))
        // S9-WS8A-01: Audit CLI summary
        .route("/api/v1/audit/cli/summary", get(audit_cli_summary))
        .route("/api/v1/security/kms/status", get(security_kms_status))
        .route(
            "/api/v1/security/kms/outage/simulate",
            post(security_kms_outage_simulate),
        )
        .route(
            "/api/v1/security/kms/outage/reconcile",
            post(security_kms_outage_reconcile),
        )
        .route("/api/v1/i18n/messages", get(i18n_messages))
        .route(
            "/api/v1/autonomous/actions/records",
            get(autonomous_action_records),
        )
        .route("/api/v1/autonomous/guardrails", get(autonomous_guardrails))
        .route(
            "/api/v1/autonomous/emergency-stop",
            post(autonomous_emergency_stop),
        )
        .route(
            "/api/v1/autonomous/actions/authorize",
            post(authorize_autonomous_action),
        )
        // WS2 Index + Constraint endpoints
        .route("/api/v1/store/indexes", get(store_list_indexes))
        .route("/api/v1/store/indexes/create", post(store_create_index))
        .route("/api/v1/store/indexes/drop", post(store_drop_index))
        .route("/api/v1/store/indexes/lookup", post(store_index_lookup))
        .route(
            "/api/v1/store/constraints/add",
            post(store_add_constraint),
        )
        .route(
            "/api/v1/store/constraints/validate",
            post(store_validate_constraint),
        )
        // S4-WS3-04: HTAP OLAP consumer apply + scan
        .route("/api/v1/store/htap/apply", post(store_htap_apply))
        .route("/api/v1/store/htap/olap/scan", get(store_htap_olap_scan))
        .route("/api/v1/store/htap/lag", get(htap_lag))
        // S4-WS3-04: HTAP force-sync — drain sync_origin into olap_store
        .route("/api/v1/store/htap/sync", post(htap_force_sync))
        // S4-WS3-04: HTAP detailed status
        .route("/api/v1/store/htap/status", get(htap_status))
        // S11-WS1-11: HTAP OLAP store statistics
        .route("/api/v1/store/htap/stats", get(htap_stats))
        // S9-WS8A-02: Audit export (all buffered events + file-backed status)
        .route("/api/v1/audit/export", get(audit_export))
        // S9-WS8A-02: Audit purge — flush in-memory audit sink
        .route("/api/v1/audit/purge", post(audit_purge))
        // S7-WS6-02: Raft consensus RPC + status endpoints
        .route("/api/v1/cluster/raft/status", get(raft_status))
        .route("/api/v1/cluster/raft/vote", post(raft_vote))
        .route("/api/v1/cluster/raft/append", post(raft_append))
        .route("/api/v1/cluster/raft/tick", post(raft_tick))
        .route("/api/v1/cluster/raft/log", get(raft_log))
        // S7-WS6-02: raft commit progress
        .route("/api/v1/cluster/raft/commit", get(raft_commit_progress))
        // S7-WS6-02: Raft point-in-time snapshot
        .route("/api/v1/cluster/raft/snapshot", get(raft_snapshot))
        .route("/api/v1/cluster/raft/heartbeat", post(raft_heartbeat))
        // S7-WS6-02: Raft election timer status
        .route("/api/v1/cluster/raft/election/status", get(raft_election_status))
        .route("/api/v1/cluster/raft/members", get(raft_member_list))
        // S7-WS6-03: Raft current leader
        .route("/api/v1/cluster/raft/leader", get(raft_leader))
        // S7-WS6-03: Raft fencing token
        .route("/api/v1/cluster/raft/fence", get(raft_fence))
        // S7-WS6-01: Raft vote statistics
        .route("/api/v1/cluster/raft/vote/stats", get(raft_vote_stats))
        // §7: Raft install-snapshot (leader → follower full state transfer)
        .route("/api/v1/cluster/raft/install_snapshot", post(raft_install_snapshot))
        .route("/api/v1/store/rows/scan", post(store_rows_scan))
        // S4-WS3-04: HTAP sync export for OLAP consumers
        .route("/api/v1/store/htap/export", post(store_htap_export))
        // S4-WS3-03: vectorized columnar scan
        .route("/api/v1/store/columnar/scan", get(store_columnar_scan))
        .route("/api/v1/store/columnar/project", get(store_columnar_project))
        // S4-WS3-03: Columnar vectorized aggregate
        .route("/api/v1/store/columnar/aggregate", get(store_columnar_aggregate))
        // S6-WS5-03: TLS runtime status
        .route("/api/v1/security/tls/status", get(security_tls_status))
        .route("/api/v1/security/tls/rotate", post(security_tls_rotate))
        // S6-WS5-03: TLS certificate info
        .route("/api/v1/security/tls/cert/info", get(security_tls_cert_info))
        // S6-WS5-04: TDE/encryption-at-rest status
        .route("/api/v1/security/tde/status", get(security_tde_status))
        // S6-WS5-04: TDE toggle override
        .route("/api/v1/security/tde/toggle", post(security_tde_toggle))
        // S6-WS5-04: TDE runtime override status
        .route("/api/v1/security/tde/override-status", get(security_tde_override_status))
        // S9-WS8-02: AI model gateway policy
        .route("/api/v1/ai/policy", get(ai_policy))
        // S2-WS2-02: WAL durability status + recovery replay
        .route("/api/v1/store/wal/status", get(wal_status))
        .route("/api/v1/store/wal/recover", post(wal_recover))
        .route("/api/v1/store/wal/checkpoint", post(wal_force_checkpoint))
        .route("/api/v1/store/wal/stats", get(wal_stats))
        // S2-WS2-02: WAL compact
        .route("/api/v1/store/wal/compact", post(wal_compact))
        // S2-WS2-02: WAL bounds (oldest/newest sequence)
        .route("/api/v1/store/wal/bounds", get(wal_bounds))
        // S2-WS2-02: WAL replay (filtered read-back)
        .route("/api/v1/store/wal/replay", get(wal_replay))
        // S2-WS2-02: WAL tail (last N records)
        .route("/api/v1/store/wal/tail", get(wal_tail))
        // S2-WS2-03: WAL mutations (recent key-value changes)
        .route("/api/v1/store/wal/mutations", get(wal_mutations))
        // S11-WS1-13: WAL latest sequence info
        .route("/api/v1/store/wal/seq", get(wal_seq))
        // S11-WS1-14: WAL head records (first N entries)
        .route("/api/v1/store/wal/head", get(wal_head))
        // S11-WS1-15: WAL range (records within a sequence range)
        .route("/api/v1/store/wal/range", get(wal_range))
        // S11-WS1-16: WAL size estimate
        .route("/api/v1/store/wal/size", get(wal_size))
        // S11-WS1-17: WAL latest single record
        .route("/api/v1/store/wal/latest", get(wal_latest))
        // S11-WS1-18: WAL records filtered by key prefix
        .route("/api/v1/store/wal/by-key", get(wal_by_key))
        // S11-WS1-19: Latest WAL checkpoint info
        .route("/api/v1/store/wal/checkpoint/latest", get(wal_checkpoint_latest))
        // S11-WS1-20: WAL insert/delete delta counts
        .route("/api/v1/store/wal/delta", get(wal_delta))
        // S11-WS1-21: Unique key count across all WAL records
        .route("/api/v1/store/wal/unique/keys", get(wal_unique_keys))
        // S11-WS1-22: WAL age (oldest/newest sequence span)
        .route("/api/v1/store/wal/age", get(wal_age))
        // S11-WS1-23: List all unique keys in the WAL
        .route("/api/v1/store/wal/keys/list", get(wal_keys_list))
        // S2-WS2-02: WAL checkpoint history
        .route("/api/v1/store/wal/checkpoint/history", get(wal_checkpoint_history))
        // S11-WS1-10: WAL truncate up to sequence
        .route("/api/v1/store/wal/truncate", post(wal_truncate))
        // S2-WS2-02: WAL segment list (checkpoint groups)
        .route("/api/v1/store/wal/segment/list", get(wal_segment_list))
        // S2-WS2-02: WAL replay count (filtered record count, no body)
        .route("/api/v1/store/wal/replay/count", get(wal_replay_count))
        // S7-WS6-04: Chaos/game-day fault injection
        .route("/api/v1/cluster/chaos/inject", post(chaos_inject))
        .route("/api/v1/cluster/chaos/clear", post(chaos_clear))
        .route("/api/v1/cluster/chaos/status", get(chaos_status))
        .route("/api/v1/cluster/chaos/health", get(chaos_health))
        // S7-WS6-04: Chaos event history
        .route("/api/v1/cluster/chaos/history", get(chaos_history))
        // S7-WS6-04: Chaos fire drill
        .route("/api/v1/cluster/chaos/fire-drill", post(chaos_fire_drill))
        // S8-WS10-02: Driver wire protocol info + session connect + disconnect
        .route("/api/v1/driver/protocol/info", get(driver_protocol_info))
        .route("/api/v1/driver/connect", post(driver_connect))
        .route("/api/v1/driver/disconnect", post(driver_disconnect))
        .route("/api/v1/driver/sessions", get(driver_session_list))
        // S8-WS10-02: driver pool health
        .route("/api/v1/driver/health", get(driver_health))
        // S8-WS10-02: driver query pass-through
        .route("/api/v1/driver/query", post(driver_query))
        // S8-WS10-02: driver session ping/keepalive
        .route("/api/v1/driver/ping", post(driver_ping))
        // S8-WS10-02: driver pool stats (operator-facing)
        .route("/api/v1/driver/pool/stats", get(driver_pool_stats))
        // S10-WS15-02: CDC stream from WAL
        .route("/api/v1/store/cdc/stream", get(cdc_stream))
        .route("/api/v1/store/cdc/stream/filter", get(cdc_stream_filter))
        // S10-WS15-02: CDC stream latest N events
        .route("/api/v1/store/cdc/stream/latest", get(cdc_stream_latest))
        .route("/api/v1/store/cdc/cursor", get(cdc_cursor_status))
        .route("/api/v1/store/cdc/cursor/advance", post(cdc_cursor_advance))
        // S10-WS15-02: CDC cursor rewind
        .route("/api/v1/store/cdc/cursor/rewind", post(cdc_cursor_rewind))
        // S10-WS15-02: CDC cursor list (all tracked table positions)
        .route("/api/v1/store/cdc/cursor/list", get(cdc_cursor_list))
        // S10-WS15-02: CDC aggregate metrics
        .route("/api/v1/store/cdc/metrics", get(cdc_metrics))
        // S2-WS2-04: Row store point-in-time snapshot export
        .route("/api/v1/store/rows/snapshot", get(row_store_snapshot))
        // S2-WS2-04: Row store operational stats
        .route("/api/v1/store/rows/stats", get(row_store_stats))
        // S2-WS2-04: Row store key-prefix count
        .route("/api/v1/store/rows/count", get(row_store_count))
        // S2-WS2-04: Row store delete by key
        .route("/api/v1/store/rows/delete", post(row_store_delete))
        // S11-WS1-10: Row store key list
        .route("/api/v1/store/rows/keys", get(store_rows_keys))
        // S11-WS1-11: Row store version / current transaction ID
        .route("/api/v1/store/rows/version", get(row_store_version))
        // S11-WS1-14: Rows modified after a given transaction ID
        .route("/api/v1/store/rows/modified", get(rows_modified))
        // S11-WS1-15: Row store current XID info
        .route("/api/v1/store/rows/xid", get(rows_xid))
        // S11-WS1-16: Visible row count at current snapshot
        .route("/api/v1/store/rows/visible", get(rows_visible))
        // S11-WS1-17: Total row count (all versions)
        .route("/api/v1/store/rows/total", get(rows_total))
        // S11-WS1-18: Count of distinct row keys
        .route("/api/v1/store/rows/keys/count", get(rows_keys_count))
        // S11-WS1-20: Count of tombstone (deleted) rows
        .route("/api/v1/store/rows/tombstone/count", get(rows_tombstone_count))
        // S11-WS1-21: Row store XID history (current + next + total)
        .route("/api/v1/store/rows/xid/history", get(rows_xid_history))
        // S11-WS1-22: First key in the row store (alphabetically)
        .route("/api/v1/store/rows/first/key", get(rows_first_key))
        // S11-WS1-23: Last alphabetically-sorted key in the row store
        .route("/api/v1/store/rows/last/key", get(rows_last_key))
        // S11-WS1-24: Count of distinct row values in the store
        .route("/api/v1/store/rows/count/distinct", get(rows_count_distinct))
        // S11-WS1-24: Check if a given key exists in the row store
        .route("/api/v1/store/rows/key/exists", get(rows_key_exists))
        // S11-WS1-25: Search rows by value
        .route("/api/v1/store/rows/value/search", get(rows_value_search))
        // S11-WS1-25: Count total WAL records
        .route("/api/v1/store/wal/record/count", get(wal_record_count))
        // S11-WS1-26: Count rows optionally filtered by key prefix
        .route("/api/v1/store/rows/count/range", get(rows_count_range))
        // S11-WS1-26: WAL checkpoint age (oldest/newest seqno)
        .route("/api/v1/store/wal/checkpoint/age", get(wal_checkpoint_age))
        // S11-WS1-27: Total payload field count across all rows
        .route("/api/v1/store/rows/payload/size", get(rows_payload_size))
        // S11-WS1-27: WAL flush count (total writes)
        .route("/api/v1/store/wal/flush/count", get(wal_flush_count))
        // S3-WS1-28: rows/field/count + wal/entry/latest
        .route("/api/v1/store/rows/field/count", get(rows_field_count))
        .route("/api/v1/store/wal/entry/latest", get(wal_entry_latest))
        // S3-WS1-29: wal/write/count + rows/key/longest
        .route("/api/v1/store/wal/write/count", get(wal_write_count))
        .route("/api/v1/store/rows/key/longest", get(rows_key_longest))
        // S3-WS1-30: rows/key/shortest (wal/age exists from S22)
        .route("/api/v1/store/rows/key/shortest", get(rows_key_shortest))
        // S3-WS1-31: wal/min/seq + rows/count/all
        .route("/api/v1/store/wal/min/seq", get(wal_min_seq))
        .route("/api/v1/store/rows/count/all", get(rows_count_all))
        // S3-WS1-32: wal/max/seq + rows/snapshot/size
        .route("/api/v1/store/wal/max/seq", get(wal_max_seq))
        .route("/api/v1/store/rows/snapshot/size", get(rows_snapshot_size))
        // S3-WS1-33: wal/entry/count + rows/version/latest
        .route("/api/v1/store/wal/entry/count", get(wal_entry_count))
        .route("/api/v1/store/rows/version/latest", get(rows_version_latest))
        // S3-WS1-34: wal/size/bytes + rows/distinct/count
        .route("/api/v1/store/wal/size/bytes", get(wal_size_bytes))
        .route("/api/v1/store/rows/distinct/count", get(rows_distinct_count))
        // S3-WS1-35: wal/delete/count + rows/key/median
        .route("/api/v1/store/wal/delete/count", get(wal_delete_count))
        .route("/api/v1/store/rows/key/median", get(rows_key_median))
        // S3-WS1-36: wal/validate + rows/checksum
        .route("/api/v1/store/wal/validate", get(wal_validate))
        .route("/api/v1/store/rows/checksum", get(rows_checksum))
        // S3-WS1-37: wal/entry/oldest + rows/field/types
        .route("/api/v1/store/wal/entry/oldest", get(wal_entry_oldest))
        .route("/api/v1/store/rows/field/types", get(rows_field_types))
        // S3-WS1-38: wal/seq/span + rows/key/empty/count
        .route("/api/v1/store/wal/seq/span", get(wal_seq_span))
        .route("/api/v1/store/rows/key/empty/count", get(rows_key_empty_count))
        // S3-WS1-39: wal/record/active + rows/key/min
        .route("/api/v1/store/wal/record/active", get(wal_record_active))
        .route("/api/v1/store/rows/key/min", get(rows_key_min))
        // S3-WS1-40: wal/record/mutations + rows/field/cardinality
        .route("/api/v1/store/wal/record/mutations", get(wal_record_mutations))
        .route("/api/v1/store/rows/field/cardinality", get(rows_field_cardinality))
        // S3-WS1-41: wal/record/deleted + rows/key/max
        .route("/api/v1/store/wal/record/deleted", get(wal_record_deleted))
        .route("/api/v1/store/rows/key/max", get(rows_key_max))
        // S3-WS1-42: wal/mutation/span + rows/value/non_null/count
        .route("/api/v1/store/wal/mutation/span", get(wal_mutation_span))
        .route("/api/v1/store/rows/value/non_null/count", get(rows_value_non_null_count))
        // S3-WS1-43: wal/mutation/count/non_deleted + rows/value/empty/count
        .route("/api/v1/store/wal/mutation/count/non_deleted", get(wal_mutation_non_deleted_count))
        .route("/api/v1/store/rows/value/empty/count", get(rows_value_empty_count))
        // S3-WS1-44: wal/non_deleted/span + rows/value/non_empty/count
        .route("/api/v1/store/wal/non_deleted/span", get(wal_non_deleted_span))
        .route("/api/v1/store/rows/value/non_empty/count", get(rows_value_non_empty_count))
        // S3-WS1-45: wal/non_deleted/count + rows/key/non_empty/count
        .route("/api/v1/store/wal/non_deleted/count", get(wal_non_deleted_count))
        .route("/api/v1/store/rows/key/non_empty/count", get(rows_key_non_empty_count))
        // S3-WS1-46: wal/non_deleted/latest + rows/value/non_blank/count
        .route("/api/v1/store/wal/non_deleted/latest", get(wal_non_deleted_latest))
        .route("/api/v1/store/rows/value/non_blank/count", get(rows_value_non_blank_count))
        // S3-WS1-47: wal/non_deleted/oldest + rows/key/non_blank/count
        .route("/api/v1/store/wal/non_deleted/oldest", get(wal_non_deleted_oldest))
        .route("/api/v1/store/rows/key/non_blank/count", get(rows_key_non_blank_count))
        // S3-WS1-48: wal/non_deleted/newest + rows/value/blank/count
        .route("/api/v1/store/wal/non_deleted/newest", get(wal_non_deleted_newest))
        .route("/api/v1/store/rows/value/blank/count", get(rows_value_blank_count))
        // S3-WS1-49: wal/record/total + rows/key/duplicates/count
        .route("/api/v1/store/wal/record/total", get(wal_record_total))
        .route("/api/v1/store/rows/key/duplicates/count", get(rows_key_duplicates_count))
        // S3-WS1-50: wal/value/duplicates/count + rows/value/duplicates/count
        .route("/api/v1/store/wal/value/duplicates/count", get(wal_value_duplicates_count))
        .route("/api/v1/store/rows/value/duplicates/count", get(rows_value_duplicates_count))
        // S3-WS1-51: wal/value/distinct/count + rows/value/distinct/count
        .route("/api/v1/store/wal/value/distinct/count", get(wal_value_distinct_count))
        .route("/api/v1/store/rows/value/distinct/count", get(rows_value_distinct_count))
        // S3-WS1-52: wal/value/unique/count + rows/value/unique/count
        .route("/api/v1/store/wal/value/unique/count", get(wal_value_unique_count))
        .route("/api/v1/store/rows/value/unique/count", get(rows_value_unique_count))
        // S3-WS1-53: wal/value/trimmed/count + rows/value/trimmed/count
        .route("/api/v1/store/wal/value/trimmed/count", get(wal_value_trimmed_count))
        .route("/api/v1/store/rows/value/trimmed/count", get(rows_value_trimmed_count))
        // S3-WS1-54: wal/value/case_variant/count + rows/value/case_variant/count
        .route("/api/v1/store/wal/value/case_variant/count", get(wal_value_case_variant_count))
        .route("/api/v1/store/rows/value/case_variant/count", get(rows_value_case_variant_count))
        .route("/api/v1/store/wal/order_by/desc_direction/count", get(wal_order_by_desc_direction_count))
        .route("/api/v1/store/rows/order_by/desc_direction/count", get(rows_order_by_desc_direction_count))
        .route("/api/v1/store/wal/order_by/random/count", get(wal_order_by_random_count))
        .route("/api/v1/store/rows/order_by/random/count", get(rows_order_by_random_count))
        .route("/api/v1/store/wal/order_by/random_seeded/count", get(wal_order_by_random_seeded_count))
        .route("/api/v1/store/rows/order_by/random_seeded/count", get(rows_order_by_random_seeded_count))
        .route("/api/v1/store/wal/order_by/asc_direction/count", get(wal_order_by_asc_direction_count))
        .route("/api/v1/store/rows/order_by/asc_direction/count", get(rows_order_by_asc_direction_count))
        .route("/api/v1/store/wal/order_by/rand_alias/count", get(wal_order_by_rand_alias_count))
        .route("/api/v1/store/rows/order_by/rand_alias/count", get(rows_order_by_rand_alias_count))
        .route("/api/v1/store/wal/order_by/multi_column/count", get(wal_order_by_multi_column_count))
        .route("/api/v1/store/rows/order_by/multi_column/count", get(rows_order_by_multi_column_count))
        .route("/api/v1/store/wal/pagination/limit_offset/count", get(wal_pagination_limit_offset_count))
        .route("/api/v1/store/rows/pagination/limit_offset/count", get(rows_pagination_limit_offset_count))
        .route("/api/v1/store/wal/pagination/offset_only/count", get(wal_pagination_offset_only_count))
        .route("/api/v1/store/rows/pagination/offset_only/count", get(rows_pagination_offset_only_count))
        .route("/api/v1/store/wal/having_without_group_by/count", get(wal_having_without_group_by_count))
        .route("/api/v1/store/rows/having_without_group_by/count", get(rows_having_without_group_by_count))
        .route("/api/v1/store/wal/having_with_group_by/count", get(wal_having_with_group_by_count))
        .route("/api/v1/store/rows/having_with_group_by/count", get(rows_having_with_group_by_count))
        .route("/api/v1/store/wal/group_by/rollup/count", get(wal_group_by_rollup_count))
        .route("/api/v1/store/rows/group_by/rollup/count", get(rows_group_by_rollup_count))
        .route("/api/v1/store/wal/group_by/cube/count", get(wal_group_by_cube_count))
        .route("/api/v1/store/rows/group_by/cube/count", get(rows_group_by_cube_count))
        .route("/api/v1/store/wal/select/distinct_on/count", get(wal_select_distinct_on_count))
        .route("/api/v1/store/rows/select/distinct_on/count", get(rows_select_distinct_on_count))
        .route("/api/v1/store/wal/for/update/count", get(wal_for_update_count))
        .route("/api/v1/store/rows/for/update/count", get(rows_for_update_count))
        .route("/api/v1/store/wal/left/join/count", get(wal_left_join_count))
        .route("/api/v1/store/rows/left/join/count", get(rows_left_join_count))
        .route("/api/v1/store/wal/right/join/count", get(wal_right_join_count))
        .route("/api/v1/store/rows/right/join/count", get(rows_right_join_count))
        .route("/api/v1/store/wal/full_outer/join/count", get(wal_full_outer_join_count))
        .route("/api/v1/store/rows/full_outer/join/count", get(rows_full_outer_join_count))
        .route("/api/v1/store/wal/inner/join/count", get(wal_inner_join_count))
        .route("/api/v1/store/rows/inner/join/count", get(rows_inner_join_count))
        .route("/api/v1/store/wal/straight/join/count", get(wal_straight_join_count))
        .route("/api/v1/store/rows/straight/join/count", get(rows_straight_join_count))
        .route("/api/v1/store/wal/semi/join/count", get(wal_semi_join_count))
        .route("/api/v1/store/rows/semi/join/count", get(rows_semi_join_count))
        .route("/api/v1/store/wal/anti/join/count", get(wal_anti_join_count))
        .route("/api/v1/store/rows/anti/join/count", get(rows_anti_join_count))
        .route("/api/v1/store/wal/cross/apply/count", get(wal_cross_apply_count))
        .route("/api/v1/store/rows/cross/apply/count", get(rows_cross_apply_count))
        .route("/api/v1/store/wal/outer/apply/count", get(wal_outer_apply_count))
        .route("/api/v1/store/rows/outer/apply/count", get(rows_outer_apply_count))
        .route("/api/v1/store/wal/apply/count", get(wal_apply_count))
        .route("/api/v1/store/rows/apply/count", get(rows_apply_count))
        .route("/api/v1/store/wal/left/semi/join/count", get(wal_left_semi_join_count))
        .route("/api/v1/store/rows/left/semi/join/count", get(rows_left_semi_join_count))
        .route("/api/v1/store/wal/left/anti/join/count", get(wal_left_anti_join_count))
        .route("/api/v1/store/rows/left/anti/join/count", get(rows_left_anti_join_count))
        .route("/api/v1/store/wal/right/semi/join/count", get(wal_right_semi_join_count))
        .route("/api/v1/store/rows/right/semi/join/count", get(rows_right_semi_join_count))
        .route("/api/v1/store/wal/right/anti/join/count", get(wal_right_anti_join_count))
        .route("/api/v1/store/rows/right/anti/join/count", get(rows_right_anti_join_count))
        .route("/api/v1/store/wal/full/semi/join/count", get(wal_full_semi_join_count))
        .route("/api/v1/store/rows/full/semi/join/count", get(rows_full_semi_join_count))
        .route("/api/v1/store/wal/full/anti/join/count", get(wal_full_anti_join_count))
        .route("/api/v1/store/rows/full/anti/join/count", get(rows_full_anti_join_count))
        .route("/api/v1/store/wal/union/all/count", get(wal_union_all_count))
        .route("/api/v1/store/rows/union/all/count", get(rows_union_all_count))
        .route("/api/v1/store/wal/aggregate/distinct/count", get(wal_aggregate_distinct_count))
        .route("/api/v1/store/rows/aggregate/distinct/count", get(rows_aggregate_distinct_count))
        .route("/api/v1/store/wal/table/alias/count", get(wal_table_alias_count))
        .route("/api/v1/store/rows/table/alias/count", get(rows_table_alias_count))
        .route("/api/v1/store/wal/sql/column/alias/count", get(wal_column_alias_count))
        .route("/api/v1/store/rows/sql/column/alias/count", get(rows_column_alias_count))
        // S11-WS1-19: Scan all rows visible at current snapshot
        .route("/api/v1/store/rows/scan/visible", get(rows_scan_visible))
        // S11-WS1-12: Row store page-level stats
        .route("/api/v1/store/rows/page/stats", get(rows_page_stats))
        // S5-WS4A-02: Broker adapter status + flush
        .route("/api/v1/ingest/outbox/broker/status", get(outbox_broker_status))
        .route("/api/v1/ingest/outbox/broker/flush", post(outbox_broker_flush))
        .route("/api/v1/ingest/outbox/broker/health", get(outbox_broker_health))
        // S5-E4A-01: Connector SDK runtime load
        .route("/api/v1/connectors", get(connector_list))
        .route("/api/v1/connectors/register", post(connector_register))
        .route("/api/v1/connectors/deregister", post(connector_deregister))
        // S5-E4A-01: Connector get by ID
        .route("/api/v1/connectors/get", get(connector_get))
        // S5-E4A-01: Connector update (version / signed flag)
        .route("/api/v1/connectors/update", post(connector_update))
        // S11-WS1-12: Connector health check
        .route("/api/v1/connectors/health", get(connectors_health))
        .route("/api/v1/ai/policy/update", post(ai_policy_update))
        .route("/api/v1/ai/policy/stats", get(ai_policy_stats))
        // S9-WS8-02: AI policy counter reset
        .route("/api/v1/ai/policy/reset", post(ai_policy_reset))
        // S9-WS8-02: AI governance audit
        .route("/api/v1/ai/governance/audit", get(ai_governance_audit))
        .route("/api/v1/ai/request", post(ai_rate_check))
        // WS4 Ingest endpoints
        .route("/api/v1/ingest/csv", post(ingest_csv))
        .route("/api/v1/ingest/json", post(ingest_json))
        .route("/api/v1/ingest/parquet", post(ingest_parquet))
        .route("/api/v1/ingest/excel", post(ingest_excel))
        .route("/api/v1/ingest/chunked", post(ingest_chunked))
        .route("/api/v1/ingest/status", get(ingest_status))
        // S5-WS4-03: ingest schema registry
        .route("/api/v1/ingest/schema", get(ingest_schema_registry))
        // S5-WS4-03: ingest schema list (format-filtered)
        .route("/api/v1/ingest/schema/list", get(ingest_schema_list))
        // S11-WS1-13: Ingest schema field details
        .route("/api/v1/ingest/schema/fields", get(ingest_schema_fields))
        // S5-WS4-03: ingest format auto-detection
        .route("/api/v1/ingest/format/detect", post(ingest_format_detect))
        // S5-WS4-04: ingest connector configuration validation
        .route("/api/v1/ingest/connector/validate", post(ingest_connector_validate))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))
        .route("/api/v1/ingest/outbox/replay", post(ingest_outbox_replay))
        // REQ-02 DDL catalog endpoint
        .route("/api/v1/catalog/schemas", get(catalog_schemas))
        // REQ-02 DDL catalog table metadata endpoint (columns + indexes)
        .route("/api/v1/catalog/tables/:table_name/columns", get(catalog_table_columns))
        // REQ-23 ACID transaction introspection
        .route("/api/v1/sql/transactions/active", get(sql_transactions_active))
        // S2-WS2-05: isolation stats per active transaction
        .route("/api/v1/sql/transactions/isolation", get(sql_transactions_isolation))
        // REQ-10/19: benchmark endpoints
        .route("/api/v1/benchmark/ingest", post(benchmark_ingest))
        .route("/api/v1/benchmark/query", post(benchmark_query))
        // S6-001: object-scoped history
        .route("/api/v1/history/object", get(history_object))
        // S6-002: dump structure and data
        .route("/api/v1/export/dump", get(export_dump))
        // S6-003: import SQL execution pipeline
        .route("/api/v1/import/sql", post(import_sql))
        // S6-004: server status for IDE panel
        .route("/api/v1/admin/server-status", get(admin_server_status))
        // S6-005: full-text search (feature-gated by VNG_FTS_ENABLED)
        .route("/api/v1/search/fulltext", post(search_fulltext))
        // MCP: Model Context Protocol tool invocation endpoints
        .route("/api/v1/mcp/capabilities", get(mcp_capabilities))
        .route("/api/v1/mcp/invoke", post(mcp_invoke))
        .with_state(state.clone());


    app
}

pub(crate) async fn add_cors(req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next) -> axum::response::Response {
    let mut res = next.run(req).await;
    res.headers_mut().insert(
        "Access-Control-Allow-Origin",
        axum::http::HeaderValue::from_static("*"),
    );
    res.headers_mut().insert(
        "Access-Control-Allow-Methods",
        axum::http::HeaderValue::from_static("GET,POST,OPTIONS"),
    );
    res.headers_mut().insert(
        "Access-Control-Allow-Headers",
        axum::http::HeaderValue::from_static("content-type,x-vng-admin-key,x-vng-operator-id,x-vng-tenant-id,x-vng-user-id"),
    );
    res
}

pub(crate) async fn options_preflight() -> axum::response::Response {
    axum::http::Response::builder()
        .status(axum::http::StatusCode::NO_CONTENT)
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "GET,POST,OPTIONS")
        .header(
            "Access-Control-Allow-Headers",
            "content-type,x-vng-admin-key,x-vng-operator-id,x-vng-tenant-id,x-vng-user-id",
        )
        .body(axum::body::Body::empty())
        .unwrap()
}

pub(crate) async fn track_http_metrics(req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next) -> axum::response::Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let started = std::time::Instant::now();

    let response = next.run(req).await;

    if path != "/metrics" {
        let status = response.status().as_u16();
        let status_class = match status {
            100..=199 => "1xx",
            200..=299 => "2xx",
            300..=399 => "3xx",
            400..=499 => "4xx",
            500..=599 => "5xx",
            _ => "other",
        };
        let route_label = coarsen_route_for_metrics(&path);

        metrics::counter!(
            "vng_http_requests_total",
            "method" => method.as_str().to_string(),
            "route" => route_label.clone(),
            "status_class" => status_class,
        )
        .increment(1);

        metrics::histogram!(
            "vng_http_request_duration_seconds",
            "method" => method.as_str().to_string(),
            "route" => route_label,
        )
        .record(started.elapsed().as_secs_f64());
    }

    response
}

fn coarsen_route_for_metrics(path: &str) -> String {
    for (prefix, replacement) in &[
        ("/api/v1/admin/databases/", "/api/v1/admin/databases/:name"),
    ] {
        if path.starts_with(prefix) && path.len() > prefix.len() {
            return replacement.to_string();
        }
    }
    path.to_string()
}
