import test from "node:test";
import assert from "node:assert/strict";
import { Column, Table } from "../models/Schema";
import {
  buildDeleteStatement,
  buildInsertStatement,
  buildSelectPageSql,
  buildUpdateStatement,
  countPendingChanges,
  deriveTableEditorCapabilities,
  encodeSqlValue,
  hasAnyRowValue,
  quoteIdentifier,
  toEditorValue,
  validateDraftRow,
  validateColumnInput,
} from "../services/TableEditorSql";
import { TableEditorRow, TableEditorTarget } from "../models/TableEditor";

const columns: Column[] = [
  { name: "id", type: "INT", nullable: false, isPrimaryKey: true, isUnique: true, isForeignKey: false },
  { name: "name", type: "VARCHAR", nullable: false, isPrimaryKey: false, isUnique: false, isForeignKey: false },
  { name: "active", type: "BOOLEAN", nullable: false, isPrimaryKey: false, isUnique: false, isForeignKey: false },
  { name: "metadata", type: "JSON", nullable: true, isPrimaryKey: false, isUnique: false, isForeignKey: false },
];

const table: Table = {
  name: "users",
  schema: "public",
  columns,
  indexes: [{ name: "users_pkey", columns: ["id"], isUnique: true, isPrimary: true }],
};

const target: TableEditorTarget = {
  database: "default",
  schema: "public",
  tableName: "users",
};

test("deriveTableEditorCapabilities prefers primary key columns", () => {
  const capabilities = deriveTableEditorCapabilities(table);
  assert.deepEqual(capabilities.keyColumns, ["id"]);
  assert.equal(capabilities.canUpdate, true);
  assert.equal(capabilities.canDelete, true);
});

test("buildSelectPageSql adds pagination window", () => {
  const sql = buildSelectPageSql(target, columns, 2, 50);
  assert.match(sql, /LIMIT 51 OFFSET 50;/);
  assert.match(sql, /FROM "public"\."users"/);
});

test("buildInsertStatement encodes booleans and JSON safely", () => {
  const row: TableEditorRow = {
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

  const sql = buildInsertStatement(target, table, row);
  assert.match(sql, /VALUES \(42, 'O''Hara', true, '\{"role":"admin"\}'\);$/);
});

test("buildUpdateStatement only includes changed non-key columns", () => {
  const row: TableEditorRow = {
    rowId: "existing-1",
    kind: "existing",
    isDeleted: false,
    values: { id: "7", name: "Delta", active: "false", metadata: "" },
    originalValues: { id: "7", name: "Alpha", active: "true", metadata: "" },
  };

  const capabilities = deriveTableEditorCapabilities(table);
  const sql = buildUpdateStatement(target, table, row, capabilities);
  assert.ok(sql);
  assert.match(sql ?? "", /SET "name" = 'Delta', "active" = false/);
  assert.match(sql ?? "", /WHERE "id" = 7;/);
});

test("buildDeleteStatement uses key columns", () => {
  const row: TableEditorRow = {
    rowId: "existing-2",
    kind: "existing",
    isDeleted: true,
    values: { id: "99", name: "", active: "false", metadata: "" },
    originalValues: { id: "99", name: "Legacy", active: "true", metadata: "" },
  };

  const capabilities = deriveTableEditorCapabilities(table);
  const sql = buildDeleteStatement(target, table, row, capabilities);
  assert.equal(sql, 'DELETE FROM "public"."users" WHERE "id" = 99;');
});

test("validateColumnInput detects invalid boolean values", () => {
  const activeColumn = columns.find((column) => column.name === "active");
  assert.ok(activeColumn);
  const message = validateColumnInput(activeColumn!, "enabled");
  assert.equal(message, "must be true/false");
});

test("validateColumnInput accepts nullable empty values", () => {
  const metadataColumn = columns.find((column) => column.name === "metadata");
  assert.ok(metadataColumn);
  const message = validateColumnInput(metadataColumn!, "");
  assert.equal(message, undefined);
});

test("validateColumnInput rejects malformed JSON values", () => {
  const metadataColumn = columns.find((column) => column.name === "metadata");
  assert.ok(metadataColumn);
  const message = validateColumnInput(metadataColumn!, "{bad json}");
  assert.equal(message, "must be valid JSON");
});

test("quoteIdentifier and toEditorValue handle escaping and object conversion", () => {
  assert.equal(quoteIdentifier('weird"name'), '"weird""name"');
  assert.equal(toEditorValue(42), "42");
  assert.equal(toEditorValue({ active: true }), '{"active":true}');

  const circular: Record<string, unknown> = {};
  circular.self = circular;
  assert.match(toEditorValue(circular), /\[object Object\]/);
});

test("validateDraftRow enforces required fields and skips binary columns", () => {
  const tableWithBinary: Table = {
    ...table,
    columns: [
      ...columns,
      { name: "blob_data", type: "BYTEA", nullable: false, isPrimaryKey: false, isUnique: false, isForeignKey: false },
    ],
  };

  const row: TableEditorRow = {
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

  const errors = validateDraftRow(tableWithBinary, row);
  assert.ok(errors.some((entry) => entry.includes("Column 'id' is required.")));
  assert.ok(errors.some((entry) => entry.includes("Column 'name' is required.")));
  assert.ok(errors.some((entry) => entry.includes("must be true/false")));
  assert.ok(!errors.some((entry) => entry.includes("blob_data")));
});

test("countPendingChanges and hasAnyRowValue reflect draft/update/delete semantics", () => {
  const capabilities = deriveTableEditorCapabilities(table);
  const rows: TableEditorRow[] = [
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

  assert.equal(hasAnyRowValue(rows[0]), true);
  assert.equal(countPendingChanges(rows, capabilities), 3);
});

test("buildUpdateStatement returns null for unchanged rows and throws when no key is available", () => {
  const unchangedRow: TableEditorRow = {
    rowId: "existing-same",
    kind: "existing",
    isDeleted: false,
    values: { id: "7", name: "Same", active: "true", metadata: "" },
    originalValues: { id: "7", name: "Same", active: "true", metadata: "" },
  };

  const capabilities = deriveTableEditorCapabilities(table);
  assert.equal(buildUpdateStatement(target, table, unchangedRow, capabilities), null);

  const noKeyCapabilities = { ...capabilities, keyColumns: [] };
  assert.throws(
    () => buildDeleteStatement(target, table, unchangedRow, noKeyCapabilities),
    /does not expose a key/
  );
});

test("encodeSqlValue covers nullable, defaults, numeric and binary validation errors", () => {
  const defaultedColumn: Column = {
    name: "status",
    type: "VARCHAR",
    nullable: false,
    isPrimaryKey: false,
    isUnique: false,
    isForeignKey: false,
    defaultValue: "'new'",
  };
  assert.equal(encodeSqlValue(defaultedColumn, ""), "'new'");

  const nullableInt: Column = {
    name: "quota",
    type: "INT",
    nullable: true,
    isPrimaryKey: false,
    isUnique: false,
    isForeignKey: false,
  };
  assert.equal(encodeSqlValue(nullableInt, "NULL"), "NULL");
  assert.throws(() => encodeSqlValue(nullableInt, "1.2"), /must be an integer/);

  const binaryColumn: Column = {
    name: "blob_data",
    type: "BYTEA",
    nullable: true,
    isPrimaryKey: false,
    isUnique: false,
    isForeignKey: false,
  };
  assert.throws(() => encodeSqlValue(binaryColumn, "ff"), /read-only/);
});