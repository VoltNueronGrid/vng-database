"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const TableEditorSql_1 = require("../services/TableEditorSql");
const columns = [
    { name: "id", type: "INT", nullable: false, isPrimaryKey: true, isUnique: true, isForeignKey: false },
    { name: "name", type: "VARCHAR", nullable: false, isPrimaryKey: false, isUnique: false, isForeignKey: false },
    { name: "active", type: "BOOLEAN", nullable: false, isPrimaryKey: false, isUnique: false, isForeignKey: false },
    { name: "metadata", type: "JSON", nullable: true, isPrimaryKey: false, isUnique: false, isForeignKey: false },
];
const table = {
    name: "users",
    schema: "public",
    columns,
    indexes: [{ name: "users_pkey", columns: ["id"], isUnique: true, isPrimary: true }],
};
const target = {
    database: "default",
    schema: "public",
    tableName: "users",
};
(0, node_test_1.default)("deriveTableEditorCapabilities prefers primary key columns", () => {
    const capabilities = (0, TableEditorSql_1.deriveTableEditorCapabilities)(table);
    strict_1.default.deepEqual(capabilities.keyColumns, ["id"]);
    strict_1.default.equal(capabilities.canUpdate, true);
    strict_1.default.equal(capabilities.canDelete, true);
});
(0, node_test_1.default)("buildSelectPageSql adds pagination window", () => {
    const sql = (0, TableEditorSql_1.buildSelectPageSql)(target, columns, 2, 50);
    strict_1.default.match(sql, /LIMIT 51 OFFSET 50;/);
    strict_1.default.match(sql, /FROM "public"\."users"/);
});
(0, node_test_1.default)("buildInsertStatement encodes booleans and JSON safely", () => {
    const row = {
        rowId: "draft-1",
        kind: "draft",
        isDeleted: false,
        values: {
            id: "42",
            name: "O'Hara",
            active: "true",
            metadata: '{"role":"admin"}',
        },
    };
    const sql = (0, TableEditorSql_1.buildInsertStatement)(target, table, row);
    strict_1.default.match(sql, /VALUES \(42, 'O''Hara', true, '\{"role":"admin"\}'\);$/);
});
(0, node_test_1.default)("buildUpdateStatement only includes changed non-key columns", () => {
    const row = {
        rowId: "existing-1",
        kind: "existing",
        isDeleted: false,
        values: { id: "7", name: "Delta", active: "false", metadata: "" },
        originalValues: { id: "7", name: "Alpha", active: "true", metadata: "" },
    };
    const capabilities = (0, TableEditorSql_1.deriveTableEditorCapabilities)(table);
    const sql = (0, TableEditorSql_1.buildUpdateStatement)(target, table, row, capabilities);
    strict_1.default.ok(sql);
    strict_1.default.match(sql ?? "", /SET "name" = 'Delta', "active" = false/);
    strict_1.default.match(sql ?? "", /WHERE "id" = 7;/);
});
(0, node_test_1.default)("buildDeleteStatement uses key columns", () => {
    const row = {
        rowId: "existing-2",
        kind: "existing",
        isDeleted: true,
        values: { id: "99", name: "", active: "false", metadata: "" },
        originalValues: { id: "99", name: "Legacy", active: "true", metadata: "" },
    };
    const capabilities = (0, TableEditorSql_1.deriveTableEditorCapabilities)(table);
    const sql = (0, TableEditorSql_1.buildDeleteStatement)(target, table, row, capabilities);
    strict_1.default.equal(sql, 'DELETE FROM "public"."users" WHERE "id" = 99;');
});
//# sourceMappingURL=TableEditorSql.test.js.map