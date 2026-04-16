"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const Connection_1 = require("../models/Connection");
(0, node_test_1.default)("validateConnectionSettings requires operator identity for operator mode", () => {
    const validationError = (0, Connection_1.validateConnectionSettings)({
        name: "Operator",
        host: "127.0.0.1",
        port: 8080,
        baseUrl: "http://127.0.0.1:8080",
        mode: "operator",
        ssl: { enabled: false },
        advanced: {},
    });
    strict_1.default.equal(validationError, "Operator ID required for operator mode");
});
(0, node_test_1.default)("validateConnectionSettings rejects empty SSL certificate paths and invalid advanced values", () => {
    const sslValidationError = (0, Connection_1.validateConnectionSettings)({
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
    const timeoutValidationError = (0, Connection_1.validateConnectionSettings)({
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
    strict_1.default.equal(sslValidationError, "SSL certificate paths cannot be empty");
    strict_1.default.equal(timeoutValidationError, "Connection timeout must be greater than 0");
});
(0, node_test_1.default)("createDefaultConnection applies overrides while preserving database-client defaults", () => {
    const connection = (0, Connection_1.createDefaultConnection)({
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
    strict_1.default.match(connection.id, /^conn-/);
    strict_1.default.equal(connection.name, "Staging");
    strict_1.default.equal(connection.runtimeTarget, "cloud");
    strict_1.default.equal(connection.mode, "tenant");
    strict_1.default.equal(connection.baseUrl, "http://127.0.0.1:8080");
    strict_1.default.equal(connection.host, "127.0.0.1");
    strict_1.default.equal(connection.port, 8080);
    strict_1.default.deepEqual(connection.ssl, {
        enabled: true,
        caPath: "/certs/ca.pem",
        rejectUnauthorized: true,
    });
    strict_1.default.deepEqual(connection.advanced, {
        connectionTimeout: 15000,
        idleTimeout: 60000,
        keepAlive: false,
        maxConnections: 4,
    });
    strict_1.default.ok(connection.createdAt > 0);
});
//# sourceMappingURL=ConnectionModels.test.js.map