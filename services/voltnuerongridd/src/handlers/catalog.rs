use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Serialize;
use std::collections::BTreeMap;
use voltnuerongrid_auth::PrivilegeAction;
use voltnuerongrid_sql::ast::Statement;
use voltnuerongrid_store::ddl_catalog::DdlCatalogEntry;
use crate::{AppState, AuthErrorResponse};
use crate::auth::{require_sql_runtime_principal, require_admin_api_key};

// ─── Catalog DTOs ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct CatalogEntryView {
    pub(crate) object_name: String,
    pub(crate) object_kind: String,
    pub(crate) created_at_unix_ms: u128,
    pub(crate) last_altered_at_unix_ms: Option<u128>,
    pub(crate) alteration_count: u32,
}

#[derive(Serialize)]
pub(crate) struct CatalogSchemasResponse {
    pub(crate) status: &'static str,
    pub(crate) active_count: usize,
    pub(crate) total_count: usize,
    pub(crate) entries: Vec<CatalogEntryView>,
}

#[derive(Serialize)]
pub(crate) struct CatalogTableColumnView {
    pub(crate) name: String,
    pub(crate) data_type: String,
    pub(crate) nullable: bool,
    pub(crate) primary_key: bool,
}

#[derive(Serialize)]
pub(crate) struct CatalogTableIndexView {
    pub(crate) name: String,
    pub(crate) columns: Vec<String>,
    pub(crate) unique: bool,
}

#[derive(Serialize)]
pub(crate) struct CatalogTableColumnsResponse {
    pub(crate) status: &'static str,
    pub(crate) table_name: String,
    pub(crate) columns: Vec<CatalogTableColumnView>,
    pub(crate) indexes: Vec<CatalogTableIndexView>,
}

