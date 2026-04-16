import test from "node:test";
import assert from "node:assert/strict";
import { describeConnectionNode, getEmptyConnectionMessage } from "../providers/DatabaseExplorerTree";
import { createDefaultConnection } from "../models/Connection";

test("describeConnectionNode marks active connection state", () => {
  const connection = {
    id: "conn-1",
    settings: createDefaultConnection({ name: "Local Dev" }),
    isActive: true,
    isConnected: true,
  };

  const presentation = describeConnectionNode(connection);
  assert.equal(presentation.contextValue, "connectionActive");
  assert.match(presentation.description, /Active/);
  assert.match(presentation.description, /Connected/);
});

test("describeConnectionNode guides inactive browsing flow", () => {
  const connection = {
    id: "conn-2",
    settings: createDefaultConnection({ name: "Staging" }),
    isActive: false,
    isConnected: false,
  };

  const presentation = describeConnectionNode(connection);
  assert.equal(presentation.contextValue, "connectionInactive");
  assert.match(presentation.description, /Not verified/);
  assert.equal(presentation.browseMessage, "Activate Staging to browse databases.");
});

test("getEmptyConnectionMessage exposes create CTA copy", () => {
  assert.equal(getEmptyConnectionMessage(), "No connections available. Create New Connection.");
});