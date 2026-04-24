"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const Connection_1 = require("../models/Connection");
const ConnectionContextCommands_1 = require("../commands/ConnectionContextCommands");
(0, node_test_1.default)("getConnectionHostLabel derives host and port from baseUrl", () => {
    const connection = {
        id: "conn-1",
        settings: (0, Connection_1.createDefaultConnection)({
            id: "conn-1",
            name: "Local",
            baseUrl: "https://db.local:9443",
            host: "db.local",
            port: 9443,
        }),
        isActive: true,
        isConnected: true,
        diagnostic: { state: "verified" },
    };
    strict_1.default.equal((0, ConnectionContextCommands_1.getConnectionHostLabel)(connection), "db.local:9443");
});
(0, node_test_1.default)("toConnectionExportJson redacts admin key while preserving diagnostic", () => {
    const connection = {
        id: "conn-2",
        settings: (0, Connection_1.createDefaultConnection)({
            id: "conn-2",
            name: "Ops",
            mode: "admin",
            adminKey: "super-secret",
        }),
        isActive: false,
        isConnected: false,
        diagnostic: { state: "degraded", message: "timeout" },
    };
    const exported = JSON.parse((0, ConnectionContextCommands_1.toConnectionExportJson)(connection));
    strict_1.default.equal(exported.settings.adminKey, "<redacted>");
    strict_1.default.equal(exported.state, "degraded");
});
(0, node_test_1.default)("buildConnectionStatusSummary includes state and history count", () => {
    const connection = {
        id: "conn-3",
        settings: (0, Connection_1.createDefaultConnection)({
            id: "conn-3",
            name: "Tenant A",
            mode: "tenant",
            tenantId: "tenant-a",
            userId: "user-a",
        }),
        isActive: true,
        isConnected: false,
        diagnostic: { state: "degraded", message: "connect_failed" },
    };
    const lines = (0, ConnectionContextCommands_1.buildConnectionStatusSummary)(connection, 7);
    strict_1.default.equal(lines.includes("State: degraded"), true);
    strict_1.default.equal(lines.includes("History entries: 7"), true);
});
//# sourceMappingURL=ConnectionContextCommands.test.js.map