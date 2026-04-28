// Studio HTTP client — wraps voltnuerongridd REST API.
// In browser dev mode requests use relative paths (routed via Vite proxy).
// In production Tauri the configured baseUrl is used directly.

export interface StudioConnection {
  baseUrl: string;
  adminApiKey?: string;
  operatorId?: string;
  tenantId?: string;
  userId?: string;
}

// ─── SQL Execute ─────────────────────────────────────────────────────────────

export interface SqlExecuteRequest {
  sql_batch: string; // single SQL string — server deserialises as String
  max_rows?: number;
}

export type RoutePath = "oltp" | "olap" | "hybrid" | "unknown";

export interface SqlTransactionResult {
  status: string;
  transaction_id: string;
  statements_executed: number;
  requires_transaction: boolean;
  touches_catalog: boolean;
  rejected_statement_count: number;
  elapsed_ms: number;
}

export interface OlapQueryResult {
  status: string;
  query_signature: string;
  elapsed_ms: number;
  rows: number;
}

export interface SqlExecuteResponse {
  status: string;
  route_path: RoutePath;
  reason: string;
  rejected_statement_count: number;
  transaction?: SqlTransactionResult;
  olap?: OlapQueryResult;
  // Future: server will populate these when row-return is implemented
  columns?: Array<{ name: string; data_type: string }>;
  rows?: Array<Record<string, unknown>>;
}

// ─── Health ──────────────────────────────────────────────────────────────────

export interface HealthResponse {
  status: string;
  node_id?: string;
  cluster_mode?: string;
}

// ─── Schema Tree  (/api/v1/admin/schema/tree) ─────────────────────────────────

export interface SchemaColumn {
  name: string;
  data_type: string;   // server field name
  nullable: boolean;
  primary_key: boolean; // server field name
}

export interface SchemaIndex {
  name: string;
  columns: string[];
  unique: boolean;
}

export interface SchemaTable {
  name: string;
  schema: string;
  columns: SchemaColumn[];
  indexes?: SchemaIndex[];
  row_count?: number; // not returned by server yet; reserved for future
}

export interface SchemaNamespace {
  name: string;
  database: string;
  tables: SchemaTable[];
}

export interface SchemaDatabase {
  name: string;
  schemas: SchemaNamespace[];
}

export interface SchemaRegistry {
  databases: SchemaDatabase[];
  timestamp?: number;
}

// ─── Audit ───────────────────────────────────────────────────────────────────

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

// ─── Cluster Topology ────────────────────────────────────────────────────────

export interface ClusterTopologyResponse {
  leader_node_id: string;
  total_nodes: number;
  active_nodes: number;
  passive_nodes: number;
  dead_nodes: number;
  active_sessions: number;
  passive_sessions: number;
  live_transactions: number;
  total_transactions: number;
  live_locks: number;
  nodes: ClusterNode[];
}

export interface ClusterNode {
  node_id: string;
  role: string;
  status: string;
  total_cpu_cores: number;
  total_ram_mb: number;
  used_cpu_pct: number;
  used_ram_mb: number;
  active_sessions: number;
  live_transactions: number;
  total_transactions: number;
  live_locks: number;
  draining: boolean;
}

// ─── Client ──────────────────────────────────────────────────────────────────

export class StudioApiClient {
  private readonly conn: StudioConnection;

  constructor(conn: StudioConnection) {
    this.conn = conn;
  }

  async health(): Promise<HealthResponse> {
    return this.get<HealthResponse>("/health");
  }

  async executeSql(req: SqlExecuteRequest): Promise<SqlExecuteResponse> {
    return this.post<SqlExecuteResponse>("/api/v1/sql/execute", req);
  }

  async getSchemaTree(): Promise<SchemaRegistry> {
    return this.get<SchemaRegistry>("/api/v1/admin/schema/tree");
  }

  async getAuditEvents(maxItems = 100): Promise<AuditEventsResponse> {
    return this.get<AuditEventsResponse>(
      `/api/v1/audit/events?max_items=${maxItems}`
    );
  }

  async getClusterTopology(): Promise<ClusterTopologyResponse> {
    return this.get<ClusterTopologyResponse>("/api/v1/admin/cluster/topology");
  }

  // ── Private helpers ────────────────────────────────────────────────────────

  private baseUrl(): string {
    // In browser dev mode, use relative paths so Vite proxy handles CORS.
    // In Tauri (production) and when TAURI_INTERNALS is present, use the full URL.
    const isTauriRuntime =
      typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    const isDevBrowser = import.meta.env.DEV && !isTauriRuntime;
    return isDevBrowser ? "" : this.conn.baseUrl.replace(/\/$/, "");
  }

  private headers(): Record<string, string> {
    const h: Record<string, string> = { "content-type": "application/json" };
    if (this.conn.adminApiKey) {
      h["x-vng-admin-key"] = this.conn.adminApiKey;
      // Server runtime endpoints require operator identity alongside admin key.
      // Default to "admin" which maps to OperatorRole::Dba in default_operator_role_bindings.
      h["x-vng-operator-id"] = this.conn.operatorId ?? "admin";
    } else if (this.conn.operatorId) {
      h["x-vng-operator-id"] = this.conn.operatorId;
    }
    if (this.conn.tenantId) h["x-vng-tenant-id"] = this.conn.tenantId;
    if (this.conn.userId) h["x-vng-user-id"] = this.conn.userId;
    return h;
  }

  private async get<T>(path: string): Promise<T> {
    const res = await fetch(`${this.baseUrl()}${path}`, {
      method: "GET",
      headers: this.headers(),
    });
    return this.parse<T>(res);
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const res = await fetch(`${this.baseUrl()}${path}`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(body),
    });
    return this.parse<T>(res);
  }

  private async parse<T>(res: Response): Promise<T> {
    if (!res.ok) {
      const text = await res.text();
      // Try to extract a human-readable reason from the JSON error body
      try {
        const json = JSON.parse(text) as { reason?: string; status?: string };
        if (json.reason) {
          throw new Error(`${json.reason}`);
        }
      } catch (parseErr) {
        if (parseErr instanceof Error && parseErr.message !== text) {
          throw parseErr;
        }
      }
      throw new Error(`HTTP ${res.status}: ${text}`);
    }
    return (await res.json()) as T;
  }
}
