import test from "node:test";
import assert from "node:assert/strict";
import { createDefaultConnection } from "../models/Connection";
import {
  buildConnectionStatusSummary,
  getConnectionHostLabel,
  toConnectionExportJson,
} from "../commands/ConnectionContextCommands";

test("getConnectionHostLabel derives host and port from baseUrl", () => {
  const connection = {
    id: "conn-1",
    settings: createDefaultConnection({
      id: "conn-1",
      name: "Local",
      baseUrl: "https://db.local:9443",
      host: "db.local",
      port: 9443,
    }),
    isActive: true,
    isConnected: true,
    diagnostic: { state: "verified" as const },
  };

  assert.equal(getConnectionHostLabel(connection), "db.local:9443");
});

test("toConnectionExportJson redacts admin key while preserving diagnostic", () => {
  const connection = {
    id: "conn-2",
    settings: createDefaultConnection({
      id: "conn-2",
      name: "Ops",
      mode: "admin",
      adminKey: "super-secret",
    }),
    isActive: false,
    isConnected: false,
    diagnostic: { state: "degraded" as const, message: "timeout" },
  };

  const exported = JSON.parse(toConnectionExportJson(connection));
  assert.equal(exported.settings.adminKey, "<redacted>");
  assert.equal(exported.state, "degraded");
});

test("buildConnectionStatusSummary includes state and history count", () => {
  const connection = {
    id: "conn-3",
    settings: createDefaultConnection({
      id: "conn-3",
      name: "Tenant A",
      mode: "tenant",
      tenantId: "tenant-a",
      userId: "user-a",
    }),
    isActive: true,
    isConnected: false,
    diagnostic: { state: "degraded" as const, message: "connect_failed" },
  };

  const lines = buildConnectionStatusSummary(connection, 7);
  assert.equal(lines.includes("State: degraded"), true);
  assert.equal(lines.includes("History entries: 7"), true);
});
