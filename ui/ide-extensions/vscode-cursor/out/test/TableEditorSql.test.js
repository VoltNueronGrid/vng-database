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
(0, node_test_1.default)("validateColumnInput detects invalid boolean values", () => {
    const activeColumn = columns.find((column) => column.name === "active");
    strict_1.default.ok(activeColumn);
    const message = (0, TableEditorSql_1.validateColumnInput)(activeColumn, "enabled");
    strict_1.default.equal(message, "must be true/false");
});
(0, node_test_1.default)("validateColumnInput accepts nullable empty values", () => {
    const metadataColumn = columns.find((column) => column.name === "metadata");
    strict_1.default.ok(metadataColumn);
    const message = (0, TableEditorSql_1.validateColumnInput)(metadataColumn, "");
    strict_1.default.equal(message, undefined);
});
(0, node_test_1.default)("validateColumnInput rejects malformed JSON values", () => {
    const metadataColumn = columns.find((column) => column.name === "metadata");
    strict_1.default.ok(metadataColumn);
    const message = (0, TableEditorSql_1.validateColumnInput)(metadataColumn, "{bad json}");
    strict_1.default.equal(message, "must be valid JSON");
});
(0, node_test_1.default)("quoteIdentifier and toEditorValue handle escaping and object conversion", () => {
    strict_1.default.equal((0, TableEditorSql_1.quoteIdentifier)('weird"name'), '"weird""name"');
    strict_1.default.equal((0, TableEditorSql_1.toEditorValue)(42), "42");
    strict_1.default.equal((0, TableEditorSql_1.toEditorValue)({ active: true }), '{"active":true}');
    const circular = {};
    circular.self = circular;
    strict_1.default.match((0, TableEditorSql_1.toEditorValue)(circular), /\[object Object\]/);
});
(0, node_test_1.default)("validateDraftRow enforces required fields and skips binary columns", () => {
    const tableWithBinary = {
        ...table,
        columns: [
            ...columns,
            { name: "blob_data", type: "BYTEA", nullable: false, isPrimaryKey: false, isUnique: false, isForeignKey: false },
        ],
    };
    const row = {
        rowId: "draft-2",
        kind: "draft",
        isDeleted: false,
        values: {
            id: "",
            name: "",
            active: "maybe",
            metadata: "",
            blob_data: "",
        },
    };
    const errors = (0, TableEditorSql_1.validateDraftRow)(tableWithBinary, row);
    strict_1.default.ok(errors.some((entry) => entry.includes("Column 'id' is required.")));
    strict_1.default.ok(errors.some((entry) => entry.includes("Column 'name' is required.")));
    strict_1.default.ok(errors.some((entry) => entry.includes("must be true/false")));
    strict_1.default.ok(!errors.some((entry) => entry.includes("blob_data")));
});
(0, node_test_1.default)("countPendingChanges and hasAnyRowValue reflect draft/update/delete semantics", () => {
    const capabilities = (0, TableEditorSql_1.deriveTableEditorCapabilities)(table);
    const rows = [
        {
            rowId: "draft",
            kind: "draft",
            isDeleted: false,
            values: { id: "", name: "new", active: "true", metadata: "" },
        },
        {
            rowId: "existing-update",
            kind: "existing",
            isDeleted: false,
            values: { id: "1", name: "updated", active: "true", metadata: "" },
            originalValues: { id: "1", name: "old", active: "true", metadata: "" },
        },
        {
            rowId: "existing-delete",
            kind: "existing",
            isDeleted: true,
            values: { id: "2", name: "gone", active: "false", metadata: "" },
            originalValues: { id: "2", name: "gone", active: "false", metadata: "" },
        },
    ];
    strict_1.default.equal((0, TableEditorSql_1.hasAnyRowValue)(rows[0]), true);
    strict_1.default.equal((0, TableEditorSql_1.countPendingChanges)(rows, capabilities), 3);
});
(0, node_test_1.default)("buildUpdateStatement returns null for unchanged rows and throws when no key is available", () => {
    const unchangedRow = {
        rowId: "existing-same",
        kind: "existing",
        isDeleted: false,
        values: { id: "7", name: "Same", active: "true", metadata: "" },
        originalValues: { id: "7", name: "Same", active: "true", metadata: "" },
    };
    const capabilities = (0, TableEditorSql_1.deriveTableEditorCapabilities)(table);
    strict_1.default.equal((0, TableEditorSql_1.buildUpdateStatement)(target, table, unchangedRow, capabilities), null);
    const noKeyCapabilities = { ...capabilities, keyColumns: [] };
    strict_1.default.throws(() => (0, TableEditorSql_1.buildDeleteStatement)(target, table, unchangedRow, noKeyCapabilities), /does not expose a key/);
});
(0, node_test_1.default)("encodeSqlValue covers nullable, defaults, numeric and binary validation errors", () => {
    const defaultedColumn = {
        name: "status",
        type: "VARCHAR",
        nullable: false,
        isPrimaryKey: false,
        isUnique: false,
        isForeignKey: false,
        defaultValue: "'new'",
    };
    strict_1.default.equal((0, TableEditorSql_1.encodeSqlValue)(defaultedColumn, ""), "'new'");
    const nullableInt = {
        name: "quota",
        type: "INT",
        nullable: true,
        isPrimaryKey: false,
        isUnique: false,
        isForeignKey: false,
    };
    strict_1.default.equal((0, TableEditorSql_1.encodeSqlValue)(nullableInt, "NULL"), "NULL");
    strict_1.default.throws(() => (0, TableEditorSql_1.encodeSqlValue)(nullableInt, "1.2"), /must be an integer/);
    const binaryColumn = {
        name: "blob_data",
        type: "BYTEA",
        nullable: true,
        isPrimaryKey: false,
        isUnique: false,
        isForeignKey: false,
    };
    strict_1.default.throws(() => (0, TableEditorSql_1.encodeSqlValue)(binaryColumn, "ff"), /read-only/);
});
//# sourceMappingURL=TableEditorSql.test.js.map