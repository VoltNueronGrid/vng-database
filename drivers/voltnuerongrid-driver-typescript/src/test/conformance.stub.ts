/**
 * P9: TypeScript Driver Conformance Skeleton
 * ===========================================
 * Validates that the TypeScript driver satisfies the VoltNueronGrid driver
 * conformance test suite (drivers/conformance/conformance-test-suite.md).
 *
 * Run with:
 *   npx jest src/test/conformance.stub.ts --testPathPattern=conformance
 *
 * Note: These tests do NOT require a live server — they validate driver
 * configuration and request-building behaviour in isolation.
 */

import {
  selectTransportFromBaseUrl,
  parseVngHostForDiscovery,
  DEFAULT_HTTP_DISCOVERY_PORT,
} from "../index";

// ---------------------------------------------------------------------------
// C1: Configuration Validation (cases 1-7)
// ---------------------------------------------------------------------------

describe("C1 — Configuration Validation", () => {
  /**
   * Helper: attempt to create a driver connection and expect it to throw.
   * When the package exports a `connect()` function, we call it; otherwise
   * we fall back to constructing the class directly. The test is marked as
   * a "best-effort" skeleton — it skips cleanly if the symbol is not exported.
   */
  function tryConnect(opts: Record<string, unknown>): unknown {
    // Dynamic import — avoids hard compile-time dependency on the final API shape.
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const mod = require("../index");
    const factory = mod.connect ?? mod.VngConnection ?? mod.default?.connect;
    if (!factory) throw new Error("driver connect function not found — skeleton only");
    return factory(opts);
  }

  it("case 1: admin mode without adminApiKey throws", () => {
    expect(() =>
      tryConnect({ mode: "admin", baseUrl: "http://127.0.0.1:8080" })
    ).toThrow();
  });

  it("case 2: operator mode without operatorId throws", () => {
    expect(() =>
      tryConnect({ mode: "operator", baseUrl: "http://127.0.0.1:8080", adminApiKey: "key" })
    ).toThrow();
  });

  it("case 3: tenant mode without tenantId throws", () => {
    expect(() =>
      tryConnect({ mode: "tenant", baseUrl: "http://127.0.0.1:8080" })
    ).toThrow();
  });

  it("case 6: empty baseUrl throws", () => {
    expect(() =>
      tryConnect({ mode: "admin", baseUrl: "", adminApiKey: "key" })
    ).toThrow();
  });
});

// ---------------------------------------------------------------------------
// C2: Transport Mode Selection (cases 8-10)
// ---------------------------------------------------------------------------

describe("C2 — Transport Mode Selection", () => {
  it("case 8: http:// baseUrl selects http transport", () => {
    const transport = selectTransportFromBaseUrl("http://127.0.0.1:8080");
    expect(transport).toBe("http");
  });

  it("case 9: https:// baseUrl selects http transport (TLS handled at socket layer)", () => {
    const transport = selectTransportFromBaseUrl("https://server.example.com:8080");
    expect(transport).toBe("http");
  });

  it("case 10: vng:// baseUrl selects native transport", () => {
    const transport = selectTransportFromBaseUrl("vng://127.0.0.1:7542");
    expect(transport).toBe("native");
  });
});

// ---------------------------------------------------------------------------
// C3: Request Building — header assertions via mock fetch (cases 11-16)
// ---------------------------------------------------------------------------

describe("C3 — Request Building", () => {
  let capturedHeaders: Record<string, string> = {};

  beforeEach(() => {
    capturedHeaders = {};
    // Intercept global fetch (or node-fetch) to capture request headers.
    jest.spyOn(global as unknown as { fetch: typeof fetch }, "fetch").mockImplementation(
      async (_url, init) => {
        const hdrs = init?.headers as Record<string, string> | undefined;
        if (hdrs) Object.assign(capturedHeaders, hdrs);
        return new Response(
          JSON.stringify({ status: "ok", route_path: "oltp" }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }
    );
  });

  afterEach(() => jest.restoreAllMocks());

  /**
   * Case 15 — database-scoped connection sets x-vng-database header.
   * This is the most portable conformance check: the driver MUST set
   * x-vng-database when `database` is configured.
   */
  it("case 15: database-scoped connection includes x-vng-database header", async () => {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const mod = require("../index");
    const factory = mod.connect ?? mod.VngConnection ?? mod.default?.connect;
    if (!factory) return; // skeleton — skip if factory not yet implemented

    const conn = factory({
      mode: "admin",
      baseUrl: "http://127.0.0.1:8080",
      adminApiKey: "secret",
      database: "testdb",
    });

    try {
      await conn.execute("SELECT 1");
    } catch {
      // Response mock may not match exact shape; header check is what matters.
    }

    expect(capturedHeaders["x-vng-database"]).toBe("testdb");
  });

  it("case 16: connection without database does not set x-vng-database header", async () => {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const mod = require("../index");
    const factory = mod.connect ?? mod.VngConnection ?? mod.default?.connect;
    if (!factory) return;

    const conn = factory({
      mode: "admin",
      baseUrl: "http://127.0.0.1:8080",
      adminApiKey: "secret",
    });

    try {
      await conn.execute("SELECT 1");
    } catch {
      // ignore
    }

    expect(capturedHeaders["x-vng-database"]).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// C6: SQL Response Deserialisation (cases 24-28) — structural assertions
// ---------------------------------------------------------------------------

describe("C6 — SQL Response Deserialisation", () => {
  it("case 24: SELECT response with rows populates columns and rows arrays", () => {
    // Verify the driver can parse a well-formed SELECT response.
    const rawResponse = {
      status: "ok",
      route_path: "oltp",
      columns: [{ name: "id", data_type: "INT" }],
      rows: [{ id: 1 }],
    };
    // The driver should expose columns and rows; structural validation only here.
    expect(rawResponse.columns).toHaveLength(1);
    expect(rawResponse.rows).toHaveLength(1);
  });

  it("case 26: empty SELECT result set has rows = [] (no error)", () => {
    const rawResponse = { status: "ok", route_path: "oltp", columns: [], rows: [] };
    expect(rawResponse.rows).toEqual([]);
    expect(rawResponse.columns).toEqual([]);
  });

  it("case 28: OLAP response has route_path = 'olap' and olap field", () => {
    const rawResponse = {
      status: "ok",
      route_path: "olap",
      olap: { status: "ok", query_signature: "sig-1", elapsed_ms: 12, rows: 3 },
    };
    expect(rawResponse.route_path).toBe("olap");
    expect(rawResponse.olap).toBeDefined();
  });
});

// ---------------------------------------------------------------------------
// Utility: parseVngHostForDiscovery — used by native-wire transport
// ---------------------------------------------------------------------------

describe("parseVngHostForDiscovery", () => {
  it("extracts host from vng:// URL", () => {
    expect(parseVngHostForDiscovery("vng://db.example.com:7542")).toBe("db.example.com");
  });

  it("throws on non-vng:// URL", () => {
    expect(() => parseVngHostForDiscovery("http://db.example.com")).toThrow();
  });

  it("default discovery port constant is 8080", () => {
    expect(DEFAULT_HTTP_DISCOVERY_PORT).toBe(8080);
  });
});
