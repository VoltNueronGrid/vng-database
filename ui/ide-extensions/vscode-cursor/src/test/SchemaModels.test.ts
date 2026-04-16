import test from "node:test";
import assert from "node:assert/strict";
import { getColumnTypeDisplay, parseColumnType } from "../models/Schema";

test("parseColumnType handles numeric, textual, boolean, temporal, json, and binary families", () => {
  assert.equal(parseColumnType("BIGINT"), "BIGINT");
  assert.equal(parseColumnType("smallint"), "SMALLINT");
  assert.equal(parseColumnType("integer"), "INT");
  assert.equal(parseColumnType("numeric(10,2)"), "DECIMAL");
  assert.equal(parseColumnType("float8"), "FLOAT");
  assert.equal(parseColumnType("double precision"), "DOUBLE");
  assert.equal(parseColumnType("varchar(255)"), "VARCHAR");
  assert.equal(parseColumnType("text"), "TEXT");
  assert.equal(parseColumnType("boolean"), "BOOLEAN");
  assert.equal(parseColumnType("date"), "DATE");
  assert.equal(parseColumnType("timestamp with time zone"), "TIMESTAMP");
  assert.equal(parseColumnType("jsonb"), "JSON");
  assert.equal(parseColumnType("bytea"), "BYTEA");
  assert.equal(parseColumnType("geography"), "UNKNOWN");
});

test("getColumnTypeDisplay maps known types and safely falls back for unknown keys", () => {
  const jsonDisplay = getColumnTypeDisplay("JSON");
  assert.deepEqual(jsonDisplay, { icon: "$(json)", label: "JSON" });

  const fallbackDisplay = getColumnTypeDisplay("NOT_A_REAL_TYPE" as never);
  assert.deepEqual(fallbackDisplay, { icon: "$(symbol-misc)", label: "UNKNOWN" });
});