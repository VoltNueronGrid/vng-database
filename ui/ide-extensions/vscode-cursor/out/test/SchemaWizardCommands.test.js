"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const SchemaWizardSql_1 = require("../commands/SchemaWizardSql");
const Schema_1 = require("../models/Schema");
(0, node_test_1.default)("parseColumnType keeps bigint and smallint fidelity", () => {
    strict_1.default.equal((0, Schema_1.parseColumnType)("BIGINT"), "BIGINT");
    strict_1.default.equal((0, Schema_1.parseColumnType)("SMALLINT"), "SMALLINT");
    strict_1.default.equal((0, Schema_1.parseColumnType)("INT"), "INT");
});
(0, node_test_1.default)("buildCreateTableDDL creates deterministic column definitions", () => {
    const ddl = (0, SchemaWizardSql_1.buildCreateTableDDL)({
        schema: "public",
        tableName: "orders",
        columns: [
            {
                name: "id",
                type: "BIGINT",
                nullable: false,
                isPrimaryKey: true,
                isUnique: true,
            },
            {
                name: "amount",
                type: "DECIMAL",
                nullable: false,
                isPrimaryKey: false,
                isUnique: false,
                defaultValue: "0",
            },
        ],
    });
    strict_1.default.match(ddl, /CREATE TABLE "public"\."orders"/);
    strict_1.default.match(ddl, /"id" BIGINT NOT NULL PRIMARY KEY/);
    strict_1.default.match(ddl, /"amount" DECIMAL NOT NULL DEFAULT 0/);
});
(0, node_test_1.default)("buildAlterTableDDL emits add-column and rename statements", () => {
    const addColumnDdl = (0, SchemaWizardSql_1.buildAlterTableDDL)({
        kind: "addColumn",
        schema: "public",
        tableName: "orders",
        column: {
            name: "status",
            type: "VARCHAR",
            nullable: false,
            isPrimaryKey: false,
            isUnique: false,
            defaultValue: "'new'",
        },
    });
    const renameDdl = (0, SchemaWizardSql_1.buildAlterTableDDL)({
        kind: "renameTable",
        schema: "public",
        tableName: "orders",
        newTableName: "customer_orders",
    });
    strict_1.default.equal(addColumnDdl, 'ALTER TABLE "public"."orders" ADD COLUMN "status" VARCHAR NOT NULL DEFAULT \'new\';');
    strict_1.default.equal(renameDdl, 'ALTER TABLE "public"."orders" RENAME TO "customer_orders";');
});
//# sourceMappingURL=SchemaWizardCommands.test.js.map