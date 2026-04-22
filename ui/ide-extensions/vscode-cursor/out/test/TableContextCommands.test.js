"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const TableContextSql_1 = require("../commands/TableContextSql");
const table = {
    name: "orders",
    schema: "public",
    columns: [
        {
            name: "id",
            type: "BIGINT",
            nullable: false,
            isPrimaryKey: true,
            isUnique: true,
            isForeignKey: false,
        },
        {
            name: "status",
            type: "VARCHAR",
            nullable: false,
            isPrimaryKey: false,
            isUnique: false,
            isForeignKey: false,
        },
    ],
    indexes: [],
};
(0, node_test_1.default)("generateTruncateTableSql produces fully-qualified truncate statement", () => {
    strict_1.default.equal((0, TableContextSql_1.generateTruncateTableSql)(table), 'TRUNCATE TABLE "public"."orders";');
});
(0, node_test_1.default)("generateDeleteTemplate prefers primary key in where clause", () => {
    strict_1.default.equal((0, TableContextSql_1.generateDeleteTemplate)(table), 'DELETE FROM "public"."orders"\nWHERE "id" = ?;');
});
(0, node_test_1.default)("generateUpdateTemplate returns guidance when table has only primary keys", () => {
    const pkOnlyTable = {
        ...table,
        columns: [table.columns[0]],
    };
    const template = (0, TableContextSql_1.generateUpdateTemplate)(pkOnlyTable);
    strict_1.default.match(template, /No mutable columns found/);
    strict_1.default.match(template, /only primary key columns/);
});
(0, node_test_1.default)("generateMockData emits requested number of insert statements", () => {
    const mockSql = (0, TableContextSql_1.generateMockData)(table, 3);
    const lines = mockSql.split("\n").filter((line) => line.trim().length > 0);
    strict_1.default.equal(lines.length, 3);
    strict_1.default.ok(lines.every((line) => line.startsWith('INSERT INTO "public"."orders"')));
});
//# sourceMappingURL=TableContextCommands.test.js.map