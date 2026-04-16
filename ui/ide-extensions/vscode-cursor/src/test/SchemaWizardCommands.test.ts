import test from "node:test";
import assert from "node:assert/strict";
import { buildAlterTableDDL, buildCreateTableDDL } from "../commands/SchemaWizardSql";
import { parseColumnType } from "../models/Schema";

test("parseColumnType keeps bigint and smallint fidelity", () => {
  assert.equal(parseColumnType("BIGINT"), "BIGINT");
  assert.equal(parseColumnType("SMALLINT"), "SMALLINT");
  assert.equal(parseColumnType("INT"), "INT");
});

test("buildCreateTableDDL creates deterministic column definitions", () => {
  const ddl = buildCreateTableDDL({
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

  assert.match(ddl, /CREATE TABLE "public"\."orders"/);
  assert.match(ddl, /"id" BIGINT NOT NULL PRIMARY KEY/);
  assert.match(ddl, /"amount" DECIMAL NOT NULL DEFAULT 0/);
});

test("buildAlterTableDDL emits add-column and rename statements", () => {
  const addColumnDdl = buildAlterTableDDL({
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

  const renameDdl = buildAlterTableDDL({
    kind: "renameTable",
    schema: "public",
    tableName: "orders",
    newTableName: "customer_orders",
  });

  assert.equal(addColumnDdl, 'ALTER TABLE "public"."orders" ADD COLUMN "status" VARCHAR NOT NULL DEFAULT \'new\';');
  assert.equal(renameDdl, 'ALTER TABLE "public"."orders" RENAME TO "customer_orders";');
});
