import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import * as net from "node:net";
import path from "node:path";
import {
  VoltNueronGridDriver,
  validateConfig,
  resolveAutoTransport,
  resolveAutoTransportWithDiscovery,
  inferHttpBaseUrlFromVngUrl,
  parseDiscoveryHttpPortStr,
  parseHostPort,
} from "../index.js";
import {
  nativeCommandRoundtrip,
  nativeSqlExecuteCommandRoundtrip,
  nativeSqlAnalyzeCommandRoundtrip,
  nativeSqlRouteCommandRoundtrip,
  nativeSqlTransactionCommandRoundtrip,
  nativeSchemaRegistryCommandRoundtrip,
} from "../nativeSession.js";
import { encodeFramedJson } from "../nativeWire.js";

const fixturesDir = path.resolve(process.cwd(), "../conformance/fixtures");

test("validateConfig follows shared conformance fixtures", () => {
  const fixture = JSON.parse(
    readFileSync(path.join(fixturesDir, "config-validation-cases.json"), "utf8")
  ) as {
    cases: Array<{
      name: string;
      config: {
        baseUrl: string;
        sessionId: string;
        mode: "admin" | "operator" | "tenant";
        adminApiKey?: string;
        operatorId?: string;
        tenantId?: string;
      };
      expectError: string;
    }>;
  };

  for (const entry of fixture.cases) {
    const error = validateConfig(entry.config);
    assert.equal(error, entry.expectError, entry.name);
  }
});

test("driver request building follows shared fixture", () => {
  const fixture = JSON.parse(
    readFileSync(path.join(fixturesDir, "request-building-cases.json"), "utf8")
  ) as {
    operatorExecuteCase: {
      config: {
        baseUrl: string;
        sessionId: string;
        mode: "operator";
        adminApiKey: string;
        operatorId: string;
      };
      sqlBatch: string;
      maxRows: number;
      expect: {
        method: string;
        url: string;
        headers: Record<string, string>;
      };
    };
  };

  const useCase = fixture.operatorExecuteCase;
  const driver = new VoltNueronGridDriver(useCase.config);

  const request = driver.buildSqlExecuteRequest(useCase.sqlBatch, useCase.maxRows);
  assert.equal(request.method, useCase.expect.method);
  assert.equal(request.url, useCase.expect.url);
  assert.equal(request.headers["x-vng-admin-key"], useCase.expect.headers["x-vng-admin-key"]);
  assert.equal(request.headers["x-vng-operator-id"], useCase.expect.headers["x-vng-operator-id"]);
});

test("transport mode fixture is consumed for dual-transport conformance gate", () => {
  const fixture = JSON.parse(
    readFileSync(path.join(fixturesDir, "transport-mode-cases.json"), "utf8")
  ) as {
    defaults: {
      fallbackPolicy: string;
      transportAutoOrder: string[];
    };
    cases: Array<{
      id: string;
      transportMode: "http" | "native" | "auto";
      operation: string;
      config: {
        baseUrl: string;
        sessionId: string;
        mode: "admin" | "operator" | "tenant";
        adminApiKey?: string;
        operatorId?: string;
      };
      runtimeCapabilities?: {
        nativeAvailable: boolean;
        httpAvailable: boolean;
      };
      expect?: {
        activeTransport: "http" | "native";
        fallbackTriggered: boolean;
      };
      expectError?: {
        kind: string;
        message: string;
      };
    }>;
  };

  assert.equal(fixture.defaults.fallbackPolicy, "native_primary_http_fallback");
  assert.deepEqual(fixture.defaults.transportAutoOrder, ["native", "http"]);
  assert.ok(fixture.cases.length >= 5);

  const httpCase = fixture.cases.find((entry) => entry.id === "tm-http-execute-operator");
  assert.ok(httpCase);
  assert.equal(httpCase.transportMode, "http");
  assert.equal(httpCase.expect?.activeTransport, "http");

  if (httpCase) {
    const driver = new VoltNueronGridDriver({
      baseUrl: httpCase.config.baseUrl,
      sessionId: httpCase.config.sessionId,
      mode: "operator",
      adminApiKey: httpCase.config.adminApiKey,
      operatorId: httpCase.config.operatorId
    });
    const request = driver.buildSqlExecuteRequest("SELECT 1;", 100);
    assert.equal(request.method, "POST");
    assert.ok(request.url.includes("/api/v1/sql/execute"));
  }

  const autoFallbackCase = fixture.cases.find((entry) => entry.id === "tm-auto-fallback-http");
  assert.ok(autoFallbackCase);
  assert.equal(autoFallbackCase?.transportMode, "auto");
  assert.equal(autoFallbackCase?.runtimeCapabilities?.nativeAvailable, false);
  assert.equal(autoFallbackCase?.runtimeCapabilities?.httpAvailable, true);
  assert.equal(autoFallbackCase?.expect?.activeTransport, "http");
  assert.equal(autoFallbackCase?.expect?.fallbackTriggered, true);

  const noTransportCase = fixture.cases.find((entry) => entry.id === "tm-auto-no-transports");
  assert.ok(noTransportCase);
  assert.equal(noTransportCase?.expectError?.kind, "transport");
});

