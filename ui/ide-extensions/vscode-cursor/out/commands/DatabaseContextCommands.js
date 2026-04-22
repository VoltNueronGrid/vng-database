"use strict";
/**
 * Context Menu Commands for Database Explorer
 */
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.generateTableDDL = generateTableDDL;
exports.generateSelectTemplate = generateSelectTemplate;
exports.generateInsertTemplate = generateInsertTemplate;
exports.generateUpdateTemplate = generateUpdateTemplate;
exports.generateDeleteTemplate = generateDeleteTemplate;
exports.generateMockData = generateMockData;
exports.exportTableStructure = exportTableStructure;
exports.generateTruncateTableSql = generateTruncateTableSql;
exports.handleCopyName = handleCopyName;
exports.handleShowDDL = handleShowDDL;
exports.handleSQLTemplate = handleSQLTemplate;
exports.handleGenerateMockData = handleGenerateMockData;
exports.handleDumpStruct = handleDumpStruct;
exports.handleDropTable = handleDropTable;
exports.handleTruncateTable = handleTruncateTable;
const vscode = __importStar(require("vscode"));
const TableContextSql_1 = require("./TableContextSql");
function getTableFromItem(element) {
    if (element.type !== "table" || !element.data) {
        return undefined;
    }
    const maybeWrapped = element.data;
    if (maybeWrapped.table) {
        return maybeWrapped.table;
    }
    return element.data;
}
/**
 * Generate DDL for a table
 */
function generateTableDDL(table) {
    const columns = table.columns
        .map((col) => {
        let def = `"${col.name}" ${col.type}`;
        if (!col.nullable)
            def += " NOT NULL";
        if (col.isPrimaryKey)
            def += " PRIMARY KEY";
        if (col.isUnique)
            def += " UNIQUE";
        if (col.defaultValue)
            def += ` DEFAULT ${col.defaultValue}`;
        return def;
    })
        .join(",\n  ");
    return `CREATE TABLE "${table.schema}"."${table.name}" (\n  ${columns}\n);`;
}
/**
 * Generate SELECT template
 */
function generateSelectTemplate(table) {
    const columns = table.columns.map((c) => `"${c.name}"`).join(",\n  ");
    return `SELECT\n  ${columns}\nFROM "${table.schema}"."${table.name}"\nWHERE 1=1;`;
}
/**
 * Generate INSERT template
 */
function generateInsertTemplate(table) {
    const columns = table.columns.map((c) => `"${c.name}"`).join(", ");
    const placeholders = table.columns.map(() => "?").join(", ");
    return `INSERT INTO "${table.schema}"."${table.name}" (${columns})\nVALUES (${placeholders});`;
}
/**
 * Generate UPDATE template
 */
function generateUpdateTemplate(table) {
    return (0, TableContextSql_1.generateUpdateTemplate)(table);
}
/**
 * Generate DELETE template
 */
function generateDeleteTemplate(table) {
    return (0, TableContextSql_1.generateDeleteTemplate)(table);
}
/**
 * Generate mock INSERT statements
 */
function generateMockData(table, rowCount = 5) {
    return (0, TableContextSql_1.generateMockData)(table, rowCount);
}
/**
 * Export table structure as JSON
 */
function exportTableStructure(table) {
    return JSON.stringify({
        name: table.name,
        schema: table.schema,
        columns: table.columns.map((c) => ({
            name: c.name,
            type: c.type,
            nullable: c.nullable,
            primaryKey: c.isPrimaryKey,
            unique: c.isUnique,
            foreignKey: c.isForeignKey,
            default: c.defaultValue,
        })),
        indexes: table.indexes,
    }, null, 2);
}
/**
 * Generate TRUNCATE statement
 */
function generateTruncateTableSql(table) {
    return (0, TableContextSql_1.generateTruncateTableSql)(table);
}
/**
 * Handle "Copy Name" command
 */
async function handleCopyName(element) {
    if (element.type === "table" || element.type === "column") {
        await vscode.env.clipboard.writeText(element.label);
        vscode.window.showInformationMessage(`Copied: ${element.label}`);
    }
}
/**
 * Handle "Show DDL" command
 */
