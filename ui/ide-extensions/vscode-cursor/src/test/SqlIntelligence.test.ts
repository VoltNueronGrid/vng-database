import test from "node:test";
import assert from "node:assert/strict";
import {
  buildSortText,
  extractAliases,
  getActiveClause,
  getAliasTarget,
  getCompletionContext,
  getSignatureContext,
  getSuggestionsFromDiagnosticMessage,
  levenshtein,
  normalizeIdentifier,
  resolveAliasOrTableReference,
  resolveTableReference,
  suggestNames,
  SchemaTableRef,
} from "../sql/SqlIntelligence";

const tables: SchemaTableRef[] = [
  {
    database: "default",
    schema: "public",
    table: "users",
    fullName: "default.public.users",
    columns: new Set(["id", "name", "active"]),
  },
  {
    database: "default",
    schema: "public",
    table: "orders",
    fullName: "default.public.orders",
    columns: new Set(["id", "user_id", "total"]),
  },
];

test("completion context and active clause detection map SQL clauses to table/column priorities", () => {
  assert.equal(getActiveClause("select id from users where "), "WHERE");
  assert.equal(getCompletionContext("select "), "column");
  assert.equal(getCompletionContext("from "), "table");
  assert.equal(getCompletionContext("u."), "column");
  assert.equal(getCompletionContext("with cte as (select 1) "), "column");
});

test("alias extraction and table resolution support qualified and aliased references", () => {
  const aliases = extractAliases("select * from public.users u join default.public.orders as o on u.id = o.user_id", tables);
  assert.ok(aliases.has("u"));
  assert.ok(aliases.has("o"));

  const aliasResolved = resolveAliasOrTableReference("u", aliases, tables);
  assert.equal(aliasResolved?.fullName, "default.public.users");

  const qualifiedResolved = resolveTableReference("default.public.orders", tables);
  assert.equal(qualifiedResolved?.table, "orders");

  const schemaResolved = resolveTableReference("public.users", tables);
  assert.equal(schemaResolved?.table, "users");
});

test("signature, sort text, and fuzzy suggestions are deterministic", () => {
  const signatureContext = getSignatureContext("select coalesce(name, ");
  assert.equal(signatureContext?.functionName, "COALESCE");
  assert.equal(signatureContext?.activeParameter, 1);

  assert.equal(getAliasTarget("u."), "u");
  assert.equal(normalizeIdentifier('"public".`users`'), "public.users");
  assert.match(buildSortText("column", "SELECT", "name"), /^01_/);
  assert.equal(levenshtein("users", "user"), 1);

  const prefixSuggestions = suggestNames("us", ["users", "usage", "orders"]);
  assert.deepEqual(prefixSuggestions, ["users", "usage"]);

  const fuzzySuggestions = suggestNames("usr", ["users", "orders", "usage"]);
  assert.equal(fuzzySuggestions[0], "users");

  const extracted = getSuggestionsFromDiagnosticMessage("Unknown table 'usr'. Suggestions: users, usage");
  assert.deepEqual(extracted, ["users", "usage"]);
  assert.deepEqual(getSuggestionsFromDiagnosticMessage("No hints available"), []);
});