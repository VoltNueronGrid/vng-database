export type RoutePath = "oltp" | "olap" | "hybrid" | "unknown";

export interface SqlExecuteRequest {
  sql_batch: string;
  max_rows?: number;
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
