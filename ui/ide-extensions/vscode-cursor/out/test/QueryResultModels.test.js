"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const QueryResult_1 = require("../models/QueryResult");
(0, node_test_1.default)("parseQueryResult infers columns from array responses", () => {
    const result = (0, QueryResult_1.parseQueryResult)("select id, active from users;", [
        { id: 1, active: true },
        { id: 2, active: false },
    ], 12);
    strict_1.default.equal(result.status, "success");
    strict_1.default.equal(result.rowCount, 2);
    strict_1.default.deepEqual(result.columns, [
        { name: "id", type: "number", index: 0 },
        { name: "active", type: "boolean", index: 1 },
    ]);
});
(0, node_test_1.default)("parseQueryResult infers columns from object payload rows when metadata is absent", () => {
    const result = (0, QueryResult_1.parseQueryResult)("select name from users;", {
        rows: [{ name: "Ada", quota: 4.5 }],
    }, 8);
    strict_1.default.deepEqual(result.columns, [
        { name: "name", type: "string", index: 0 },
        { name: "quota", type: "number", index: 1 },
    ]);
});
(0, node_test_1.default)("exportAsCSV escapes quotes and leaves null fields empty", () => {
    const csv = (0, QueryResult_1.exportAsCSV)({
        id: "result-1",
        query: "select * from users;",
        status: "success",
        columns: [
            { name: "name", type: "string", index: 0 },
            { name: "note", type: "string", index: 1 },
        ],
        rows: [
            { name: 'Ada "Admin"', note: null },
            { name: "Grace", note: "ready" },
        ],
        rowCount: 2,
        executionTime: 4,
        timestamp: 1,
    });
    strict_1.default.equal(csv, '"name","note"\n"Ada ""Admin""",\n"Grace","ready"');
});
(0, node_test_1.default)("exportAsJSON formats rows with stable indentation", () => {
    const json = (0, QueryResult_1.exportAsJSON)({
        id: "result-2",
        query: "select * from users;",
        status: "success",
        columns: [],
        rows: [{ id: 1, name: "Ada" }],
        rowCount: 1,
        executionTime: 3,
        timestamp: 1,
    });
    strict_1.default.equal(json, '[\n  {\n    "id": 1,\n    "name": "Ada"\n  }\n]');
});
(0, node_test_1.default)("exportAsInsertSQL handles strings, booleans, and null values", () => {
    const sql = (0, QueryResult_1.exportAsInsertSQL)({
        id: "result-3",
        query: "select * from users;",
        status: "success",
        columns: [
            { name: "name", type: "string", index: 0 },
            { name: "active", type: "boolean", index: 1 },
            { name: "note", type: "string", index: 2 },
        ],
        rows: [
            { name: "O'Hara", active: true, note: null },
        ],
        rowCount: 1,
        executionTime: 7,
        timestamp: 1,
    }, "users");
    strict_1.default.equal(sql, "INSERT INTO users (name, active, note) VALUES ('O''Hara', true, NULL);");
});
//# sourceMappingURL=QueryResultModels.test.js.map