//! Boot-time WAL replay, durability engine init, RBAC defaults.
use crate::AppState;


/// Write a SQL statement to the durability engine (RocksDB or in-memory).
/// The engine is the single source of truth from Phase 2.2 onward.
pub(crate) fn persist_sql_statement(
    state: &AppState,
    kind: voltnuerongrid_store::SqlWalKind,
    sql: &str,
) {
    if let Ok(mut wal) = state.wal_engine.lock() {
        let _ = wal.append_sql(kind, sql);
    } else {
        tracing::error!(target: "vng.wal", "wal_engine mutex poisoned in persist_sql_statement");
    }
    metrics::counter!(
        "vng_wal_append_total",
        "kind" => kind.as_str(),
    ).increment(1);
}


/// Phase 2 — pick the durability engine based on `runtime_config.storage`.
///
/// Selection rules:
/// - `StorageEngine::Rocksdb` (default) → open RocksDB at the configured
///   `data_dir`. The `wal_fsync_on_commit` flag is propagated via the
///   `VNG_WAL_FSYNC_ON_COMMIT` env var (the engine reads it directly to
///   keep its open() signature stable across feature flags).
///   On failure to open RocksDB the process **exits**, not falls back to
///   in-memory. Silently degrading durability would defeat the whole
///   point of Phase 2; an obvious crash is preferred.
/// - `StorageEngine::Vng` → currently mapped to in-memory with a warning,
///   because the native VNG engine is not yet shipped.
pub(crate) fn build_durability_engine(
    cfg: &voltnuerongrid_config::RuntimeConfig,
) -> BoxedDurabilityEngine {
    use voltnuerongrid_config::StorageEngine;

    let durability_cfg = DurabilityConfig {
        wal_enabled: true,
        checkpoint_interval_seconds: 60,
        max_wal_records_before_checkpoint: 1_000,
    };

    match cfg.storage.engine {
        StorageEngine::Rocksdb => {
            // Propagate the fsync flag to the rocksdb engine via env var so
            // its open() signature can stay simple (the engine reads it once
            // at boot — main.rs sets it before construction).
            std::env::set_var(
                "VNG_WAL_FSYNC_ON_COMMIT",
                if cfg.storage.wal_fsync_on_commit { "1" } else { "0" },
            );
            let path = std::path::PathBuf::from(&cfg.storage.data_dir).join("rocksdb");
            tracing::info!(
                target: "vng.durability",
                path = %path.display(),
                fsync = cfg.storage.wal_fsync_on_commit,
                "opening RocksDB durability engine"
            );
            match BoxedDurabilityEngine::rocksdb(&path, durability_cfg) {
                Ok(engine) => {
                    tracing::info!(
                        target: "vng.durability",
                        kind = engine.engine_kind(),
                        latest_sequence = engine.latest_sequence(),
                        checkpoint_count = engine.checkpoint_count(),
                        "durability engine opened"
                    );
                    engine
                }
                Err(e) => {
                    eprintln!(
                        "[vng-durability] FATAL: failed to open RocksDB at {}: {}",
                        path.display(),
                        e
                    );
                    eprintln!(
                        "[vng-durability] refusing to fall back to in-memory — \
                         silently dropping durability would mask data loss. \
                         Fix the path or set storage.engine = vng to opt out."
                    );
                    std::process::exit(2);
                }
            }
        }
        StorageEngine::Vng => {
            tracing::warn!(
                target: "vng.durability",
                "storage.engine = vng — native VNG engine is not yet implemented; \
                 falling back to non-durable in-memory engine. Set \
                 storage.engine = rocksdb for production durability."
            );
            BoxedDurabilityEngine::in_memory(durability_cfg)
        }
    }
}


// ─── Phase 2.1: engine-first replay helpers ─────────────────────────────────
//
// Boot replay precedence:
// 1. If the durability engine persists SQL streams (RocksDB) AND has any
//    statements in the requested kind, drive replay from there.
// 2. Otherwise, fall back to the legacy text WAL files. The first successful
//    engine-backed replay (after migration) lets the operator delete the
//    text files.
//
// The engine-first path is the reason for the SqlWalKind extension to the
// trait. Once all deployments have migrated, the legacy path can be removed.

