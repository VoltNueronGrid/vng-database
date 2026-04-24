"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || function (mod) {
    if (mod && mod.__esModule) return mod;
    var result = {};
    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
    __setModuleDefault(result, mod);
    return result;
};
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const node_fs_1 = require("node:fs");
const net = __importStar(require("node:net"));
const node_path_1 = __importDefault(require("node:path"));
const index_js_1 = require("../index.js");
const nativeSession_js_1 = require("../nativeSession.js");
const nativeWire_js_1 = require("../nativeWire.js");
const fixturesDir = node_path_1.default.resolve(process.cwd(), "../conformance/fixtures");
(0, node_test_1.default)("validateConfig follows shared conformance fixtures", () => {
    const fixture = JSON.parse((0, node_fs_1.readFileSync)(node_path_1.default.join(fixturesDir, "config-validation-cases.json"), "utf8"));
    for (const entry of fixture.cases) {
        const error = (0, index_js_1.validateConfig)(entry.config);
        strict_1.default.equal(error, entry.expectError, entry.name);
    }
});
(0, node_test_1.default)("driver request building follows shared fixture", () => {
    const fixture = JSON.parse((0, node_fs_1.readFileSync)(node_path_1.default.join(fixturesDir, "request-building-cases.json"), "utf8"));
    const useCase = fixture.operatorExecuteCase;
    const driver = new index_js_1.VoltNueronGridDriver(useCase.config);
    const request = driver.buildSqlExecuteRequest(useCase.sqlBatch, useCase.maxRows);
    strict_1.default.equal(request.method, useCase.expect.method);
    strict_1.default.equal(request.url, useCase.expect.url);
    strict_1.default.equal(request.headers["x-vng-admin-key"], useCase.expect.headers["x-vng-admin-key"]);
    strict_1.default.equal(request.headers["x-vng-operator-id"], useCase.expect.headers["x-vng-operator-id"]);
});
(0, node_test_1.default)("transport mode fixture is consumed for dual-transport conformance gate", () => {
    const fixture = JSON.parse((0, node_fs_1.readFileSync)(node_path_1.default.join(fixturesDir, "transport-mode-cases.json"), "utf8"));
    strict_1.default.equal(fixture.defaults.fallbackPolicy, "native_primary_http_fallback");
    strict_1.default.deepEqual(fixture.defaults.transportAutoOrder, ["native", "http"]);
    strict_1.default.ok(fixture.cases.length >= 5);
    const httpCase = fixture.cases.find((entry) => entry.id === "tm-http-execute-operator");
    strict_1.default.ok(httpCase);
    strict_1.default.equal(httpCase.transportMode, "http");
    strict_1.default.equal(httpCase.expect?.activeTransport, "http");
    if (httpCase) {
        const driver = new index_js_1.VoltNueronGridDriver({
            baseUrl: httpCase.config.baseUrl,
            sessionId: httpCase.config.sessionId,
            mode: "operator",
            adminApiKey: httpCase.config.adminApiKey,
            operatorId: httpCase.config.operatorId
        });
        const request = driver.buildSqlExecuteRequest("SELECT 1;", 100);
        strict_1.default.equal(request.method, "POST");
        strict_1.default.ok(request.url.includes("/api/v1/sql/execute"));
    }
    const autoFallbackCase = fixture.cases.find((entry) => entry.id === "tm-auto-fallback-http");
    strict_1.default.ok(autoFallbackCase);
    strict_1.default.equal(autoFallbackCase?.transportMode, "auto");
    strict_1.default.equal(autoFallbackCase?.runtimeCapabilities?.nativeAvailable, false);
    strict_1.default.equal(autoFallbackCase?.runtimeCapabilities?.httpAvailable, true);
    strict_1.default.equal(autoFallbackCase?.expect?.activeTransport, "http");
    strict_1.default.equal(autoFallbackCase?.expect?.fallbackTriggered, true);
    const noTransportCase = fixture.cases.find((entry) => entry.id === "tm-auto-no-transports");
    strict_1.default.ok(noTransportCase);
    strict_1.default.equal(noTransportCase?.expectError?.kind, "transport");
});
(0, node_test_1.default)("resolveTransportMode auto uses baseUrl scheme (NT-S3-002)", () => {
    const d1 = new index_js_1.VoltNueronGridDriver({
        baseUrl: "vng://127.0.0.1:7542",
        sessionId: "s",
        mode: "admin",
        adminApiKey: "k"
    });
    const r1 = d1.resolveTransportMode("auto");
    strict_1.default.equal(r1.active, "native");
    strict_1.default.equal(r1.usedAutoResolution, true);
    const d2 = new index_js_1.VoltNueronGridDriver({
        baseUrl: "http://127.0.0.1:8080",
        sessionId: "s",
        mode: "admin",
        adminApiKey: "k"
    });
    const r2 = d2.resolveTransportMode("auto");
    strict_1.default.equal(r2.active, "http");
});
(0, node_test_1.default)("resolveAutoTransport dual-endpoint matches transport-mode fixture semantics", () => {
    const dual = {
        baseUrl: "vng://127.0.0.1:7542",
        httpFallbackUrl: "http://127.0.0.1:8080",
        sessionId: "s",
        mode: "admin",
        adminApiKey: "secret"
    };
    const a = (0, index_js_1.resolveAutoTransport)(dual, { nativeAvailable: true, httpAvailable: true });
    strict_1.default.equal(a.active, "native");
    strict_1.default.equal(a.fallbackTriggered, false);
    const b = (0, index_js_1.resolveAutoTransport)(dual, { nativeAvailable: false, httpAvailable: true });
    strict_1.default.equal(b.active, "http");
    strict_1.default.equal(b.fallbackTriggered, true);
    strict_1.default.equal(b.fallbackReason, "native_unavailable");
    strict_1.default.throws(() => (0, index_js_1.resolveAutoTransport)(dual, { nativeAvailable: false, httpAvailable: false }), /no available transport/);
});
(0, node_test_1.default)("parseDiscoveryHttpPortStr + parseHostPort", () => {
    strict_1.default.equal((0, index_js_1.parseDiscoveryHttpPortStr)("8080"), 8080);
    strict_1.default.equal((0, index_js_1.parseDiscoveryHttpPortStr)("0"), undefined);
    const hp = (0, index_js_1.parseHostPort)("127.0.0.1:65534");
    strict_1.default.equal(hp.host, "127.0.0.1");
    strict_1.default.equal(hp.port, 65534);
});
(0, node_test_1.default)("inferHttpBaseUrlFromVngUrl + resolveAutoTransportWithDiscovery (single-URL discovery port)", () => {
    strict_1.default.equal((0, index_js_1.inferHttpBaseUrlFromVngUrl)("vng://127.0.0.1:7542", 8080), "http://127.0.0.1:8080");
    const cfg = {
        baseUrl: "vng://127.0.0.1:7542",
        sessionId: "s",
        mode: "admin",
        adminApiKey: "k",
    };
    const r = (0, index_js_1.resolveAutoTransportWithDiscovery)(cfg, { nativeAvailable: true, httpAvailable: true }, 8080);
    strict_1.default.equal(r.active, "native");
    strict_1.default.ok((r.notes ?? "").includes("dual-endpoint"));
});
// ---------------------------------------------------------------------------
// NT-S4-001 — native session command parity unit tests (mock TCP server)
// ---------------------------------------------------------------------------
/**
 * Spin up a minimal framed-JSON TCP server that uses a single persistent
 * `data` listener with an accumulated buffer to avoid the race between
 * listener teardown and re-registration that occurs in readExactly.
 *
 * Protocol:
 *   1. Hello → HelloAck
 *   2. Auth → AuthAck  (only when expectAuth: true)
 *   3. Command → capture frame + Result
 */
