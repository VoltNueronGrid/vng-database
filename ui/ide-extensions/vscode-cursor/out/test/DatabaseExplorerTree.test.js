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
    };
    const presentation = (0, DatabaseExplorerTree_1.describeConnectionNode)(connection);
    strict_1.default.equal(presentation.contextValue, "connectionActive");
    strict_1.default.match(presentation.description, /Active/);
    strict_1.default.match(presentation.description, /Connected/);
});
(0, node_test_1.default)("describeConnectionNode guides inactive browsing flow", () => {
    const connection = {
        id: "conn-2",
        settings: (0, Connection_1.createDefaultConnection)({ name: "Staging" }),
        isActive: false,
        isConnected: false,
    };
    const presentation = (0, DatabaseExplorerTree_1.describeConnectionNode)(connection);
    strict_1.default.equal(presentation.contextValue, "connectionInactive");
    strict_1.default.match(presentation.description, /Not verified/);
    strict_1.default.equal(presentation.browseMessage, "Activate Staging to browse databases.");
});
(0, node_test_1.default)("getEmptyConnectionMessage exposes create CTA copy", () => {
    strict_1.default.equal((0, DatabaseExplorerTree_1.getEmptyConnectionMessage)(), "No connections available. Create New Connection.");
});
//# sourceMappingURL=DatabaseExplorerTree.test.js.map