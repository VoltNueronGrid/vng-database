import test from "node:test";
import assert from "node:assert/strict";
import {
  describeConnectionNode,
  describeTableRowCount,
  describeTableSections,
  getConnectionFlowSnapshot,
  groupConnectionsForTree,
  shouldExpandConnectionToDatabases,
} from "../providers/DatabaseExplorerTree";
import { createDefaultConnection } from "../models/Connection";
import { Table } from "../models/Schema";

test("describeConnectionNode marks active connection state", () => {
  const connection = {
    id: "conn-1",
    settings: createDefaultConnection({ name: "Local Dev" }),
    isActive: true,
    isConnected: true,
    state: "verified" as const,
    diagnostics: {},
  };

  const presentation = describeConnectionNode(connection);
  assert.equal(presentation.contextValue, "connectionActive");
  assert.match(presentation.description, /Active/);
  assert.match(presentation.description, /Verified/);
});

test("describeConnectionNode guides inactive browsing flow", () => {
  const connection = {
    id: "conn-2",
    settings: createDefaultConnection({ name: "Staging" }),
    isActive: false,
    isConnected: false,
    state: "active" as const,
    diagnostics: {},
  };

  const presentation = describeConnectionNode(connection);
  assert.equal(presentation.contextValue, "connectionInactive");
  assert.match(presentation.description, /Not verified/);
  assert.equal(presentation.browseMessage, "Activate Staging to browse databases.");
});

test("connection flow covers empty -> create -> connect -> expand -> disconnect", () => {
  const createdConnection = {
    id: "conn-flow",
    settings: createDefaultConnection({ name: "Flow Connection" }),
    isActive: false,
    isConnected: false,
    state: "active" as const,
    diagnostics: {},
  };

  const emptySnapshot = getConnectionFlowSnapshot([]);
  assert.equal(emptySnapshot.rootKind, "empty");
  assert.equal(emptySnapshot.canExpand, false);

  const afterCreateSnapshot = getConnectionFlowSnapshot([createdConnection], createdConnection);
  assert.equal(afterCreateSnapshot.rootKind, "connections");
  assert.equal(afterCreateSnapshot.canExpand, false);

  const connected = { ...createdConnection, isActive: true, isConnected: true, state: "verified" as const };
  const afterConnectSnapshot = getConnectionFlowSnapshot([connected], connected);
  assert.equal(afterConnectSnapshot.rootKind, "connections");
  assert.equal(afterConnectSnapshot.canExpand, true);
  assert.equal(shouldExpandConnectionToDatabases(connected), true);

  const disconnected = { ...connected, isActive: false, isConnected: false, state: "degraded" as const };
  const afterDisconnectSnapshot = getConnectionFlowSnapshot([disconnected], disconnected);
  assert.equal(afterDisconnectSnapshot.rootKind, "connections");
  assert.equal(afterDisconnectSnapshot.canExpand, false);
  assert.equal(shouldExpandConnectionToDatabases(disconnected), false);
});

test("groupConnectionsForTree groups by folder and keeps deterministic ordering", () => {
  const alpha = {
    id: "a",
    settings: createDefaultConnection({ name: "Alpha", group: "staging" }),
    isActive: false,
    isConnected: false,
    state: "active" as const,
    diagnostics: {},
  };
  const beta = {
    id: "b",
    settings: createDefaultConnection({ name: "Beta", group: "prod" }),
    isActive: false,
    isConnected: false,
    state: "active" as const,
    diagnostics: {},
  };
  const gamma = {
    id: "c",
    settings: createDefaultConnection({ name: "Gamma" }),
    isActive: false,
    isConnected: false,
    state: "active" as const,
    diagnostics: {},
  };

  const grouped = groupConnectionsForTree([gamma, alpha, beta]);
  assert.deepEqual(grouped.map((bucket) => bucket.groupLabel), ["localmachine", "prod", "staging"]);
  assert.deepEqual(grouped[0]?.connections.map((connection) => connection.settings.name), ["Gamma"]);
  assert.deepEqual(grouped[1]?.connections.map((connection) => connection.settings.name), ["Beta"]);
  assert.deepEqual(grouped[2]?.connections.map((connection) => connection.settings.name), ["Alpha"]);
});

test("describeTableSections returns deterministic section order and counts", () => {
  const table: Table = {
    name: "users",
    schema: "public",
    columns: [
      {
        name: "id",
        type: "BIGINT",
        nullable: false,
        defaultValue: "",
        isPrimaryKey: true,
        isForeignKey: false,
        isUnique: true,
      },
      {
        name: "email",
        type: "VARCHAR",
        nullable: false,
        defaultValue: "",
        isPrimaryKey: false,
        isForeignKey: false,
        isUnique: false,
      },
    ],
    indexes: [{ name: "users_pkey", columns: ["id"], isUnique: true, isPrimary: true }],
    triggers: [{ name: "users_audit", event: "INSERT", timing: "AFTER", enabled: true }],
  };

  const sections = describeTableSections(table);
  assert.deepEqual(
    sections.map((entry) => ({ kind: entry.kind, count: entry.count })),
    [
      { kind: "columns", count: 2 },
      { kind: "indexes", count: 1 },
      { kind: "triggers", count: 1 },
    ]
  );
});

test("describeTableRowCount formats known counts and hides missing values", () => {
  const table: Table = {
    name: "events",
    schema: "public",
    columns: [],
    indexes: [],
    rowCount: 15230,
  };

  assert.equal(describeTableRowCount(table), "~15.2K rows");
  assert.equal(describeTableRowCount({ ...table, rowCount: 912 }), "912 rows");
  assert.equal(describeTableRowCount({ ...table, rowCount: 2_700_000 }), "~2.7M rows");
  assert.equal(describeTableRowCount({ ...table, rowCount: 4_500_000_000 }), "~4.5B rows");
  assert.equal(describeTableRowCount({ ...table, rowCount: undefined }), "");
  assert.equal(describeTableRowCount({ ...table, rowCount: -1 }), "");
});