// ─── Admin schema tree DTOs ────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct AdminSchemaColumn {
    pub(crate) name: String,
    pub(crate) data_type: String,
    pub(crate) nullable: bool,
    pub(crate) primary_key: bool,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaIndex {
    pub(crate) name: String,
    pub(crate) columns: Vec<String>,
    pub(crate) unique: bool,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaTable {
    pub(crate) name: String,
    pub(crate) schema: String,
    pub(crate) columns: Vec<AdminSchemaColumn>,
    pub(crate) indexes: Vec<AdminSchemaIndex>,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaFunction {
    pub(crate) name: String,
    pub(crate) schema: String,
    /// Raw DDL body stored at CREATE FUNCTION time.
    pub(crate) definition: String,
    /// Argument list extracted from the first `(…)` in the DDL, or empty string.
    pub(crate) arguments: String,
    /// Return type extracted from RETURNS clause, or "void".
    pub(crate) return_type: String,
    /// Language tag, e.g. "sql", "plpgsql", "rust".
    pub(crate) language: String,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaView {
    pub(crate) name: String,
    pub(crate) schema: String,
    pub(crate) definition: String,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaTrigger {
    pub(crate) name: String,
    pub(crate) schema: String,
    pub(crate) table: String,
    pub(crate) definition: String,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaEvent {
    pub(crate) name: String,
    pub(crate) schema: String,
    pub(crate) schedule: String,
    pub(crate) definition: String,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaSchemaEntry {
    pub(crate) name: String,
    pub(crate) database: String,
    pub(crate) tables: Vec<AdminSchemaTable>,
    pub(crate) views: Vec<AdminSchemaView>,
    pub(crate) functions: Vec<AdminSchemaFunction>,
    pub(crate) triggers: Vec<AdminSchemaTrigger>,
    pub(crate) events: Vec<AdminSchemaEvent>,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaDatabase {
    pub(crate) name: String,
    pub(crate) schemas: Vec<AdminSchemaSchemaEntry>,
}

#[derive(Serialize)]
pub(crate) struct AdminSchemaTreeResponse {
    pub(crate) databases: Vec<AdminSchemaDatabase>,
    pub(crate) timestamp: u128,
}

// ─── Catalog handlers ──────────────────────────────────────────────────────────

pub(crate) async fn catalog_schemas(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<CatalogSchemasResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_sql_runtime_principal(&headers, &state, PrivilegeAction::Read, "sql/catalog/schemas")?;
    let catalog = state.ddl_catalog.lock().expect("ddl_catalog lock");
    let active = catalog.active_entries();
    let entries: Vec<CatalogEntryView> = active
        .iter()
        .map(|e| CatalogEntryView {
            object_name: e.object_name.clone(),
            object_kind: e.object_kind.clone(),
            created_at_unix_ms: e.created_at_unix_ms,
            last_altered_at_unix_ms: e.last_altered_at_unix_ms,
            alteration_count: e.alteration_count,
        })
        .collect();
    let resp = CatalogSchemasResponse {
        status: "ok",
        active_count: catalog.active_count(),
        total_count: catalog.total_count(),
        entries,
    };
    Ok((StatusCode::OK, Json(resp)))
}

pub(crate) async fn catalog_table_columns(
    State(state): State<AppState>,
    Path(table_name): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<CatalogTableColumnsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_sql_runtime_principal(&headers, &state, PrivilegeAction::Read, "sql/catalog/schemas")?;

    let normalized_name = table_name.trim().to_ascii_lowercase();
    let entry = {
        let catalog = state.ddl_catalog.lock().expect("ddl_catalog lock");
        catalog.get(&normalized_name).cloned()
    };

    let Some(entry) = entry else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(CatalogTableColumnsResponse {
                status: "not_found",
                table_name,
                columns: Vec::new(),
                indexes: Vec::new(),
            }),
        ));
    };

    if entry.dropped {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(CatalogTableColumnsResponse {
                status: "not_found",
                table_name: entry.object_name,
                columns: Vec::new(),
                indexes: Vec::new(),
            }),
        ));
    }

    let columns = match voltnuerongrid_sql::parse_one(&entry.original_statement) {
        Ok(Statement::CreateTable(stmt)) => stmt
            .columns
            .iter()
            .map(|column| CatalogTableColumnView {
                name: column.name.clone(),
                data_type: column.data_type.clone(),
                nullable: true,
                primary_key: false,
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    let indexes = {
        let mgr = state.index_manager.lock().expect("index lock");
        mgr.list_indexes()
            .iter()
            .filter(|idx| idx.table.eq_ignore_ascii_case(&entry.object_name))
            .map(|idx| CatalogTableIndexView {
                name: idx.name.clone(),
                columns: vec![idx.column.clone()],
                unique: idx.unique,
            })
            .collect::<Vec<_>>()
    };

    Ok((
        StatusCode::OK,
        Json(CatalogTableColumnsResponse {
            status: "ok",
            table_name: entry.object_name,
            columns,
            indexes,
        }),
    ))
}

pub(crate) async fn admin_schema_tree(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AdminSchemaTreeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;

    let catalog = state.ddl_catalog.lock().expect("ddl_catalog lock");
    let index_mgr = state.index_manager.lock().expect("index lock");

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let mut db_map: BTreeMap<String, BTreeMap<String, Vec<&DdlCatalogEntry>>> = BTreeMap::new();
    for entry in catalog.active_entries() {
        db_map
            .entry(entry.database_name.clone())
            .or_default()
            .entry(entry.schema_name.clone())
            .or_default()
            .push(entry);
    }

    let databases: Vec<AdminSchemaDatabase> = db_map
        .into_iter()
        .map(|(db_name, schema_map)| {
            let schemas: Vec<AdminSchemaSchemaEntry> = schema_map
                .into_iter()
                .map(|(schema_name, entries)| {
                    let mut tables: Vec<AdminSchemaTable> = entries
                        .iter()
                        .filter(|e| e.object_kind == "table")
                        .map(|e| {
                            let columns = match voltnuerongrid_sql::parse_one(&e.original_statement) {
                                Ok(Statement::CreateTable(stmt)) => stmt
                                    .columns
                                    .iter()
                                    .map(|c| AdminSchemaColumn {
                                        name: c.name.clone(),
                                        data_type: c.data_type.clone(),
                                        nullable: true,
                                        primary_key: false,
                                    })
                                    .collect(),
                                _ => Vec::new(),
                            };
                            let indexes = index_mgr
                                .list_indexes()
                                .iter()
                                .filter(|idx| idx.table.eq_ignore_ascii_case(&e.object_name))
                                .map(|idx| AdminSchemaIndex {
                                    name: idx.name.clone(),
                                    columns: vec![idx.column.clone()],
                                    unique: idx.unique,
                                })
                                .collect();
                            AdminSchemaTable {
                                name: e.object_name.clone(),
                                schema: schema_name.clone(),
                                columns,
                                indexes,
                            }
                        })
                        .collect();
                    tables.sort_by(|a, b| a.name.cmp(&b.name));

                    let mut views: Vec<AdminSchemaView> = entries
                        .iter()
                        .filter(|e| e.object_kind == "view" || e.object_kind == "materialized_view")
                        .map(|e| AdminSchemaView {
                            name: e.object_name.clone(),
                            schema: schema_name.clone(),
                            definition: e.original_statement.clone(),
                        })
                        .collect();
                    views.sort_by(|a, b| a.name.cmp(&b.name));

                    let mut functions: Vec<AdminSchemaFunction> = entries
                        .iter()
                        .filter(|e| e.object_kind == "function")
                        .map(|e| {
                            let def = &e.original_statement;
                            let arguments = def
                                .find('(')
                                .and_then(|start| {
                                    def[start + 1..].find(')').map(|end| {
                                        def[start + 1..start + 1 + end].trim().to_string()
                                    })
                                })
                                .unwrap_or_default();
                            let return_type = {
                                let upper = def.to_ascii_uppercase();
                                upper
                                    .find("RETURNS")
                                    .and_then(|pos| {
                                        def[pos + 7..].split_whitespace().next().map(|s| {
                                            s.trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
                                                .to_string()
                                        })
                                    })
                                    .unwrap_or_else(|| "void".to_string())
                            };
                            let language = {
                                let upper = def.to_ascii_uppercase();
                                upper
                                    .find("LANGUAGE")
                                    .and_then(|pos| {
                                        def[pos + 8..]
                                            .split_whitespace()
                                            .next()
                                            .map(|s| s.trim().to_ascii_lowercase())
                                    })
                                    .unwrap_or_else(|| "sql".to_string())
                            };
                            AdminSchemaFunction {
                                name: e.object_name.clone(),
                                schema: schema_name.clone(),
                                definition: def.clone(),
                                arguments,
                                return_type,
                                language,
                            }
                        })
                        .collect();
                    functions.sort_by(|a, b| a.name.cmp(&b.name));

                    let mut triggers: Vec<AdminSchemaTrigger> = entries
                        .iter()
                        .filter(|e| e.object_kind == "trigger")
                        .map(|e| AdminSchemaTrigger {
                            name: e.object_name.clone(),
                            schema: schema_name.clone(),
                            table: extract_trigger_table_name(&e.original_statement),
                            definition: e.original_statement.clone(),
                        })
                        .collect();
                    triggers.sort_by(|a, b| a.name.cmp(&b.name));

                    let mut events: Vec<AdminSchemaEvent> = entries
                        .iter()
                        .filter(|e| e.object_kind == "event")
                        .map(|e| AdminSchemaEvent {
                            name: e.object_name.clone(),
                            schema: schema_name.clone(),
                            schedule: extract_event_schedule(&e.original_statement),
                            definition: e.original_statement.clone(),
                        })
                        .collect();
                    events.sort_by(|a, b| a.name.cmp(&b.name));

                    AdminSchemaSchemaEntry {
                        name: schema_name.clone(),
                        database: db_name.clone(),
                        tables,
                        views,
                        functions,
                        triggers,
                        events,
                    }
                })
                .collect();
            AdminSchemaDatabase {
                name: db_name,
                schemas,
            }
        })
        .collect();

    Ok((StatusCode::OK, Json(AdminSchemaTreeResponse { databases, timestamp: now_ms })))
}

fn extract_trigger_table_name(ddl: &str) -> String {
    let tokens: Vec<&str> = ddl.split_whitespace().collect();
    for pair in tokens.windows(2) {
        if pair[0].eq_ignore_ascii_case("ON") {
            return pair[1]
                .trim_matches(|c: char| c == ';' || c == '(' || c == ')')
                .rsplit('.')
                .next()
                .unwrap_or(pair[1])
                .to_string();
        }
    }
    String::new()
}

fn extract_event_schedule(ddl: &str) -> String {
    let upper = ddl.to_ascii_uppercase();
    if let Some(start) = upper.find("ON SCHEDULE") {
        let rest = ddl[start + "ON SCHEDULE".len()..].trim();
        if let Some(end) = rest.to_ascii_uppercase().find(" DO ") {
            return rest[..end].trim().trim_end_matches(';').to_string();
        }
        return rest.trim().trim_end_matches(';').to_string();
    }
    String::new()
}