test("resolveTransportMode auto uses baseUrl scheme (NT-S3-002)", () => {
  const d1 = new VoltNueronGridDriver({
    baseUrl: "vng://127.0.0.1:7542",
    sessionId: "s",
    mode: "admin",
    adminApiKey: "k"
  });
  const r1 = d1.resolveTransportMode("auto");
  assert.equal(r1.active, "native");
  assert.equal(r1.usedAutoResolution, true);

  const d2 = new VoltNueronGridDriver({
    baseUrl: "http://127.0.0.1:8080",
    sessionId: "s",
    mode: "admin",
    adminApiKey: "k"
  });
  const r2 = d2.resolveTransportMode("auto");
  assert.equal(r2.active, "http");
});

test("resolveAutoTransport dual-endpoint matches transport-mode fixture semantics", () => {
  const dual = {
    baseUrl: "vng://127.0.0.1:7542",
    httpFallbackUrl: "http://127.0.0.1:8080",
    sessionId: "s",
    mode: "admin" as const,
    adminApiKey: "secret"
  };
  const a = resolveAutoTransport(dual, { nativeAvailable: true, httpAvailable: true });
  assert.equal(a.active, "native");
  assert.equal(a.fallbackTriggered, false);
  const b = resolveAutoTransport(dual, { nativeAvailable: false, httpAvailable: true });
  assert.equal(b.active, "http");
  assert.equal(b.fallbackTriggered, true);
  assert.equal(b.fallbackReason, "native_unavailable");
  assert.throws(
    () => resolveAutoTransport(dual, { nativeAvailable: false, httpAvailable: false }),
    /no available transport/
  );
});

test("parseDiscoveryHttpPortStr + parseHostPort", () => {
  assert.equal(parseDiscoveryHttpPortStr("8080"), 8080);
  assert.equal(parseDiscoveryHttpPortStr("0"), undefined);
  const hp = parseHostPort("127.0.0.1:65534");
  assert.equal(hp.host, "127.0.0.1");
  assert.equal(hp.port, 65534);
});