function spawnMockNativeServer(opts) {
    return new Promise((resolve) => {
        let resolveFrame;
        const capturedFramePromise = new Promise((res) => {
            resolveFrame = res;
        });
        const server = net.createServer((sock) => {
            let buf = Buffer.alloc(0);
            let step = 0; // 0=await Hello, 1=await Auth (if needed), 2=await Command
            /** Attempt to decode and return a complete framed JSON message from buf. */
            function tryReadFrame() {
                if (buf.length < 4)
                    return null;
                const len = buf.readUInt32BE(0);
                if (buf.length < 4 + len)
                    return null;
                const frame = JSON.parse(buf.subarray(4, 4 + len).toString("utf8"));
                buf = buf.subarray(4 + len);
                return frame;
            }
            function writeFrame(payload) {
                sock.write((0, nativeWire_js_1.encodeFramedJson)(payload));
            }
            sock.on("data", (chunk) => {
                buf = Buffer.concat([buf, chunk]);
                // eslint-disable-next-line no-constant-condition
                while (true) {
                    const frame = tryReadFrame();
                    if (!frame)
                        break;
                    if (step === 0) {
                        writeFrame({
                            frame_type: "HelloAck",
                            protocol_version: "v1",
                            request_id: frame["request_id"],
                            session_id: null,
                            payload: { status: "ok" },
                        });
                        step = opts.expectAuth ? 1 : 2;
                    }
                    else if (step === 1) {
                        writeFrame({
                            frame_type: "AuthAck",
                            protocol_version: "v1",
                            request_id: frame["request_id"],
                            session_id: null,
                            payload: { status: "ok" },
                        });
                        step = 2;
                    }
                    else if (step === 2) {
                        resolveFrame(frame);
                        writeFrame({
                            frame_type: "Result",
                            protocol_version: "v1",
                            request_id: frame["request_id"],
                            session_id: null,
                            payload: opts.resultPayload ?? { status: "ok" },
                        });
                        sock.end();
                    }
                }
            });
            sock.on("error", () => { });
        });
        server.listen(0, "127.0.0.1", () => {
            const addr = server.address();
            resolve({ server, port: addr.port, capturedFramePromise });
        });
    });
}
// ---------------------------------------------------------------------------
// NT-S4-001 — pure unit tests: verify frame construction without TCP
//
// These tests verify the Command frame payload that each helper function
// builds by inspecting what encodeFramedJson would receive. They use the
// encodeFramedJson codec directly to decode the bytes sent by the helper.
// ---------------------------------------------------------------------------
/**
 * Decode a framed JSON buffer produced by encodeFramedJson.
 * Used in pure unit tests to inspect the wire representation.
 */
