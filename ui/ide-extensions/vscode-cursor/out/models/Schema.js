"use strict";
/**
 * Database schema models: Database, Schema, Table, Column
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.parseColumnType = parseColumnType;
exports.getColumnTypeDisplay = getColumnTypeDisplay;
/**
 * Parse column type string to ColumnType
 */
function parseColumnType(typeStr) {
    const normalized = typeStr.toUpperCase().trim();
    if (normalized.includes("BIGINT"))
        return "BIGINT";
    if (normalized.includes("SMALLINT"))
        return "SMALLINT";
    if (normalized.includes("INT"))
        return "INT";
    if (normalized.includes("DECIMAL") || normalized.includes("NUMERIC"))
        return "DECIMAL";
    if (normalized.includes("FLOAT"))
        return "FLOAT";
    if (normalized.includes("DOUBLE"))
        return "DOUBLE";
    if (normalized.includes("VARCHAR") || normalized.includes("STRING"))
        return "VARCHAR";
    if (normalized.includes("TEXT"))
        return "TEXT";
    if (normalized.includes("BOOL"))
        return "BOOLEAN";
    if (normalized.includes("DATE"))
        return "DATE";
    if (normalized.includes("TIMESTAMP"))
        return "TIMESTAMP";
    if (normalized.includes("JSON"))
        return "JSON";
    if (normalized.includes("BYTE"))
        return "BYTEA";
    return "UNKNOWN";
}
/**
 * Get column type icon/display
 */
function getColumnTypeDisplay(type) {
    const map = {
        INT: { icon: "$(symbol-number)", label: "INT" },
        BIGINT: { icon: "$(symbol-number)", label: "BIGINT" },
        SMALLINT: { icon: "$(symbol-number)", label: "SMALLINT" },
        DECIMAL: { icon: "$(symbol-number)", label: "DECIMAL" },
        FLOAT: { icon: "$(symbol-number)", label: "FLOAT" },
        DOUBLE: { icon: "$(symbol-number)", label: "DOUBLE" },
        VARCHAR: { icon: "$(symbol-string)", label: "VARCHAR" },
        TEXT: { icon: "$(symbol-string)", label: "TEXT" },
        BOOLEAN: { icon: "$(symbol-boolean)", label: "BOOLEAN" },
        DATE: { icon: "$(symbol-misc)", label: "DATE" },
        TIMESTAMP: { icon: "$(symbol-misc)", label: "TIMESTAMP" },
        JSON: { icon: "$(json)", label: "JSON" },
        BYTEA: { icon: "$(file-binary)", label: "BYTEA" },
        UNKNOWN: { icon: "$(symbol-misc)", label: "UNKNOWN" },
    };
    return map[type] || map.UNKNOWN;
}
//# sourceMappingURL=Schema.js.map