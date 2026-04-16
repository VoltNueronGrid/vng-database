import test from "node:test";
import assert from "node:assert/strict";
import { createDefaultConnection, validateConnectionSettings } from "../models/Connection";

test("validateConnectionSettings requires operator identity for operator mode", () => {
  const validationError = validateConnectionSettings({
    name: "Operator",
    host: "127.0.0.1",
    port: 8080,
    baseUrl: "http://127.0.0.1:8080",
    mode: "operator",
    ssl: { enabled: false },
    advanced: {},
  });

  assert.equal(validationError, "Operator ID required for operator mode");
});

test("validateConnectionSettings rejects empty SSL certificate paths and invalid advanced values", () => {
  const sslValidationError = validateConnectionSettings({
    name: "TLS",
    host: "127.0.0.1",
    port: 8080,
    baseUrl: "https://127.0.0.1:8080",
    mode: "admin",
    ssl: {
      enabled: true,
      caPath: "",
    },
    advanced: {},
  });

  const timeoutValidationError = validateConnectionSettings({
    name: "Timeout",
    host: "127.0.0.1",
    port: 8080,
    baseUrl: "http://127.0.0.1:8080",
    mode: "admin",
    ssl: { enabled: false },
    advanced: {
      connectionTimeout: 0,
    },
  });

  assert.equal(sslValidationError, "SSL certificate paths cannot be empty");
  assert.equal(timeoutValidationError, "Connection timeout must be greater than 0");
});

test("createDefaultConnection applies overrides while preserving database-client defaults", () => {
  const connection = createDefaultConnection({
    name: "Staging",
    runtimeTarget: "cloud",
    mode: "tenant",
    tenantId: "tenant-a",
    userId: "user-a",
    ssl: {
      enabled: true,
      caPath: "/certs/ca.pem",
      rejectUnauthorized: true,
    },
    advanced: {
      connectionTimeout: 15000,
      idleTimeout: 60000,
      keepAlive: false,
      maxConnections: 4,
    },
  });

  assert.match(connection.id, /^conn-/);
  assert.equal(connection.name, "Staging");
  assert.equal(connection.runtimeTarget, "cloud");
  assert.equal(connection.mode, "tenant");
  assert.equal(connection.baseUrl, "http://127.0.0.1:8080");
  assert.equal(connection.host, "127.0.0.1");
  assert.equal(connection.port, 8080);
  assert.deepEqual(connection.ssl, {
    enabled: true,
    caPath: "/certs/ca.pem",
    rejectUnauthorized: true,
  });
  assert.deepEqual(connection.advanced, {
    connectionTimeout: 15000,
    idleTimeout: 60000,
    keepAlive: false,
    maxConnections: 4,
  });
  assert.ok(connection.createdAt > 0);
});