/// Replay DDL into a freshly-created catalog from the durability engine.
pub(crate) fn replay_ddl_into(
    catalog: &mut DdlCatalog,
    engine: &Arc<Mutex<voltnuerongrid_store::BoxedDurabilityEngine>>,
) {
    use voltnuerongrid_store::SqlWalKind;
    let now_ms = now_unix_ms();

    let stmts: Vec<String> = {
        let guard = engine.lock().expect("wal_engine lock for replay_ddl");
        if guard.persists_sql() && guard.sql_count(SqlWalKind::Ddl) > 0 {
            guard.iter_sql(SqlWalKind::Ddl)
        } else {
            Vec::new()
        }
    };
    for sql in &stmts {
        apply_ddl_to_catalog(catalog, sql, now_ms);
    }
    if !stmts.is_empty() {
        eprintln!("[vng-wal] replayed {} DDL statement(s) from durability engine", stmts.len());
        metrics::counter!(
            "vng_wal_replay_total",
            "kind" => "ddl",
            "source" => "engine",
        ).increment(stmts.len() as u64);
    }
}


/// Replay DML into a freshly-created row store from the durability engine.
pub(crate) fn replay_dml_into(
    rs: &mut PagedRowStore,
    engine: &Arc<Mutex<voltnuerongrid_store::BoxedDurabilityEngine>>,
) {
    use voltnuerongrid_store::SqlWalKind;

    let stmts: Vec<String> = {
        let guard = engine.lock().expect("wal_engine lock for replay_dml");
        if guard.persists_sql() && guard.sql_count(SqlWalKind::Dml) > 0 {
            guard.iter_sql(SqlWalKind::Dml)
        } else {
            Vec::new()
        }
    };
    if stmts.is_empty() {
        return;
    }
    let xid = rs.begin_xid();
    for sql in &stmts {
        apply_dml_to_rowstore(rs, xid, sql);
    }
    eprintln!("[vng-wal] replayed {} DML statement(s) from durability engine", stmts.len());
    metrics::counter!(
        "vng_wal_replay_total",
        "kind" => "dml",
        "source" => "engine",
    ).increment(stmts.len() as u64);
}


/// Apply a single DDL statement to the catalog.
pub(crate) fn apply_ddl_to_catalog(catalog: &mut DdlCatalog, sql: &str, now_ms: u128) {
    if let Some(info) = parse_ddl_info(sql) {
        match info.operation {
            "create" => { let _ = catalog.record_create(&info.object_kind, &info.database_name, &info.schema_name, &info.object_name, sql, now_ms, info.replace_ok); }
            "drop"   => { catalog.record_drop(&info.database_name, &info.schema_name, &info.object_name); }
            "alter"  => { catalog.record_alter(&info.database_name, &info.schema_name, &info.object_name, sql, now_ms); }
            _ => {}
        }
    }
}


/// Apply a single DML statement to the row store.
pub(crate) fn apply_dml_to_rowstore(rs: &mut PagedRowStore, xid: voltnuerongrid_store::mvcc::Xid, sql: &str) {
    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.starts_with("INSERT") {
        for (k, d, _) in extract_all_insert_rows(sql) {
            rs.insert(xid, &k, d);
        }
    } else if upper.starts_with("DELETE") {
        if let Some(k) = extract_delete_key_from_sql(sql) {
            rs.delete(xid, &k);
        }
    } else if upper.starts_with("UPDATE") {
        if let Some((k, d)) = extract_update_row_from_sql(sql) {
            rs.insert(xid, &k, d);
        }
    }
}


