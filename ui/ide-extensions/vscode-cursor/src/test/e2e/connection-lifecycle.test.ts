/**
 * E2E scaffold: connection lifecycle integration test
 *
 * Exercises the full flow without a real server:
 *   Create → Connect → Verify (state=verified) → Browse schema → Query → Disconnect
 *
 * Uses lightweight stub objects in place of VSCode APIs and real HTTP calls.
 * Run with:  node --test src/test/e2e/connection-lifecycle.test.ts
 */

import test from "node:test";
import assert from "node:assert/strict";

import { createDefaultConnection, Connection, ConnectionDiagnostic } from "../../models/Connection";
import { SchemaRegistry } from "../../models/Schema";
import { QueryResult } from "../../models/QueryResult";
import { ConnectionManager } from "../../services/ConnectionManager";
import { QueryExecutionService } from "../../services/QueryExecutionService";
import { buildRemediationHint } from "../../services/RemediationHints";

// ---------------------------------------------------------------------------
// Minimal stub for vscode.ExtensionContext (only what ConnectionManager uses)
// ---------------------------------------------------------------------------
function makeContext(initialConnections: unknown[] = []): {
  globalState: {
    data: Map<string, unknown>;
    get<T>(key: string, defaultValue: T): T;
    update(key: string, value: unknown): Promise<void>;
  };
  secrets: {
    store(key: string, value: string): Promise<void>;
    get(key: string): Promise<string | undefined>;
    delete(key: string): Promise<void>;
  };
} {
  const store = new Map<string, unknown>([["vng.connections", initialConnections]]);
  const secrets = new Map<string, string>();

  return {
    globalState: {
      data: store,
      get<T>(key: string, defaultValue: T): T {
        return (store.has(key) ? store.get(key) : defaultValue) as T;
      },
      async update(key: string, value: unknown): Promise<void> {
        store.set(key, value);
      },
    },
    secrets: {
      async store(key: string, value: string): Promise<void> {
        secrets.set(key, value);
      },
      async get(key: string): Promise<string | undefined> {
        return secrets.get(key);
      },
      async delete(key: string): Promise<void> {
        secrets.delete(key);
      },
    },
  };
}

// ---------------------------------------------------------------------------
// Stub HttpClient that simulates a healthy server
// ---------------------------------------------------------------------------
function makeSuccessfulHttpClient() {
  return {
    async testConnection(_connection: Connection): Promise<{ status: number; ok: boolean }> {
      return { status: 200, ok: true };
    },
    async executeQuery(
      _connection: Connection,
      query: string,
      _options?: unknown
    ): Promise<{ status: number; data: unknown[]; headers: Record<string, string> }> {
      if (query.toLowerCase().includes("select 1")) {
        return { status: 200, data: [{ value: 1 }], headers: {} };
      }
      return { status: 200, data: [{ result: "ok" }], headers: {} };
    },
  };
}

