import test from "node:test";
import assert from "node:assert/strict";
import { exportAsCSV, exportAsInsertSQL, exportAsJSON, parseQueryResult } from "../models/QueryResult";

test("parseQueryResult infers columns from array responses", () => {
  const result = parseQueryResult(
    "select id, active from users;",
    [
      { id: 1, active: true },
      { id: 2, active: false },
    ],
    12
  );

  assert.equal(result.status, "success");
  assert.equal(result.rowCount, 2);
  assert.deepEqual(result.columns, [
    { name: "id", type: "number", index: 0 },
    { name: "active", type: "boolean", index: 1 },
  ]);
});

test("parseQueryResult infers columns from object payload rows when metadata is absent", () => {
  const result = parseQueryResult(
    "select name from users;",
    {
      rows: [{ name: "Ada", quota: 4.5 }],
    },
    8
  );

  assert.deepEqual(result.columns, [
    { name: "name", type: "string", index: 0 },
    { name: "quota", type: "number", index: 1 },
  ]);
});

test("exportAsCSV escapes quotes and leaves null fields empty", () => {
  const csv = exportAsCSV({
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

  assert.equal(csv, '"name","note"\n"Ada ""Admin""",\n"Grace","ready"');
});

test("exportAsJSON formats rows with stable indentation", () => {
  const json = exportAsJSON({
    id: "result-2",
    query: "select * from users;",
    status: "success",
    columns: [],
    rows: [{ id: 1, name: "Ada" }],
    rowCount: 1,
    executionTime: 3,
    timestamp: 1,
  });

  assert.equal(json, '[\n  {\n    "id": 1,\n    "name": "Ada"\n  }\n]');
});

test("exportAsInsertSQL handles strings, booleans, and null values", () => {
  const sql = exportAsInsertSQL(
    {
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
    },
    "users"
  );

  assert.equal(sql, "INSERT INTO users (name, active, note) VALUES ('O''Hara', true, NULL);");
});