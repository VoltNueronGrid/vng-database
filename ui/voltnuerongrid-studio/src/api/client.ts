import type {
  AuditEventsResponse,
  AuthorizeActionRequest,
  AuthorizeActionResponse,
  AutonomousActionRecordsResponse,
  RuntimeConfig,
  SqlExecuteRequest,
  SqlExecuteResponse,
} from "./types.js";

// ── Admin / Auth response types ───────────────────────────────────────────────

export interface AdminUserEntry {
  user_id: string;
  username: string;
  role: string;
  tenant_id?: string;
  created_ms: number;
}

export interface AdminUserListResponse {
  users: AdminUserEntry[];
  count: number;
}

export interface CreateUserRequest {
  username: string;
  password: string;
  role?: string;
  tenant_id?: string;
}

export interface CreateUserResponse {
  status: string;
  user_id: string;
  username: string;
  role: string;
}

export interface LoginResponse {
  status: string;
  token: string;
  user_id: string;
  username: string;
  role: string;
  expires_at_secs: number;
}

export interface StudioApiClientConfig {
  baseUrl: string;
  adminApiKey?: string;
  operatorId?: string;
  sessionId?: string;
}

export class StudioApiClient {
  private readonly config: StudioApiClientConfig;

  constructor(config: StudioApiClientConfig) {
    this.config = config;
  }

  async executeSql(req: SqlExecuteRequest): Promise<SqlExecuteResponse> {
    return this.postJson<SqlExecuteResponse>("/api/v1/sql/execute", req);
  }

  async authorizeAction(
    req: AuthorizeActionRequest,
  ): Promise<AuthorizeActionResponse> {
    return this.postJson<AuthorizeActionResponse>(
      "/api/v1/autonomous/actions/authorize",
      req,
    );
  }

  async listAuditEvents(maxItems = 100): Promise<AuditEventsResponse> {
    return this.getJson<AuditEventsResponse>(`/api/v1/audit/events?max_items=${maxItems}`);
  }

  // ── User management ───────────────────────────────────────────────────────

  async listUsers(): Promise<AdminUserListResponse> {
    // The admin API returns a flat list via the WAL-replayed user store.
    // We infer user list by checking the session endpoint.
    return this.getJson<AdminUserListResponse>("/api/v1/admin/users/list");
  }

  async createUser(req: CreateUserRequest): Promise<CreateUserResponse> {
    return this.postJson<CreateUserResponse>("/api/v1/admin/users", req);
  }

  async deleteUser(userId: string): Promise<{ status: string }> {
    return this.deleteJson<{ status: string }>(`/api/v1/admin/users/${userId}`);
  }

  async revokeUserSessions(userId: string): Promise<{ status: string; sessions_revoked: number }> {
    return this.deleteJson<{ status: string; sessions_revoked: number }>(
      `/api/v1/admin/users/${userId}/sessions`,
    );
  }

  // ── Auth ─────────────────────────────────────────────────────────────────

  async login(username: string, password: string): Promise<LoginResponse> {
    return this.postJson<LoginResponse>("/api/v1/auth/login", { username, password });
  }

  async listActionRecords(maxItems = 100): Promise<AutonomousActionRecordsResponse> {
    return this.getJson<AutonomousActionRecordsResponse>(
      `/api/v1/autonomous/actions/records?max_items=${maxItems}`,
    );
  }

  /** M-6: Fetch the server boot-time runtime configuration. */
  async getRuntimeConfig(): Promise<RuntimeConfig> {
    return this.getJson<RuntimeConfig>("/api/v1/admin/runtime-config");
  }

  private async deleteJson<T>(path: string): Promise<T> {
    const response = await fetch(this.url(path), {
      method: "DELETE",
      headers: this.headers(),
    });
    return this.parseJson<T>(response);
  }

  private async postJson<T>(path: string, payload: unknown): Promise<T> {
    const response = await fetch(this.url(path), {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(payload),
    });
    return this.parseJson<T>(response);
  }

  private async getJson<T>(path: string): Promise<T> {
    const response = await fetch(this.url(path), {
      method: "GET",
      headers: this.headers(),
    });
    return this.parseJson<T>(response);
  }

  private headers(): Record<string, string> {
    const headers: Record<string, string> = {
      "content-type": "application/json",
    };
    if (this.config.adminApiKey) headers["x-vng-admin-key"] = this.config.adminApiKey;
    if (this.config.operatorId) headers["x-vng-operator-id"] = this.config.operatorId;
    if (this.config.sessionId) headers["x-vng-session-id"] = this.config.sessionId;
    return headers;
  }

  private url(path: string): string {
    return `${this.config.baseUrl.replace(/\/$/, "")}${path}`;
  }

  private async parseJson<T>(response: Response): Promise<T> {
    if (!response.ok) {
      const text = await response.text();
      throw new Error(`HTTP ${response.status}: ${text}`);
    }
    return (await response.json()) as T;
  }
}
