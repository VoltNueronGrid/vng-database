"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const SqlIntelligence_1 = require("../sql/SqlIntelligence");
const tables = [
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
(0, node_test_1.default)("completion context and active clause detection map SQL clauses to table/column priorities", () => {
    strict_1.default.equal((0, SqlIntelligence_1.getActiveClause)("select id from users where "), "WHERE");
    strict_1.default.equal((0, SqlIntelligence_1.getCompletionContext)("select "), "column");
    strict_1.default.equal((0, SqlIntelligence_1.getCompletionContext)("from "), "table");
    strict_1.default.equal((0, SqlIntelligence_1.getCompletionContext)("u."), "column");
    strict_1.default.equal((0, SqlIntelligence_1.getCompletionContext)("with cte as (select 1) "), "column");
});
(0, node_test_1.default)("alias extraction and table resolution support qualified and aliased references", () => {
    const aliases = (0, SqlIntelligence_1.extractAliases)("select * from public.users u join default.public.orders as o on u.id = o.user_id", tables);
    strict_1.default.ok(aliases.has("u"));
    strict_1.default.ok(aliases.has("o"));
    const aliasResolved = (0, SqlIntelligence_1.resolveAliasOrTableReference)("u", aliases, tables);
    strict_1.default.equal(aliasResolved?.fullName, "default.public.users");
    const qualifiedResolved = (0, SqlIntelligence_1.resolveTableReference)("default.public.orders", tables);
    strict_1.default.equal(qualifiedResolved?.table, "orders");
    const schemaResolved = (0, SqlIntelligence_1.resolveTableReference)("public.users", tables);
    strict_1.default.equal(schemaResolved?.table, "users");
});
(0, node_test_1.default)("signature, sort text, and fuzzy suggestions are deterministic", () => {
    const signatureContext = (0, SqlIntelligence_1.getSignatureContext)("select coalesce(name, ");
    strict_1.default.equal(signatureContext?.functionName, "COALESCE");
    strict_1.default.equal(signatureContext?.activeParameter, 1);
    strict_1.default.equal((0, SqlIntelligence_1.getAliasTarget)("u."), "u");
    strict_1.default.equal((0, SqlIntelligence_1.normalizeIdentifier)('"public".`users`'), "public.users");
    strict_1.default.match((0, SqlIntelligence_1.buildSortText)("column", "SELECT", "name"), /^01_/);
    strict_1.default.equal((0, SqlIntelligence_1.levenshtein)("users", "user"), 1);
    const prefixSuggestions = (0, SqlIntelligence_1.suggestNames)("us", ["users", "usage", "orders"]);
    strict_1.default.deepEqual(prefixSuggestions, ["users", "usage"]);
    const fuzzySuggestions = (0, SqlIntelligence_1.suggestNames)("usr", ["users", "orders", "usage"]);
    strict_1.default.equal(fuzzySuggestions[0], "users");
    const extracted = (0, SqlIntelligence_1.getSuggestionsFromDiagnosticMessage)("Unknown table 'usr'. Suggestions: users, usage");
    strict_1.default.deepEqual(extracted, ["users", "usage"]);
    strict_1.default.deepEqual((0, SqlIntelligence_1.getSuggestionsFromDiagnosticMessage)("No hints available"), []);
});
//# sourceMappingURL=SqlIntelligence.test.js.map