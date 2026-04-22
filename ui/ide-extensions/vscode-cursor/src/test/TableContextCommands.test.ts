import test from "node:test";
import assert from "node:assert/strict";

import {
  generateDeleteTemplate,
  generateMockData,
  generateTruncateTableSql,
  generateUpdateTemplate,
} from "../commands/TableContextSql";
import { Table } from "../models/Schema";

const table: Table = {
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

test("generateTruncateTableSql produces fully-qualified truncate statement", () => {
  assert.equal(generateTruncateTableSql(table), 'TRUNCATE TABLE "public"."orders";');
});

test("generateDeleteTemplate prefers primary key in where clause", () => {
  assert.equal(generateDeleteTemplate(table), 'DELETE FROM "public"."orders"\nWHERE "id" = ?;');
});

test("generateUpdateTemplate returns guidance when table has only primary keys", () => {
  const pkOnlyTable: Table = {
    ...table,
    columns: [table.columns[0]],
  };

  const template = generateUpdateTemplate(pkOnlyTable);
  assert.match(template, /No mutable columns found/);
  assert.match(template, /only primary key columns/);
});

test("generateMockData emits requested number of insert statements", () => {
  const mockSql = generateMockData(table, 3);
  const lines = mockSql.split("\n").filter((line) => line.trim().length > 0);
  assert.equal(lines.length, 3);
  assert.ok(lines.every((line) => line.startsWith('INSERT INTO "public"."orders"')));
});
