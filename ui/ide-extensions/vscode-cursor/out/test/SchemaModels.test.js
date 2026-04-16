"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const Schema_1 = require("../models/Schema");
(0, node_test_1.default)("parseColumnType handles numeric, textual, boolean, temporal, json, and binary families", () => {
    strict_1.default.equal((0, Schema_1.parseColumnType)("BIGINT"), "BIGINT");
    strict_1.default.equal((0, Schema_1.parseColumnType)("smallint"), "SMALLINT");
    strict_1.default.equal((0, Schema_1.parseColumnType)("integer"), "INT");
    strict_1.default.equal((0, Schema_1.parseColumnType)("numeric(10,2)"), "DECIMAL");
    strict_1.default.equal((0, Schema_1.parseColumnType)("float8"), "FLOAT");
    strict_1.default.equal((0, Schema_1.parseColumnType)("double precision"), "DOUBLE");
    strict_1.default.equal((0, Schema_1.parseColumnType)("varchar(255)"), "VARCHAR");
    strict_1.default.equal((0, Schema_1.parseColumnType)("text"), "TEXT");
    strict_1.default.equal((0, Schema_1.parseColumnType)("boolean"), "BOOLEAN");
    strict_1.default.equal((0, Schema_1.parseColumnType)("date"), "DATE");
    strict_1.default.equal((0, Schema_1.parseColumnType)("timestamp with time zone"), "TIMESTAMP");
    strict_1.default.equal((0, Schema_1.parseColumnType)("jsonb"), "JSON");
    strict_1.default.equal((0, Schema_1.parseColumnType)("bytea"), "BYTEA");
    strict_1.default.equal((0, Schema_1.parseColumnType)("geography"), "UNKNOWN");
});
(0, node_test_1.default)("getColumnTypeDisplay maps known types and safely falls back for unknown keys", () => {
    const jsonDisplay = (0, Schema_1.getColumnTypeDisplay)("JSON");
    strict_1.default.deepEqual(jsonDisplay, { icon: "$(json)", label: "JSON" });
    const fallbackDisplay = (0, Schema_1.getColumnTypeDisplay)("NOT_A_REAL_TYPE");
    strict_1.default.deepEqual(fallbackDisplay, { icon: "$(symbol-misc)", label: "UNKNOWN" });
});
//# sourceMappingURL=SchemaModels.test.js.map