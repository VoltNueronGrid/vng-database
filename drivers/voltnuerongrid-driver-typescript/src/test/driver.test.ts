import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import {
  VoltNueronGridDriver,
  validateConfig,
  resolveAutoTransport,
} from "../index.js";

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