// ---------------------------------------------------------------------------
// Stub SchemaManager (getSchemaRegistry only)
// ---------------------------------------------------------------------------
function makeSchemaManagerStub(): { getSchemaRegistry(conn: Connection): Promise<SchemaRegistry> } {
  return {
    async getSchemaRegistry(_conn: Connection): Promise<SchemaRegistry> {
      return {
        timestamp: Date.now(),
        databases: [
          {
            name: "test_db",
            schemas: [
              {
                name: "public",
                database: "test_db",
                tables: [
                  {
                    name: "users",
                    schema: "public",
                    indexes: [],
                    columns: [
                      { name: "id", type: "BIGINT", nullable: false, isPrimaryKey: true, isUnique: true, isForeignKey: false },
                      { name: "name", type: "TEXT", nullable: true, isPrimaryKey: false, isUnique: false, isForeignKey: false },
                    ],
                  },
                ],
              },
            ],
          },
        ],
      };
    },
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test("connection lifecycle: create and activate a connection", async () => {
  const ctx = makeContext();
  const manager = new ConnectionManager(ctx as never);
  await manager.initialize();

  const settings = createDefaultConnection({ name: "Dev Server", host: "127.0.0.1", port: 8080 });
  const conn = await manager.addConnection(settings);

  assert.equal(conn.settings.name, "Dev Server");
  assert.equal(conn.isActive, false);
  assert.equal(conn.isConnected, false);
  assert.equal(conn.diagnostic.state, "unverified");

  const activated = await manager.setActiveConnection(conn.id);
  assert.ok(activated);
  assert.equal(activated!.isActive, true);
});

test("connection lifecycle: connect transitions state to verified", async () => {
  const ctx = makeContext();
  const manager = new ConnectionManager(ctx as never);
  await manager.initialize();

  const settings = createDefaultConnection({ name: "Dev Server" });
  const conn = await manager.addConnection(settings);
  await manager.setActiveConnection(conn.id);

  // Simulate a successful probe
  const httpClient = makeSuccessfulHttpClient();
  const probeResult = await httpClient.testConnection(conn);

  assert.equal(probeResult.status, 200);
  manager.setConnectionStatus(conn.id, probeResult.ok, `HTTP ${probeResult.status}`);

  const updated = manager.getConnection(conn.id)!;
  assert.equal(updated.isConnected, true);
  assert.equal(updated.diagnostic.state, "verified");
  assert.equal(updated.diagnostic.message, "HTTP 200");
  assert.ok(updated.diagnostic.lastChecked !== undefined);
});

test("connection lifecycle: second probe failure after verified → degraded", async () => {
  const ctx = makeContext();
  const manager = new ConnectionManager(ctx as never);
  await manager.initialize();

  const settings = createDefaultConnection({ name: "Dev Server" });
  const conn = await manager.addConnection(settings);

  // First: verified
  manager.setConnectionStatus(conn.id, true, "HTTP 200");
  assert.equal(manager.getConnection(conn.id)!.diagnostic.state, "verified");

  // Second probe fails — should become degraded
  manager.setConnectionStatus(conn.id, false, "TCP connection refused");
  const degraded = manager.getConnection(conn.id)!;
  assert.equal(degraded.isConnected, false);
  assert.equal(degraded.diagnostic.state, "degraded");
  assert.equal(degraded.diagnostic.message, "TCP connection refused");
});

test("connection lifecycle: probe failure without prior verification → error", async () => {
  const ctx = makeContext();
  const manager = new ConnectionManager(ctx as never);
  await manager.initialize();

  const settings = createDefaultConnection({ name: "Bad Server" });
  const conn = await manager.addConnection(settings);

  // Probe fails immediately (never verified)
  manager.setConnectionStatus(conn.id, false, "HTTP 401: auth failed");
  const errConn = manager.getConnection(conn.id)!;
  assert.equal(errConn.diagnostic.state, "error");
  assert.equal(errConn.diagnostic.message, "HTTP 401: auth failed");
});

test("connection lifecycle: setConnectionDiagnostic directly", async () => {
  const ctx = makeContext();
  const manager = new ConnectionManager(ctx as never);
  await manager.initialize();

  const settings = createDefaultConnection({ name: "Dev Server" });
  const conn = await manager.addConnection(settings);

  manager.setConnectionDiagnostic(conn.id, "error", "HTTP 403: forbidden");
  const updated = manager.getConnection(conn.id)!;
  assert.equal(updated.diagnostic.state, "error");
  assert.equal(updated.diagnostic.message, "HTTP 403: forbidden");
});

test("connection lifecycle: browse schema registry", async () => {
  const ctx = makeContext();
  const manager = new ConnectionManager(ctx as never);
  await manager.initialize();

  const settings = createDefaultConnection({ name: "Dev Server" });
  const conn = await manager.addConnection(settings);
  manager.setConnectionStatus(conn.id, true, "HTTP 200");

  const schemaManager = makeSchemaManagerStub();
  const registry = await schemaManager.getSchemaRegistry(conn);

  assert.equal(registry.databases.length, 1);
  assert.equal(registry.databases[0].name, "test_db");
  assert.equal(registry.databases[0].schemas[0].tables[0].name, "users");
});

test("connection lifecycle: execute a query and check result", async () => {
  const ctx = makeContext();
  const manager = new ConnectionManager(ctx as never);
  await manager.initialize();

  const settings = createDefaultConnection({ name: "Dev Server" });
  const conn = await manager.addConnection(settings);
  manager.setConnectionStatus(conn.id, true, "HTTP 200");

  const globalState = {
    get<T>(_key: string, defaultValue: T): T {
      return defaultValue;
    },
    async update() {
      return;
    },
  };

  const httpClient = makeSuccessfulHttpClient();
  const queryService = new QueryExecutionService(httpClient as never, globalState as never);

  const result: QueryResult = await queryService.executeQuery(conn, "select 1;");

  assert.equal(result.status, "success");
  assert.ok(result.rows.length > 0);
});

test("connection lifecycle: disconnect resets connection state", async () => {
  const ctx = makeContext();
  const manager = new ConnectionManager(ctx as never);
  await manager.initialize();

  const settings = createDefaultConnection({ name: "Dev Server" });
  const conn = await manager.addConnection(settings);
  await manager.setActiveConnection(conn.id);

  // Connect
  manager.setConnectionStatus(conn.id, true, "HTTP 200");
  assert.equal(manager.getConnection(conn.id)!.diagnostic.state, "verified");

  // Disconnect
  manager.setConnectionStatus(conn.id, false, "user disconnected");
  await manager.clearActiveConnection();

  const final = manager.getConnection(conn.id)!;
  assert.equal(final.isActive, false);
  assert.equal(final.isConnected, false);
  assert.equal(final.diagnostic.state, "degraded"); // was verified → degraded on disconnect
});

test("remediation hints: 401 → edit connection message", () => {
  const hint = buildRemediationHint("http://127.0.0.1:8080/health", 401);
  assert.ok(hint.includes("Admin Key"));
  assert.ok(hint.includes("Edit Connection"));
});

test("remediation hints: 403 → role configuration message", () => {
  const hint = buildRemediationHint("http://127.0.0.1:8080/api/v1/sql/execute", 403);
  assert.ok(hint.toLowerCase().includes("role"));
});

test("remediation hints: connection refused → server not running message", () => {
  const conn = {
    id: "test",
    settings: createDefaultConnection({ baseUrl: "http://127.0.0.1:9999" }),
    isActive: true,
    isConnected: false,
    diagnostic: { state: "unverified" } as ConnectionDiagnostic,
  };

  const hint = buildRemediationHint("http://127.0.0.1:9999/health", 0, "ECONNREFUSED", conn);
  assert.ok(hint.includes("http://127.0.0.1:9999"));
  assert.ok(hint.includes("cargo run"));
});

test("remediation hints: timeout → firewall / advanced settings message", () => {
  const hint = buildRemediationHint("http://127.0.0.1:8080/health", 0, "request timed out");
  assert.ok(hint.toLowerCase().includes("timeout") || hint.toLowerCase().includes("timed out"));
  assert.ok(hint.toLowerCase().includes("firewall"));
});
