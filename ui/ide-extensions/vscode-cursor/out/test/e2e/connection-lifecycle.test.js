"use strict";
/**
 * E2E scaffold: connection lifecycle integration test
 *
 * Exercises the full flow without a real server:
 *   Create → Connect → Verify (state=verified) → Browse schema → Query → Disconnect
 *
 * Uses lightweight stub objects in place of VSCode APIs and real HTTP calls.
 * Run with:  node --test src/test/e2e/connection-lifecycle.test.ts
 */
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const Connection_1 = require("../../models/Connection");
const ConnectionManager_1 = require("../../services/ConnectionManager");
const QueryExecutionService_1 = require("../../services/QueryExecutionService");
const RemediationHints_1 = require("../../services/RemediationHints");
// ---------------------------------------------------------------------------
// Minimal stub for vscode.ExtensionContext (only what ConnectionManager uses)
// ---------------------------------------------------------------------------
function makeContext(initialConnections = []) {
    const store = new Map([["vng.connections", initialConnections]]);
    const secrets = new Map();
    return {
        globalState: {
            data: store,
            get(key, defaultValue) {
                return (store.has(key) ? store.get(key) : defaultValue);
            },
            async update(key, value) {
                store.set(key, value);
            },
        },
        secrets: {
            async store(key, value) {
                secrets.set(key, value);
            },
            async get(key) {
                return secrets.get(key);
            },
            async delete(key) {
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
        async testConnection(_connection) {
            return { status: 200, ok: true };
        },
        async executeQuery(_connection, query, _options) {
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
function makeSchemaManagerStub() {
    return {
        async getSchemaRegistry(_conn) {
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
(0, node_test_1.default)("connection lifecycle: create and activate a connection", async () => {
    const ctx = makeContext();
    const manager = new ConnectionManager_1.ConnectionManager(ctx);
    await manager.initialize();
    const settings = (0, Connection_1.createDefaultConnection)({ name: "Dev Server", host: "127.0.0.1", port: 8080 });
    const conn = await manager.addConnection(settings);
    strict_1.default.equal(conn.settings.name, "Dev Server");
    strict_1.default.equal(conn.isActive, false);
    strict_1.default.equal(conn.isConnected, false);
    strict_1.default.equal(conn.diagnostic.state, "unverified");
    const activated = await manager.setActiveConnection(conn.id);
    strict_1.default.ok(activated);
    strict_1.default.equal(activated.isActive, true);
});
(0, node_test_1.default)("connection lifecycle: connect transitions state to verified", async () => {
    const ctx = makeContext();
    const manager = new ConnectionManager_1.ConnectionManager(ctx);
    await manager.initialize();
    const settings = (0, Connection_1.createDefaultConnection)({ name: "Dev Server" });
    const conn = await manager.addConnection(settings);
    await manager.setActiveConnection(conn.id);
    // Simulate a successful probe
    const httpClient = makeSuccessfulHttpClient();
    const probeResult = await httpClient.testConnection(conn);
    strict_1.default.equal(probeResult.status, 200);
    manager.setConnectionStatus(conn.id, probeResult.ok, `HTTP ${probeResult.status}`);
    const updated = manager.getConnection(conn.id);
    strict_1.default.equal(updated.isConnected, true);
    strict_1.default.equal(updated.diagnostic.state, "verified");
    strict_1.default.equal(updated.diagnostic.message, "HTTP 200");
    strict_1.default.ok(updated.diagnostic.lastChecked !== undefined);
});
(0, node_test_1.default)("connection lifecycle: second probe failure after verified → degraded", async () => {
    const ctx = makeContext();
    const manager = new ConnectionManager_1.ConnectionManager(ctx);
    await manager.initialize();
    const settings = (0, Connection_1.createDefaultConnection)({ name: "Dev Server" });
    const conn = await manager.addConnection(settings);
    // First: verified
    manager.setConnectionStatus(conn.id, true, "HTTP 200");
    strict_1.default.equal(manager.getConnection(conn.id).diagnostic.state, "verified");
    // Second probe fails — should become degraded
    manager.setConnectionStatus(conn.id, false, "TCP connection refused");
    const degraded = manager.getConnection(conn.id);
    strict_1.default.equal(degraded.isConnected, false);
    strict_1.default.equal(degraded.diagnostic.state, "degraded");
    strict_1.default.equal(degraded.diagnostic.message, "TCP connection refused");
});
(0, node_test_1.default)("connection lifecycle: probe failure without prior verification → error", async () => {
    const ctx = makeContext();
    const manager = new ConnectionManager_1.ConnectionManager(ctx);
    await manager.initialize();
    const settings = (0, Connection_1.createDefaultConnection)({ name: "Bad Server" });
    const conn = await manager.addConnection(settings);
    // Probe fails immediately (never verified)
    manager.setConnectionStatus(conn.id, false, "HTTP 401: auth failed");
    const errConn = manager.getConnection(conn.id);
    strict_1.default.equal(errConn.diagnostic.state, "error");
    strict_1.default.equal(errConn.diagnostic.message, "HTTP 401: auth failed");
});
(0, node_test_1.default)("connection lifecycle: setConnectionDiagnostic directly", async () => {
    const ctx = makeContext();
    const manager = new ConnectionManager_1.ConnectionManager(ctx);
    await manager.initialize();
    const settings = (0, Connection_1.createDefaultConnection)({ name: "Dev Server" });
    const conn = await manager.addConnection(settings);
    manager.setConnectionDiagnostic(conn.id, "error", "HTTP 403: forbidden");
    const updated = manager.getConnection(conn.id);
    strict_1.default.equal(updated.diagnostic.state, "error");
    strict_1.default.equal(updated.diagnostic.message, "HTTP 403: forbidden");
});
(0, node_test_1.default)("connection lifecycle: browse schema registry", async () => {
    const ctx = makeContext();
    const manager = new ConnectionManager_1.ConnectionManager(ctx);
    await manager.initialize();
    const settings = (0, Connection_1.createDefaultConnection)({ name: "Dev Server" });
    const conn = await manager.addConnection(settings);
    manager.setConnectionStatus(conn.id, true, "HTTP 200");
    const schemaManager = makeSchemaManagerStub();
    const registry = await schemaManager.getSchemaRegistry(conn);
    strict_1.default.equal(registry.databases.length, 1);
    strict_1.default.equal(registry.databases[0].name, "test_db");
    strict_1.default.equal(registry.databases[0].schemas[0].tables[0].name, "users");
});
(0, node_test_1.default)("connection lifecycle: execute a query and check result", async () => {
    const ctx = makeContext();
    const manager = new ConnectionManager_1.ConnectionManager(ctx);
    await manager.initialize();
    const settings = (0, Connection_1.createDefaultConnection)({ name: "Dev Server" });
    const conn = await manager.addConnection(settings);
    manager.setConnectionStatus(conn.id, true, "HTTP 200");
    const globalState = {
        get(_key, defaultValue) {
            return defaultValue;
        },
        async update() {
            return;
        },
    };
    const httpClient = makeSuccessfulHttpClient();
    const queryService = new QueryExecutionService_1.QueryExecutionService(httpClient, globalState);
    const result = await queryService.executeQuery(conn, "select 1;");
    strict_1.default.equal(result.status, "success");
    strict_1.default.ok(result.rows.length > 0);
});
(0, node_test_1.default)("connection lifecycle: disconnect resets connection state", async () => {
    const ctx = makeContext();
    const manager = new ConnectionManager_1.ConnectionManager(ctx);
    await manager.initialize();
    const settings = (0, Connection_1.createDefaultConnection)({ name: "Dev Server" });
    const conn = await manager.addConnection(settings);
    await manager.setActiveConnection(conn.id);
    // Connect
    manager.setConnectionStatus(conn.id, true, "HTTP 200");
    strict_1.default.equal(manager.getConnection(conn.id).diagnostic.state, "verified");
    // Disconnect
    manager.setConnectionStatus(conn.id, false, "user disconnected");
    await manager.clearActiveConnection();
    const final = manager.getConnection(conn.id);
    strict_1.default.equal(final.isActive, false);
    strict_1.default.equal(final.isConnected, false);
    strict_1.default.equal(final.diagnostic.state, "degraded"); // was verified → degraded on disconnect
});
(0, node_test_1.default)("remediation hints: 401 → edit connection message", () => {
    const hint = (0, RemediationHints_1.buildRemediationHint)("http://127.0.0.1:8080/health", 401);
    strict_1.default.ok(hint.includes("Admin Key"));
    strict_1.default.ok(hint.includes("Edit Connection"));
});
(0, node_test_1.default)("remediation hints: 403 → role configuration message", () => {
    const hint = (0, RemediationHints_1.buildRemediationHint)("http://127.0.0.1:8080/api/v1/sql/execute", 403);
    strict_1.default.ok(hint.toLowerCase().includes("role"));
});
(0, node_test_1.default)("remediation hints: connection refused → server not running message", () => {
    const conn = {
        id: "test",
        settings: (0, Connection_1.createDefaultConnection)({ baseUrl: "http://127.0.0.1:9999" }),
        isActive: true,
        isConnected: false,
        diagnostic: { state: "unverified" },
    };
    const hint = (0, RemediationHints_1.buildRemediationHint)("http://127.0.0.1:9999/health", 0, "ECONNREFUSED", conn);
    strict_1.default.ok(hint.includes("http://127.0.0.1:9999"));
    strict_1.default.ok(hint.includes("cargo run"));
});
(0, node_test_1.default)("remediation hints: timeout → firewall / advanced settings message", () => {
    const hint = (0, RemediationHints_1.buildRemediationHint)("http://127.0.0.1:8080/health", 0, "request timed out");
    strict_1.default.ok(hint.toLowerCase().includes("timeout") || hint.toLowerCase().includes("timed out"));
    strict_1.default.ok(hint.toLowerCase().includes("firewall"));
});
//# sourceMappingURL=connection-lifecycle.test.js.map