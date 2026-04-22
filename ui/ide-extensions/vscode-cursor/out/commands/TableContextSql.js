"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.generateUpdateTemplate = generateUpdateTemplate;
exports.generateDeleteTemplate = generateDeleteTemplate;
exports.generateTruncateTableSql = generateTruncateTableSql;
exports.generateMockData = generateMockData;
function generateUpdateTemplate(table) {
    const mutableColumns = table.columns
        .filter((column) => !column.isPrimaryKey)
        .map((column) => `"${column.name}" = ?`);
    if (mutableColumns.length === 0) {
        return `-- No mutable columns found on "${table.schema}"."${table.name}"\n-- Table appears to contain only primary key columns.`;
    }
    const sets = mutableColumns.join(",\n  ");
    const pk = table.columns.find((column) => column.isPrimaryKey);
    const whereClause = pk ? `WHERE "${pk.name}" = ?;` : "WHERE 1=1;";
    return `UPDATE "${table.schema}"."${table.name}"\nSET ${sets}\n${whereClause}`;
}
function generateDeleteTemplate(table) {
    const pk = table.columns.find((column) => column.isPrimaryKey);
    const whereClause = pk ? `WHERE "${pk.name}" = ?;` : "WHERE 1=1;";
    return `DELETE FROM "${table.schema}"."${table.name}"\n${whereClause}`;
}
function generateTruncateTableSql(table) {
    return `TRUNCATE TABLE "${table.schema}"."${table.name}";`;
}
function generateMockData(table, rowCount = 5) {
    const mockValueGetter = (column) => {
        const type = column.type.toUpperCase();
        if (type.includes("INT") || type.includes("BIGINT")) {
            return Math.floor(Math.random() * 10000).toString();
        }
        if (type.includes("FLOAT") || type.includes("DOUBLE") || type.includes("DECIMAL")) {
            return (Math.random() * 100).toFixed(2);
        }
        if (type === "BOOLEAN" || type.includes("BOOL")) {
            return Math.random() > 0.5 ? "true" : "false";
        }
        if (type.includes("DATE")) {
            return `'${new Date(Date.now() - Math.random() * 365 * 24 * 60 * 60 * 1000).toISOString().split("T")[0]}'`;
        }
        if (type.includes("TIMESTAMP")) {
            return `'${new Date().toISOString()}'`;
        }
        if (type.includes("VARCHAR") || type === "TEXT") {
            return `'Sample Data ${Math.random().toString(36).substring(7)}'`;
        }
        return "NULL";
    };
    const inserts = [];
    const columns = table.columns.map((column) => `"${column.name}"`).join(", ");
    for (let index = 0; index < rowCount; index++) {
        const values = table.columns.map((column) => mockValueGetter(column)).join(", ");
        inserts.push(`INSERT INTO "${table.schema}"."${table.name}" (${columns}) VALUES (${values});`);
    }
    return inserts.join("\n");
}
//# sourceMappingURL=TableContextSql.js.map