pub(crate) fn default_rbac_privilege_matrix() -> RbacPrivilegeMatrix {
    let mut matrix = RbacPrivilegeMatrix::new();

    for role in [OperatorRole::Dba] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "sql.runtime".to_string(),
                scopes: vec!["sql/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.failover".to_string(),
                scopes: vec!["cluster".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Execute,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.sre".to_string(),
                scopes: vec!["sre/*".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Execute,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.dr_hooks".to_string(),
                scopes: vec!["dr_hooks/*".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Execute,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "storage.catalog".to_string(),
                scopes: vec!["store/*".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Write,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "ingest.connectors".to_string(),
                scopes: vec!["ingest/*".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Write,
                    PrivilegeAction::Manage,
                ],
            },
        );
    }

    for role in ["tenant_analyst", "tenant_admin"] {
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "sql.runtime".to_string(),
                scopes: vec!["tenants/{tenant}/sql/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "ingest.connectors".to_string(),
                scopes: vec!["tenants/{tenant}/ingest/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Write],
            },
        );
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "storage.catalog".to_string(),
                scopes: vec![
                    "tenants/{tenant}/store/indexes".to_string(),
                    "tenants/{tenant}/store/indexes/lookup".to_string(),
                    "tenants/{tenant}/store/constraints/validate".to_string(),
                ],
                actions: vec![PrivilegeAction::Read],
            },
        );
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "observability.audit".to_string(),
                scopes: vec!["tenants/{tenant}/audit/events".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "observability.autonomous_records".to_string(),
                scopes: vec!["tenants/{tenant}/autonomous/records".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    matrix.grant_role(
        "tenant_admin",
        ResourceGrant {
            resource: "storage.catalog".to_string(),
            scopes: vec![
                "tenants/{tenant}/store/indexes".to_string(),
                "tenants/{tenant}/store/constraints".to_string(),
            ],
            actions: vec![PrivilegeAction::Manage],
        },
    );

    for role in [OperatorRole::Dba, OperatorRole::Sre] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "observability.audit".to_string(),
                scopes: vec!["audit/*".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Sre, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.sre".to_string(),
                scopes: vec!["sre/reliability", "sre/failure_budget", "sre/gate"].into_iter().map(String::from).collect(),
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Sre] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.failover".to_string(),
                scopes: vec!["cluster".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.dr_hooks".to_string(),
                scopes: vec!["dr_hooks/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.sre".to_string(),
                scopes: vec!["sre/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Security, OperatorRole::AiOperator] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "autonomous.guardrails".to_string(),
                scopes: vec!["autonomous/*".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "autonomous.guardrails".to_string(),
                scopes: vec!["autonomous/emergency_stop".to_string()],
                actions: vec![PrivilegeAction::Manage],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::AiOperator] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "autonomous.actions".to_string(),
                scopes: vec!["autonomous/actions".to_string()],
                actions: vec![PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "observability.autonomous_records".to_string(),
                scopes: vec!["autonomous/records".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "observability.audit".to_string(),
                scopes: vec!["audit/*".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "security.kms".to_string(),
                scopes: vec![
                    "security/kms".to_string(),
                    "security/kms/outage".to_string(),
                    "security/tls/status".to_string(),
                    "security/tls/rotate".to_string(),
                    "security/tde/status".to_string(),
                    "security/tde/toggle".to_string(),
                ],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "security.supply_chain".to_string(),
                scopes: vec!["security/plugins/provenance/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Manage],
            },
        );
    }

    matrix.grant_role(
        OperatorRole::Sre.as_str(),
        ResourceGrant {
            resource: "security.kms".to_string(),
            scopes: vec![
                "security/kms".to_string(),
                "security/tls/status".to_string(),
                "security/tde/status".to_string(),
            ],
            actions: vec![PrivilegeAction::Read],
        },
    );

    // S9-WS8-02: AI model gateway policy enforcement.
    for role in [OperatorRole::Dba, OperatorRole::Security, OperatorRole::AiOperator] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "ai.governance".to_string(),
                scopes: vec!["ai/policy".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }
    for role in [OperatorRole::Dba, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "ai.governance".to_string(),
                scopes: vec!["ai/policy".to_string()],
                actions: vec![PrivilegeAction::Manage],
            },
        );
    }

    // S6-WS5-03 / S6-WS5-04: TLS and TDE status endpoints — already covered
    // by security.kms Read grants which we reuse for TLS/TDE status.

    // S9-WS8A-02: Audit export endpoint — accessible to DBA and Security operators.
    for role in [OperatorRole::Dba, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "audit.read".to_string(),
                scopes: vec!["audit/export".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    matrix
}

