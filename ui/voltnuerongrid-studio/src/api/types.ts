export type RoutePath = "oltp" | "olap" | "hybrid" | "unknown";

export interface SqlExecuteRequest {
  sql_batch: string;
  max_rows?: number;
  /** M-6: ACID isolation level override for this request. */
  isolation_level?: string;
  /** M-6: Client-side statement timeout hint in milliseconds (0 = no limit). */
  statement_timeout_ms?: number;
}

/** M-6: Server runtime configuration shape returned by GET /api/v1/admin/runtime-config */
export interface RuntimeConfig {
  storage: {
    engine: string;
    data_dir: string;
    max_background_jobs: number;
    wal_fsync_on_commit: boolean;
  };
  sql: {
    engine: string;
    htap_olap_threshold_rows: number;
    max_result_rows: number;
  };
}

export interface SqlTransactionResponse {
  status: string;
  transaction_id: string;
  statements_executed: number;
  requires_transaction: boolean;
  touches_catalog: boolean;
  rejected_statement_count: number;
  elapsed_ms: number;
}

export interface OlapQueryResponse {
  status: string;
  query_signature: string;
  elapsed_ms: number;
  rows: number;
}

export interface SqlExecuteResponse {
  status: string;
  route_path: RoutePath;
  reason: string;
  transaction?: SqlTransactionResponse;
  olap?: OlapQueryResponse;
  rejected_statement_count: number;
}

export interface AuthorizeActionRequest {
  action: string;
  scope?: string;
}

export interface AuthorizeActionResponse {
  status: string;
  action: string;
  requested_scope: string;
  decision: string;
  reason: string;
  trace_id: string;
}

export interface AuditEvent {
  event_id: number;
  occurred_epoch_ms: number;
  actor: string;
  action: string;
  kind: string;
  outcome: string;
  details_json: string;
}

export interface AuditEventsResponse {
  status: string;
  total_events: number;
  events: AuditEvent[];
}

export interface AutonomousActionRecord {
  trace_id: string;
  occurred_epoch_ms: number;
  action: string;
  scope: string;
  requested_by: string;
  decision: string;
  reason: string;
}

export interface AutonomousActionRecordsResponse {
  status: string;
  total_records: number;
  records: AutonomousActionRecord[];
}
