import test from "node:test";
import assert from "node:assert/strict";
import {
  describeConnectionNode,
  getConnectionFlowSnapshot,
  shouldExpandConnectionToDatabases,
} from "../providers/DatabaseExplorerTree";
import { createDefaultConnection } from "../models/Connection";

test("describeConnectionNode marks active connection state", () => {
  const connection = {
    id: "conn-1",
    settings: createDefaultConnection({ name: "Local Dev" }),
    isActive: true,
    isConnected: true,
    diagnostic: { state: "verified" as const },
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
    diagnostic: { state: "unverified" as const },
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
    diagnostic: { state: "unverified" as const },
  };

  const emptySnapshot = getConnectionFlowSnapshot([]);
  assert.equal(emptySnapshot.rootKind, "empty");
  assert.equal(emptySnapshot.canExpand, false);

  const afterCreateSnapshot = getConnectionFlowSnapshot([createdConnection], createdConnection);
  assert.equal(afterCreateSnapshot.rootKind, "connections");
  assert.equal(afterCreateSnapshot.canExpand, false);

  const connected = { ...createdConnection, isActive: true, isConnected: true };
  const afterConnectSnapshot = getConnectionFlowSnapshot([connected], connected);
  assert.equal(afterConnectSnapshot.rootKind, "connections");
  assert.equal(afterConnectSnapshot.canExpand, true);
  assert.equal(shouldExpandConnectionToDatabases(connected), true);

  const disconnected = { ...connected, isActive: false, isConnected: false };
  const afterDisconnectSnapshot = getConnectionFlowSnapshot([disconnected], disconnected);
  assert.equal(afterDisconnectSnapshot.rootKind, "connections");
  assert.equal(afterDisconnectSnapshot.canExpand, false);
  assert.equal(shouldExpandConnectionToDatabases(disconnected), false);
});