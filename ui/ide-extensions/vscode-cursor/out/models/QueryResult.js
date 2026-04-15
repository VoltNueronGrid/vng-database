"use strict";
/**
 * Query execution and result models
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.parseQueryResult = parseQueryResult;
exports.exportAsCSV = exportAsCSV;
exports.exportAsJSON = exportAsJSON;
exports.exportAsInsertSQL = exportAsInsertSQL;
/**
 * Parse query result from server response
 */
function parseQueryResult(query, data, executionTime) {
    const id = `result-${Date.now()}`;
    const timestamp = Date.now();
    // Handle different response formats
    let rows = [];
    let columns = [];
    if (Array.isArray(data)) {
        // Response is array of rows
        rows = data;
        // Infer columns from first row
        if (rows.length > 0) {
            const firstRow = rows[0];
            columns = Object.keys(firstRow).map((name, index) => ({
                name,
                type: typeof firstRow[name],
                index,
            }));
        }
    }
    else if (data && typeof data === "object") {
        // Response is object with data/rows and possibly columns
        rows = data.rows || data.data || [];
        columns = data.columns || [];
        if (columns.length === 0 && rows.length > 0) {
            const firstRow = rows[0];
            columns = Object.keys(firstRow).map((name, index) => ({
                name,
                type: typeof firstRow[name],
                index,
            }));
        }
    }
    return {
        id,
        query,
        status: "success",
        rows,
        columns,
        rowCount: rows.length,
        executionTime,
        timestamp,
    };
}
/**
 * Export result as CSV
 */
function exportAsCSV(result) {
    if (result.columns.length === 0) {
        return "";
    }
    // CSV header
    const header = result.columns.map((col) => `"${col.name.replace(/"/g, '""')}"`).join(",");
    // CSV rows
    const csvRows = result.rows.map((row) => result.columns
        .map((col) => {
        const value = row[col.name];
        if (value === null || value === undefined) {
            return "";
        }
        const strValue = String(value).replace(/"/g, '""');
        return `"${strValue}"`;
    })
        .join(","));
    return [header, ...csvRows].join("\n");
}
/**
 * Export result as JSON
 */
function exportAsJSON(result) {
    return JSON.stringify(result.rows, null, 2);
}
/**
 * Export result as INSERT statements
 */
function exportAsInsertSQL(result, tableName) {
    if (result.columns.length === 0 || result.rows.length === 0) {
        return "";
    }
    const columnNames = result.columns.map((col) => col.name).join(", ");
    const inserts = result.rows.map((row) => {
        const values = result.columns
            .map((col) => {
            const value = row[col.name];
            if (value === null || value === undefined) {
                return "NULL";
            }
            if (typeof value === "string") {
                return `'${value.replace(/'/g, "''")}'`;
            }
            if (typeof value === "boolean") {
                return value ? "true" : "false";
            }
            return String(value);
        })
            .join(", ");
        return `INSERT INTO ${tableName} (${columnNames}) VALUES (${values});`;
    });
    return inserts.join("\n");
}
//# sourceMappingURL=QueryResult.js.map