async function handleShowDDL(element) {
    if (element.type === "table") {
        const table = getTableFromItem(element);
        if (!table) {
            vscode.window.showErrorMessage("Unable to resolve table metadata.");
            return;
        }
        const ddl = generateTableDDL(table);
        await showInQuickPick("Table DDL", ddl);
    }
}
/**
 * Handle "SQL Template" command
 */
async function handleSQLTemplate(element) {
    if (element.type === "table") {
        const table = getTableFromItem(element);
        if (!table) {
            vscode.window.showErrorMessage("Unable to resolve table metadata.");
            return;
        }
        const items = [
            { label: "SELECT", detail: "Generate SELECT statement", value: generateSelectTemplate(table) },
            { label: "INSERT", detail: "Generate INSERT statement", value: generateInsertTemplate(table) },
            { label: "UPDATE", detail: "Generate UPDATE statement", value: generateUpdateTemplate(table) },
            { label: "DELETE", detail: "Generate DELETE statement", value: generateDeleteTemplate(table) },
        ];
        const selected = await vscode.window.showQuickPick(items, {
            title: "Select SQL Template",
            placeHolder: "Choose a template",
        });
        if (selected) {
            await showInQuickPick(`${selected.label} Template`, selected.value);
        }
    }
}
/**
 * Handle "Generate Mock Data" command
 */
async function handleGenerateMockData(element) {
    if (element.type === "table") {
        const table = getTableFromItem(element);
        if (!table) {
            vscode.window.showErrorMessage("Unable to resolve table metadata.");
            return;
        }
        const rowCountStr = await vscode.window.showInputBox({
            title: "Generate Mock Data",
            prompt: "How many rows?",
            value: "5",
            validateInput: (v) => (isNaN(parseInt(v)) || parseInt(v) < 1 ? "Enter a positive number" : undefined),
        });
        if (rowCountStr) {
            const rowCount = parseInt(rowCountStr);
            const mockData = generateMockData(table, rowCount);
            await showInQuickPick("Generated Mock Data", mockData);
        }
    }
}
/**
 * Handle "Dump Struct" command
 */
async function handleDumpStruct(element) {
    if (element.type === "table") {
        const table = getTableFromItem(element);
        if (!table) {
            vscode.window.showErrorMessage("Unable to resolve table metadata.");
            return;
        }
        const json = exportTableStructure(table);
        await showInQuickPick("Table Structure", json);
    }
}
/**
 * Handle "Drop" command
 */
async function handleDropTable(element) {
    if (element.type === "table") {
        const table = getTableFromItem(element);
        if (!table) {
            vscode.window.showErrorMessage("Unable to resolve table metadata.");
            return;
        }
        const confirmed = await vscode.window.showWarningMessage(`Are you sure you want to drop table "${table.schema}"."${table.name}"?`, { modal: true }, "Drop");
        if (confirmed === "Drop") {
            const sql = `DROP TABLE "${table.schema}"."${table.name}";`;
            await showInQuickPick("Drop Table SQL", sql);
        }
    }
}
/**
 * Handle "Truncate" command
 */
async function handleTruncateTable(element) {
    if (element.type === "table") {
        const table = getTableFromItem(element);
        if (!table) {
            vscode.window.showErrorMessage("Unable to resolve table metadata.");
            return;
        }
        const confirmed = await vscode.window.showWarningMessage(`Are you sure you want to truncate table "${table.schema}"."${table.name}"? This will remove all rows.`, { modal: true }, "Truncate");
        if (confirmed === "Truncate") {
            const sql = generateTruncateTableSql(table);
            await showInQuickPick("Truncate Table SQL", sql);
        }
    }
}
/**
 * Show content in a quick pick dropdown
 */
async function showInQuickPick(title, content) {
    const lines = content.split("\n");
    await vscode.window.showQuickPick(lines, {
        title,
        canPickMany: false,
        matchOnDetail: false,
    });
}
//# sourceMappingURL=DatabaseContextCommands.js.map