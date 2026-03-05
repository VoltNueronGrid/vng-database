import type {
  AuditEventsResponse,
  AuthorizeActionRequest,
  AuthorizeActionResponse,
  AutonomousActionRecordsResponse,
  SqlExecuteRequest,
  SqlExecuteResponse,
} from "./types.js";

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

  async listActionRecords(maxItems = 100): Promise<AutonomousActionRecordsResponse> {
    return this.getJson<AutonomousActionRecordsResponse>(
      `/api/v1/autonomous/actions/records?max_items=${maxItems}`,
    );
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
