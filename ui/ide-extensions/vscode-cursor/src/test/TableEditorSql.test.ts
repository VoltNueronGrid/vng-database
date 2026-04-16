import test from "node:test";
import assert from "node:assert/strict";
import { Column, Table } from "../models/Schema";
import {
  buildDeleteStatement,
  buildInsertStatement,
  buildSelectPageSql,
  buildUpdateStatement,
  deriveTableEditorCapabilities,
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