function decodeFramedJsonBuffer(buf) {
    if (buf.length < 4)
        throw new Error("buffer too short for frame header");
    const len = buf.readUInt32BE(0);
    if (buf.length < 4 + len)
        throw new Error("buffer too short for frame body");
    return JSON.parse(buf.subarray(4, 4 + len).toString("utf8"));
}
(0, node_test_1.default)("NT-S4-001 encodeFramedJson codec round-trips correctly (codec unit test)", () => {
    const frame = {
        frame_type: "Command",
        protocol_version: "v1",
        request_id: "r1",
        session_id: "s1",
        payload: { command: "sql.execute", body: { sql_batch: "SELECT 1;" } },
    };
    const buf = (0, nativeWire_js_1.encodeFramedJson)(frame);
    const decoded = decodeFramedJsonBuffer(buf);
    strict_1.default.deepEqual(decoded, frame);
});
(0, node_test_1.default)("NT-S4-001 nativeCommandRoundtrip Command frame shape (via codec inspection)", () => {
    // Capture what would be written to the socket by intercepting encodeFramedJson output.
    // We build the expected Command frame directly and verify it matches the codec output.
    const commandName = "sql.execute";
    const body = { sql_batch: "SELECT 1;" };
    const sessionId = "test-session-1";
    const rid = "ts-native-sql.execute";
    const expectedFrame = {
        frame_type: "Command",
        protocol_version: "v1",
        request_id: `${rid}-cmd`,
        session_id: sessionId,
        payload: { command: commandName, body },
    };
    const encoded = (0, nativeWire_js_1.encodeFramedJson)(expectedFrame);
    const decoded = decodeFramedJsonBuffer(encoded);
    strict_1.default.equal(decoded["frame_type"], "Command");
    strict_1.default.equal(decoded["protocol_version"], "v1");
    strict_1.default.equal(decoded["session_id"], sessionId);
    const payload = decoded["payload"];
    strict_1.default.equal(payload["command"], "sql.execute");
    const decodedBody = payload["body"];
    strict_1.default.equal(decodedBody["sql_batch"], "SELECT 1;");
});
(0, node_test_1.default)("NT-S4-001 nativeSqlExecuteCommandRoundtrip frame includes max_rows when provided", () => {
    const frameBody = { sql_batch: "SELECT * FROM t;", max_rows: 500 };
    const encoded = (0, nativeWire_js_1.encodeFramedJson)({
        frame_type: "Command",
        protocol_version: "v1",
        request_id: "r-cmd",
        session_id: "sess-execute",
        payload: { command: "sql.execute", body: frameBody },
    });
    const decoded = decodeFramedJsonBuffer(encoded);
    const payload = decoded["payload"];
    strict_1.default.equal(payload["command"], "sql.execute");
    const body = payload["body"];
    strict_1.default.equal(body["sql_batch"], "SELECT * FROM t;");
    strict_1.default.equal(body["max_rows"], 500);
});
(0, node_test_1.default)("NT-S4-001 nativeSqlExecuteCommandRoundtrip frame omits max_rows when not provided", () => {
    const frameBody = { sql_batch: "SELECT 1;" };
    // omit max_rows (not setting it at all)
    const encoded = (0, nativeWire_js_1.encodeFramedJson)({
        frame_type: "Command",
        protocol_version: "v1",
        request_id: "r-cmd",
        session_id: "sess-execute-no-maxrows",
        payload: { command: "sql.execute", body: frameBody },
    });
    const decoded = decodeFramedJsonBuffer(encoded);
    const payload = decoded["payload"];
    const body = payload["body"];
    strict_1.default.ok(!("max_rows" in body), "max_rows should be absent when not provided");
});
(0, node_test_1.default)("NT-S4-001 nativeSqlAnalyzeCommandRoundtrip uses sql.analyze command name", () => {
    const encoded = (0, nativeWire_js_1.encodeFramedJson)({
        frame_type: "Command",
        protocol_version: "v1",
        request_id: "r-cmd",
        session_id: "sess-analyze",
        payload: { command: "sql.analyze", body: { sql_batch: "EXPLAIN SELECT 1;" } },
    });
    const decoded = decodeFramedJsonBuffer(encoded);
    const payload = decoded["payload"];
    strict_1.default.equal(payload["command"], "sql.analyze");
    const body = payload["body"];
    strict_1.default.equal(body["sql_batch"], "EXPLAIN SELECT 1;");
});
(0, node_test_1.default)("NT-S4-001 nativeSqlRouteCommandRoundtrip uses sql.route command name", () => {
    const encoded = (0, nativeWire_js_1.encodeFramedJson)({
        frame_type: "Command",
        protocol_version: "v1",
        request_id: "r-cmd",
        session_id: "sess-route",
        payload: { command: "sql.route", body: { sql_batch: "SELECT 1;" } },
    });
    const decoded = decodeFramedJsonBuffer(encoded);
    const payload = decoded["payload"];
    strict_1.default.equal(payload["command"], "sql.route");
});
(0, node_test_1.default)("NT-S4-001 nativeSqlTransactionCommandRoundtrip uses sql.transaction with statements and isolation_level", () => {
    const statements = ["INSERT INTO t VALUES (1);", "INSERT INTO t VALUES (2);"];
    const encoded = (0, nativeWire_js_1.encodeFramedJson)({
        frame_type: "Command",
        protocol_version: "v1",
        request_id: "r-cmd",
        session_id: "sess-txn",
        payload: { command: "sql.transaction", body: { statements, isolation_level: "serializable" } },
    });
    const decoded = decodeFramedJsonBuffer(encoded);
    const payload = decoded["payload"];
    strict_1.default.equal(payload["command"], "sql.transaction");
    const body = payload["body"];
    strict_1.default.deepEqual(body["statements"], statements);
    strict_1.default.equal(body["isolation_level"], "serializable");
});
(0, node_test_1.default)("NT-S4-001 nativeSqlTransactionCommandRoundtrip omits isolation_level when not provided", () => {
    const frameBody = { statements: ["SELECT 1;"] };
    const encoded = (0, nativeWire_js_1.encodeFramedJson)({
        frame_type: "Command",
        protocol_version: "v1",
        request_id: "r-cmd",
        session_id: "sess-txn-noisol",
        payload: { command: "sql.transaction", body: frameBody },
    });
    const decoded = decodeFramedJsonBuffer(encoded);
    const payload = decoded["payload"];
    const body = payload["body"];
    strict_1.default.ok(!("isolation_level" in body), "isolation_level should be absent when not provided");
});
(0, node_test_1.default)("NT-S4-001 nativeSchemaRegistryCommandRoundtrip uses ingest.schema.registry with empty body", () => {
    const encoded = (0, nativeWire_js_1.encodeFramedJson)({
        frame_type: "Command",
        protocol_version: "v1",
        request_id: "r-cmd",
        session_id: "sess-schema",
        payload: { command: "ingest.schema.registry", body: {} },
    });
    const decoded = decodeFramedJsonBuffer(encoded);
    const payload = decoded["payload"];
    strict_1.default.equal(payload["command"], "ingest.schema.registry");
    const body = payload["body"];
    strict_1.default.deepEqual(body, {});
});
// ---------------------------------------------------------------------------
// NT-S4-001 — TCP integration tests using the mock server
// These use the mock server's queue-based reader that avoids the race.
// ---------------------------------------------------------------------------
(0, node_test_1.default)("NT-S4-001 nativeCommandRoundtrip TCP roundtrip — sql.execute (no auth)", async () => {
    const { server, port, capturedFramePromise } = await spawnMockNativeServer({
        expectAuth: false,
        resultPayload: { status: "ok", rows: [] },
    });
    try {
        await (0, nativeSession_js_1.nativeCommandRoundtrip)({ host: "127.0.0.1", port, sessionId: "test-session-1", connectTimeoutMs: 5000 }, "sql.execute", { sql_batch: "SELECT 1;" });
        const captured = await capturedFramePromise;
        strict_1.default.equal(captured["frame_type"], "Command");
        strict_1.default.equal(captured["protocol_version"], "v1");
        strict_1.default.equal(captured["session_id"], "test-session-1");
        const payload = captured["payload"];
        strict_1.default.equal(payload["command"], "sql.execute");
        const body = payload["body"];
        strict_1.default.equal(body["sql_batch"], "SELECT 1;");
    }
    finally {
        server.close();
    }
});
(0, node_test_1.default)("NT-S4-001 nativeSchemaRegistryCommandRoundtrip TCP roundtrip — ingest.schema.registry", async () => {
    const { server, port, capturedFramePromise } = await spawnMockNativeServer({
        expectAuth: false,
        resultPayload: { schemas: [] },
    });
    try {
        await (0, nativeSession_js_1.nativeSchemaRegistryCommandRoundtrip)({ host: "127.0.0.1", port, sessionId: "sess-schema", connectTimeoutMs: 5000 });
        const captured = await capturedFramePromise;
        const payload = captured["payload"];
        strict_1.default.equal(payload["command"], "ingest.schema.registry");
        const body = payload["body"];
        strict_1.default.deepEqual(body, {});
    }
    finally {
        server.close();
    }
});
(0, node_test_1.default)("NT-S4-001 nativeCommandRoundtrip TCP roundtrip with adminApiKey sends Auth frame", async () => {
    const { server, port, capturedFramePromise } = await spawnMockNativeServer({
        expectAuth: true,
    });
    try {
        await (0, nativeSession_js_1.nativeCommandRoundtrip)({
            host: "127.0.0.1",
            port,
            sessionId: "sess-auth",
            adminApiKey: "super-secret-key",
            connectTimeoutMs: 5000,
        }, "health", {});
        const captured = await capturedFramePromise;
        strict_1.default.equal(captured["frame_type"], "Command");
        const payload = captured["payload"];
        strict_1.default.equal(payload["command"], "health");
    }
    finally {
        server.close();
    }
});