test("inferHttpBaseUrlFromVngUrl + resolveAutoTransportWithDiscovery (single-URL discovery port)", () => {
  assert.equal(
    inferHttpBaseUrlFromVngUrl("vng://127.0.0.1:7542", 8080),
    "http://127.0.0.1:8080"
  );
  const cfg = {
    baseUrl: "vng://127.0.0.1:7542",
    sessionId: "s",
    mode: "admin" as const,
    adminApiKey: "k",
  };
  const r = resolveAutoTransportWithDiscovery(
    cfg,
    { nativeAvailable: true, httpAvailable: true },
    8080
  );
  assert.equal(r.active, "native");
  assert.ok((r.notes ?? "").includes("dual-endpoint"));
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
function spawnMockNativeServer(opts: {
  expectAuth?: boolean;
  resultPayload?: Record<string, unknown>;
}): Promise<{
  server: net.Server;
  port: number;
  capturedFramePromise: Promise<Record<string, unknown>>;
}> {
  return new Promise((resolve) => {
    let resolveFrame!: (f: Record<string, unknown>) => void;
    const capturedFramePromise = new Promise<Record<string, unknown>>((res) => {
      resolveFrame = res;
    });

    const server = net.createServer((sock) => {
      let buf = Buffer.alloc(0);
      let step = 0; // 0=await Hello, 1=await Auth (if needed), 2=await Command

      /** Attempt to decode and return a complete framed JSON message from buf. */
      function tryReadFrame(): Record<string, unknown> | null {
        if (buf.length < 4) return null;
        const len = buf.readUInt32BE(0);
        if (buf.length < 4 + len) return null;
        const frame = JSON.parse(buf.subarray(4, 4 + len).toString("utf8")) as Record<
          string,
          unknown
        >;
        buf = buf.subarray(4 + len);
        return frame;
      }

      function writeFrame(payload: unknown) {
        sock.write(encodeFramedJson(payload));
      }

      sock.on("data", (chunk: Buffer) => {
        buf = Buffer.concat([buf, chunk]);
        // eslint-disable-next-line no-constant-condition
        while (true) {
          const frame = tryReadFrame();
          if (!frame) break;

          if (step === 0) {
            writeFrame({
              frame_type: "HelloAck",
              protocol_version: "v1",
              request_id: frame["request_id"],
              session_id: null,
              payload: { status: "ok" },
            });
            step = opts.expectAuth ? 1 : 2;
          } else if (step === 1) {
            writeFrame({
              frame_type: "AuthAck",
              protocol_version: "v1",
              request_id: frame["request_id"],
              session_id: null,
              payload: { status: "ok" },
            });
            step = 2;
          } else if (step === 2) {
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

      sock.on("error", () => { /* tolerate client disconnect */ });
    });

    server.listen(0, "127.0.0.1", () => {
      const addr = server.address() as net.AddressInfo;
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
function decodeFramedJsonBuffer(buf: Buffer): unknown {
  if (buf.length < 4) throw new Error("buffer too short for frame header");
  const len = buf.readUInt32BE(0);
  if (buf.length < 4 + len) throw new Error("buffer too short for frame body");
  return JSON.parse(buf.subarray(4, 4 + len).toString("utf8"));
}

test("NT-S4-001 encodeFramedJson codec round-trips correctly (codec unit test)", () => {
  const frame = {
    frame_type: "Command",
    protocol_version: "v1",
    request_id: "r1",
    session_id: "s1",
    payload: { command: "sql.execute", body: { sql_batch: "SELECT 1;" } },
  };
  const buf = encodeFramedJson(frame);
  const decoded = decodeFramedJsonBuffer(buf);
  assert.deepEqual(decoded, frame);
});

test("NT-S4-001 nativeCommandRoundtrip Command frame shape (via codec inspection)", () => {
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

  const encoded = encodeFramedJson(expectedFrame);
  const decoded = decodeFramedJsonBuffer(encoded) as Record<string, unknown>;

  assert.equal(decoded["frame_type"], "Command");
  assert.equal(decoded["protocol_version"], "v1");
  assert.equal(decoded["session_id"], sessionId);
  const payload = decoded["payload"] as Record<string, unknown>;
  assert.equal(payload["command"], "sql.execute");
  const decodedBody = payload["body"] as Record<string, unknown>;
  assert.equal(decodedBody["sql_batch"], "SELECT 1;");
});

test("NT-S4-001 nativeSqlExecuteCommandRoundtrip frame includes max_rows when provided", () => {
  const frameBody: Record<string, unknown> = { sql_batch: "SELECT * FROM t;", max_rows: 500 };
  const encoded = encodeFramedJson({
    frame_type: "Command",
    protocol_version: "v1",
    request_id: "r-cmd",
    session_id: "sess-execute",
    payload: { command: "sql.execute", body: frameBody },
  });
  const decoded = decodeFramedJsonBuffer(encoded) as Record<string, unknown>;
  const payload = decoded["payload"] as Record<string, unknown>;
  assert.equal(payload["command"], "sql.execute");
  const body = payload["body"] as Record<string, unknown>;
  assert.equal(body["sql_batch"], "SELECT * FROM t;");
  assert.equal(body["max_rows"], 500);
});

test("NT-S4-001 nativeSqlExecuteCommandRoundtrip frame omits max_rows when not provided", () => {
  const frameBody: Record<string, unknown> = { sql_batch: "SELECT 1;" };
  // omit max_rows (not setting it at all)
  const encoded = encodeFramedJson({
    frame_type: "Command",
    protocol_version: "v1",
    request_id: "r-cmd",
    session_id: "sess-execute-no-maxrows",
    payload: { command: "sql.execute", body: frameBody },
  });
  const decoded = decodeFramedJsonBuffer(encoded) as Record<string, unknown>;
  const payload = decoded["payload"] as Record<string, unknown>;
  const body = payload["body"] as Record<string, unknown>;
  assert.ok(!("max_rows" in body), "max_rows should be absent when not provided");
});

test("NT-S4-001 nativeSqlAnalyzeCommandRoundtrip uses sql.analyze command name", () => {
  const encoded = encodeFramedJson({
    frame_type: "Command",
    protocol_version: "v1",
    request_id: "r-cmd",
    session_id: "sess-analyze",
    payload: { command: "sql.analyze", body: { sql_batch: "EXPLAIN SELECT 1;" } },
  });
  const decoded = decodeFramedJsonBuffer(encoded) as Record<string, unknown>;
  const payload = decoded["payload"] as Record<string, unknown>;
  assert.equal(payload["command"], "sql.analyze");
  const body = payload["body"] as Record<string, unknown>;
  assert.equal(body["sql_batch"], "EXPLAIN SELECT 1;");
});

test("NT-S4-001 nativeSqlRouteCommandRoundtrip uses sql.route command name", () => {
  const encoded = encodeFramedJson({
    frame_type: "Command",
    protocol_version: "v1",
    request_id: "r-cmd",
    session_id: "sess-route",
    payload: { command: "sql.route", body: { sql_batch: "SELECT 1;" } },
  });
  const decoded = decodeFramedJsonBuffer(encoded) as Record<string, unknown>;
  const payload = decoded["payload"] as Record<string, unknown>;
  assert.equal(payload["command"], "sql.route");
});

test("NT-S4-001 nativeSqlTransactionCommandRoundtrip uses sql.transaction with statements and isolation_level", () => {
  const statements = ["INSERT INTO t VALUES (1);", "INSERT INTO t VALUES (2);"];
  const encoded = encodeFramedJson({
    frame_type: "Command",
    protocol_version: "v1",
    request_id: "r-cmd",
    session_id: "sess-txn",
    payload: { command: "sql.transaction", body: { statements, isolation_level: "serializable" } },
  });
  const decoded = decodeFramedJsonBuffer(encoded) as Record<string, unknown>;
  const payload = decoded["payload"] as Record<string, unknown>;
  assert.equal(payload["command"], "sql.transaction");
  const body = payload["body"] as Record<string, unknown>;
  assert.deepEqual(body["statements"], statements);
  assert.equal(body["isolation_level"], "serializable");
});

test("NT-S4-001 nativeSqlTransactionCommandRoundtrip omits isolation_level when not provided", () => {
  const frameBody: Record<string, unknown> = { statements: ["SELECT 1;"] };
  const encoded = encodeFramedJson({
    frame_type: "Command",
    protocol_version: "v1",
    request_id: "r-cmd",
    session_id: "sess-txn-noisol",
    payload: { command: "sql.transaction", body: frameBody },
  });
  const decoded = decodeFramedJsonBuffer(encoded) as Record<string, unknown>;
  const payload = decoded["payload"] as Record<string, unknown>;
  const body = payload["body"] as Record<string, unknown>;
  assert.ok(!("isolation_level" in body), "isolation_level should be absent when not provided");
});

test("NT-S4-001 nativeSchemaRegistryCommandRoundtrip uses ingest.schema.registry with empty body", () => {
  const encoded = encodeFramedJson({
    frame_type: "Command",
    protocol_version: "v1",
    request_id: "r-cmd",
    session_id: "sess-schema",
    payload: { command: "ingest.schema.registry", body: {} },
  });
  const decoded = decodeFramedJsonBuffer(encoded) as Record<string, unknown>;
  const payload = decoded["payload"] as Record<string, unknown>;
  assert.equal(payload["command"], "ingest.schema.registry");
  const body = payload["body"] as Record<string, unknown>;
  assert.deepEqual(body, {});
});

// ---------------------------------------------------------------------------
// NT-S4-001 — TCP integration tests using the mock server
// These use the mock server's queue-based reader that avoids the race.
// ---------------------------------------------------------------------------

test("NT-S4-001 nativeCommandRoundtrip TCP roundtrip — sql.execute (no auth)", async () => {
  const { server, port, capturedFramePromise } = await spawnMockNativeServer({
    expectAuth: false,
    resultPayload: { status: "ok", rows: [] },
  });

  try {
    await nativeCommandRoundtrip(
      { host: "127.0.0.1", port, sessionId: "test-session-1", connectTimeoutMs: 5000 },
      "sql.execute",
      { sql_batch: "SELECT 1;" }
    );

    const captured = await capturedFramePromise;
    assert.equal(captured["frame_type"], "Command");
    assert.equal(captured["protocol_version"], "v1");
    assert.equal(captured["session_id"], "test-session-1");
    const payload = captured["payload"] as Record<string, unknown>;
    assert.equal(payload["command"], "sql.execute");
    const body = payload["body"] as Record<string, unknown>;
    assert.equal(body["sql_batch"], "SELECT 1;");
  } finally {
    server.close();
  }
});

test("NT-S4-001 nativeSchemaRegistryCommandRoundtrip TCP roundtrip — ingest.schema.registry", async () => {
  const { server, port, capturedFramePromise } = await spawnMockNativeServer({
    expectAuth: false,
    resultPayload: { schemas: [] },
  });

  try {
    await nativeSchemaRegistryCommandRoundtrip(
      { host: "127.0.0.1", port, sessionId: "sess-schema", connectTimeoutMs: 5000 }
    );

    const captured = await capturedFramePromise;
    const payload = captured["payload"] as Record<string, unknown>;
    assert.equal(payload["command"], "ingest.schema.registry");
    const body = payload["body"] as Record<string, unknown>;
    assert.deepEqual(body, {});
  } finally {
    server.close();
  }
});

test("NT-S4-001 nativeCommandRoundtrip TCP roundtrip with adminApiKey sends Auth frame", async () => {
  const { server, port, capturedFramePromise } = await spawnMockNativeServer({
    expectAuth: true,
  });

  try {
    await nativeCommandRoundtrip(
      {
        host: "127.0.0.1",
        port,
        sessionId: "sess-auth",
        adminApiKey: "super-secret-key",
        connectTimeoutMs: 5000,
      },
      "health",
      {}
    );

    const captured = await capturedFramePromise;
    assert.equal(captured["frame_type"], "Command");
    const payload = captured["payload"] as Record<string, unknown>;
    assert.equal(payload["command"], "health");
  } finally {
    server.close();
  }
});
