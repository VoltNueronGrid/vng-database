"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.buildCreateTableDDL = buildCreateTableDDL;
exports.buildAlterTableDDL = buildAlterTableDDL;
const TableEditorSql_1 = require("../services/TableEditorSql");
function buildCreateTableDDL(draft) {
    const definitions = draft.columns
        .map((column) => {
        let definition = `${(0, TableEditorSql_1.quoteIdentifier)(column.name)} ${column.type}`;
        if (!column.nullable) {
            definition += " NOT NULL";
        }
        if (column.isPrimaryKey) {
            definition += " PRIMARY KEY";
        }
        if (column.isUnique && !column.isPrimaryKey) {
            definition += " UNIQUE";
        }
        if (column.defaultValue && column.defaultValue.trim().length > 0) {
            definition += ` DEFAULT ${column.defaultValue.trim()}`;
        }
        return definition;
    })
        .join(",\n  ");
    return `CREATE TABLE ${(0, TableEditorSql_1.quoteIdentifier)(draft.schema)}.${(0, TableEditorSql_1.quoteIdentifier)(draft.tableName)} (\n  ${definitions}\n);`;
}
function buildAlterTableDDL(operation) {
    if (operation.kind === "renameTable") {
        return `ALTER TABLE ${(0, TableEditorSql_1.quoteIdentifier)(operation.schema)}.${(0, TableEditorSql_1.quoteIdentifier)(operation.tableName)} RENAME TO ${(0, TableEditorSql_1.quoteIdentifier)(operation.newTableName)};`;
    }
    let columnDefinition = `${(0, TableEditorSql_1.quoteIdentifier)(operation.column.name)} ${operation.column.type}`;
    if (!operation.column.nullable) {
        columnDefinition += " NOT NULL";
    }
    if (operation.column.defaultValue && operation.column.defaultValue.trim().length > 0) {
        columnDefinition += ` DEFAULT ${operation.column.defaultValue.trim()}`;
    }
    return `ALTER TABLE ${(0, TableEditorSql_1.quoteIdentifier)(operation.schema)}.${(0, TableEditorSql_1.quoteIdentifier)(operation.tableName)} ADD COLUMN ${columnDefinition};`;
}
//# sourceMappingURL=SchemaWizardSql.js.map