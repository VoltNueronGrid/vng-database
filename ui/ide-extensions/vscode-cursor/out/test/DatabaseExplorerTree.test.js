"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const DatabaseExplorerTree_1 = require("../providers/DatabaseExplorerTree");
const Connection_1 = require("../models/Connection");
(0, node_test_1.default)("describeConnectionNode marks active connection state", () => {
    const connection = {
        id: "conn-1",
        settings: (0, Connection_1.createDefaultConnection)({ name: "Local Dev" }),
        isActive: true,
        isConnected: true,
        state: "verified",
        diagnostics: {},
    };
    const presentation = (0, DatabaseExplorerTree_1.describeConnectionNode)(connection);
    strict_1.default.equal(presentation.contextValue, "connectionActive");
    strict_1.default.match(presentation.description, /Active/);
    strict_1.default.match(presentation.description, /Verified/);
});
(0, node_test_1.default)("describeConnectionNode guides inactive browsing flow", () => {
    const connection = {
        id: "conn-2",
        settings: (0, Connection_1.createDefaultConnection)({ name: "Staging" }),
        isActive: false,
        isConnected: false,
        state: "active",
        diagnostics: {},
    };
    const presentation = (0, DatabaseExplorerTree_1.describeConnectionNode)(connection);
    strict_1.default.equal(presentation.contextValue, "connectionInactive");
    strict_1.default.match(presentation.description, /Not verified/);
    strict_1.default.equal(presentation.browseMessage, "Activate Staging to browse databases.");
});
(0, node_test_1.default)("connection flow covers empty -> create -> connect -> expand -> disconnect", () => {
    const createdConnection = {
        id: "conn-flow",
        settings: (0, Connection_1.createDefaultConnection)({ name: "Flow Connection" }),
        isActive: false,
        isConnected: false,
        state: "active",
        diagnostics: {},
    };
    const emptySnapshot = (0, DatabaseExplorerTree_1.getConnectionFlowSnapshot)([]);
    strict_1.default.equal(emptySnapshot.rootKind, "empty");
    strict_1.default.equal(emptySnapshot.canExpand, false);
    const afterCreateSnapshot = (0, DatabaseExplorerTree_1.getConnectionFlowSnapshot)([createdConnection], createdConnection);
    strict_1.default.equal(afterCreateSnapshot.rootKind, "connections");
    strict_1.default.equal(afterCreateSnapshot.canExpand, false);
    const connected = { ...createdConnection, isActive: true, isConnected: true, state: "verified" };
    const afterConnectSnapshot = (0, DatabaseExplorerTree_1.getConnectionFlowSnapshot)([connected], connected);
    strict_1.default.equal(afterConnectSnapshot.rootKind, "connections");
    strict_1.default.equal(afterConnectSnapshot.canExpand, true);
    strict_1.default.equal((0, DatabaseExplorerTree_1.shouldExpandConnectionToDatabases)(connected), true);
    const disconnected = { ...connected, isActive: false, isConnected: false, state: "degraded" };
    const afterDisconnectSnapshot = (0, DatabaseExplorerTree_1.getConnectionFlowSnapshot)([disconnected], disconnected);
    strict_1.default.equal(afterDisconnectSnapshot.rootKind, "connections");
    strict_1.default.equal(afterDisconnectSnapshot.canExpand, false);
    strict_1.default.equal((0, DatabaseExplorerTree_1.shouldExpandConnectionToDatabases)(disconnected), false);
});
(0, node_test_1.default)("groupConnectionsForTree groups by folder and keeps deterministic ordering", () => {
    const alpha = {
        id: "a",
        settings: (0, Connection_1.createDefaultConnection)({ name: "Alpha", group: "staging" }),
        isActive: false,
        isConnected: false,
        state: "active",
        diagnostics: {},
    };
    const beta = {
        id: "b",
        settings: (0, Connection_1.createDefaultConnection)({ name: "Beta", group: "prod" }),
        isActive: false,
        isConnected: false,
        state: "active",
        diagnostics: {},
    };
    const gamma = {
        id: "c",
        settings: (0, Connection_1.createDefaultConnection)({ name: "Gamma" }),
        isActive: false,
        isConnected: false,
        state: "active",
        diagnostics: {},
    };
    const grouped = (0, DatabaseExplorerTree_1.groupConnectionsForTree)([gamma, alpha, beta]);
    strict_1.default.deepEqual(grouped.map((bucket) => bucket.groupLabel), ["localmachine", "prod", "staging"]);
    strict_1.default.deepEqual(grouped[0]?.connections.map((connection) => connection.settings.name), ["Gamma"]);
    strict_1.default.deepEqual(grouped[1]?.connections.map((connection) => connection.settings.name), ["Beta"]);
    strict_1.default.deepEqual(grouped[2]?.connections.map((connection) => connection.settings.name), ["Alpha"]);
});
(0, node_test_1.default)("describeTableSections returns deterministic section order and counts", () => {
    const table = {
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
    const sections = (0, DatabaseExplorerTree_1.describeTableSections)(table);
    strict_1.default.deepEqual(sections.map((entry) => ({ kind: entry.kind, count: entry.count })), [
        { kind: "columns", count: 2 },
        { kind: "indexes", count: 1 },
        { kind: "triggers", count: 1 },
    ]);
});
(0, node_test_1.default)("describeTableRowCount formats known counts and hides missing values", () => {
    const table = {
        name: "events",
        schema: "public",
        columns: [],
        indexes: [],
        rowCount: 15230,
    };
    strict_1.default.equal((0, DatabaseExplorerTree_1.describeTableRowCount)(table), "~15.2K rows");
    strict_1.default.equal((0, DatabaseExplorerTree_1.describeTableRowCount)({ ...table, rowCount: 912 }), "912 rows");
    strict_1.default.equal((0, DatabaseExplorerTree_1.describeTableRowCount)({ ...table, rowCount: 2700000 }), "~2.7M rows");
    strict_1.default.equal((0, DatabaseExplorerTree_1.describeTableRowCount)({ ...table, rowCount: 4500000000 }), "~4.5B rows");
    strict_1.default.equal((0, DatabaseExplorerTree_1.describeTableRowCount)({ ...table, rowCount: undefined }), "");
    strict_1.default.equal((0, DatabaseExplorerTree_1.describeTableRowCount)({ ...table, rowCount: -1 }), "");
});
//# sourceMappingURL=DatabaseExplorerTree.test.js.map