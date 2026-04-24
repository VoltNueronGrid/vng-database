"use strict";
/**
 * Context Menu Commands for Database Explorer
 * Includes S5-001 (connection), S5-002 (table), S5-003 (column) additions.
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
exports.handleCopyName = handleCopyName;
exports.handleShowDDL = handleShowDDL;
exports.handleSQLTemplate = handleSQLTemplate;
exports.handleGenerateMockData = handleGenerateMockData;
exports.handleDumpStruct = handleDumpStruct;
exports.handleDropTable = handleDropTable;
exports.handleCopyConnectionHost = handleCopyConnectionHost;
exports.handleShowConnectionStatus = handleShowConnectionStatus;
exports.handleShowConnectionHistory = handleShowConnectionHistory;
exports.handleImportSqlFile = handleImportSqlFile;
exports.handleDumpTableData = handleDumpTableData;
exports.handleTruncateTable = handleTruncateTable;
exports.handleCopyColumnName = handleCopyColumnName;
exports.handleCopyColumnDefinition = handleCopyColumnDefinition;
exports.handleAddColumnWizard = handleAddColumnWizard;
const vscode = __importStar(require("vscode"));
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
        let def = `"${col.name}" ${col.type ?? "UNKNOWN"}`;
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
    const mutableColumns = table.columns
        .filter((c) => !c.isPrimaryKey)
        .map((c) => `"${c.name}" = ?`);
    if (mutableColumns.length === 0) {
        return `-- No mutable columns found on "${table.schema}"."${table.name}"\n-- Table appears to contain only primary key columns.`;
    }
    const sets = mutableColumns.join(",\n  ");
    const pk = table.columns.find((c) => c.isPrimaryKey);
    const whereClause = pk ? `WHERE "${pk.name}" = ?;` : "WHERE 1=1;";
    return `UPDATE "${table.schema}"."${table.name}"\nSET ${sets}\n${whereClause}`;
}
/**
 * Generate DELETE template
 */
function generateDeleteTemplate(table) {
    const pk = table.columns.find((c) => c.isPrimaryKey);
    const whereClause = pk ? `WHERE "${pk.name}" = ?;` : "WHERE 1=1;";
    return `DELETE FROM "${table.schema}"."${table.name}"\n${whereClause}`;
}
/**
 * Generate mock INSERT statements
 */
function generateMockData(table, rowCount = 5) {
    const mockValueGetter = (column) => {
        const type = (column.type ?? "UNKNOWN").toUpperCase();
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
    const columns = table.columns.map((c) => `"${c.name}"`).join(", ");
    for (let i = 0; i < rowCount; i++) {
        const values = table.columns.map((c) => mockValueGetter(c)).join(", ");
        inserts.push(`INSERT INTO "${table.schema}"."${table.name}" (${columns}) VALUES (${values});`);
    }
    return inserts.join("\n");
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
 * Handle "Copy Name" command — copies fully-qualified name (db.schema.table or db.schema.table.column)
 */
async function handleCopyName(element) {
    if (element.type === "table") {
        const wrapped = element.data;
        const table = wrapped.table ?? element.data;
        const db = wrapped.database ?? "default";
        const schema = wrapped.schema ?? table?.schema ?? "public";
        const name = table?.name ?? element.label;
        const qualified = `${db}.${schema}.${name}`;
        await vscode.env.clipboard.writeText(qualified);
        vscode.window.showInformationMessage(`Copied: ${qualified}`);
    }
    else if (element.type === "column") {
        await vscode.env.clipboard.writeText(element.label);
        vscode.window.showInformationMessage(`Copied: ${element.label}`);
    }
}
/**
 * Handle "Show DDL" command — opens a new untitled SQL editor
 */
async function handleShowDDL(element) {
    if (element.type === "table") {
        const table = getTableFromItem(element);
        if (!table) {
            vscode.window.showErrorMessage("Unable to resolve table metadata.");
            return;
        }
        const ddl = generateTableDDL(table);
        const doc = await vscode.workspace.openTextDocument({ content: ddl, language: "sql" });
        await vscode.window.showTextDocument(doc, { preview: false });
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
            const doc = await vscode.workspace.openTextDocument({ content: selected.value, language: "sql" });
            await vscode.window.showTextDocument(doc, { preview: false });
        }
    }
}
/**
 * Handle "Generate Mock Data" command — opens generated INSERT SQL in a new untitled editor
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
            const doc = await vscode.workspace.openTextDocument({ content: mockData, language: "sql" });
            await vscode.window.showTextDocument(doc, { preview: false });
        }
    }
}
/**
 * Handle "Dump Struct" command — opens table structure JSON in a new untitled editor
 */
async function handleDumpStruct(element) {
    if (element.type === "table") {
        const table = getTableFromItem(element);
        if (!table) {
            vscode.window.showErrorMessage("Unable to resolve table metadata.");
            return;
        }
        const json = exportTableStructure(table);
        const doc = await vscode.workspace.openTextDocument({ content: json, language: "json" });
        await vscode.window.showTextDocument(doc, { preview: false });
    }
}
/**
 * Handle "Drop" command — returns the DROP SQL for execution, or undefined if cancelled.
 */
async function handleDropTable(element) {
    if (element.type !== "table") {
        return undefined;
    }
    const table = getTableFromItem(element);
    if (!table) {
        vscode.window.showErrorMessage("Unable to resolve table metadata.");
        return undefined;
    }
    const confirmed = await vscode.window.showWarningMessage(`Are you sure you want to DROP TABLE "${table.schema}"."${table.name}"? This cannot be undone.`, { modal: true }, "Drop");
    if (confirmed !== "Drop") {
        return undefined;
    }
    return `DROP TABLE "${table.schema}"."${table.name}";`;
}
// ─── S5-001: Connection context menu commands ─────────────────────────────────
/**
 * Copy the host:port of the selected connection to the clipboard.
 */
async function handleCopyConnectionHost(connection) {
    const { host, port } = connection.settings;
    const text = `${host}:${port}`;
    await vscode.env.clipboard.writeText(text);
    vscode.window.showInformationMessage(`Copied: ${text}`);
}
/**
 * Show connection diagnostic information in the VoltNueronGrid output channel.
 */
async function handleShowConnectionStatus(connection, output) {
    const { name, baseUrl, mode, host, port } = connection.settings;
    const healthState = connection.isConnected ? "verified" : "not verified";
    output.appendLine("─── Connection Status ───────────────────────────────────");
    output.appendLine(`Name:      ${name}`);
    output.appendLine(`Base URL:  ${baseUrl}`);
    output.appendLine(`Host:      ${host}:${port}`);
    output.appendLine(`Mode:      ${mode}`);
    output.appendLine(`Health:    ${healthState}`);
    output.appendLine(`Active:    ${connection.isActive}`);
    output.appendLine("─────────────────────────────────────────────────────────");
    output.show(true);
    vscode.window.showInformationMessage(`Connection status for '${name}' written to output.`);
}
/**
 * Show recent query history entries for the given connection.
 */
async function handleShowConnectionHistory(connection, historyEntries) {
    if (historyEntries.length === 0) {
        vscode.window.showInformationMessage(`No query history found for '${connection.settings.name}'.`);
        return;
    }
    const items = historyEntries.slice(0, 50).map((entry) => ({
        label: entry.query.replace(/\s+/g, " ").trim().slice(0, 120),
        description: `${entry.status} • ${entry.executionTime ?? 0} ms`,
        detail: new Date(entry.timestamp).toLocaleString(),
    }));
    await vscode.window.showQuickPick(items, {
        title: `Query History — ${connection.settings.name}`,
        placeHolder: "Recent queries (read-only)",
        canPickMany: false,
    });
}
/**
 * Open a file-picker for a .sql file and return its content for execution.
 * The actual SQL execution is wired in extension.ts.
 */
async function handleImportSqlFile() {
    const uris = await vscode.window.showOpenDialog({
        canSelectMany: false,
        filters: { SQL: ["sql"], "All Files": ["*"] },
        openLabel: "Import SQL File",
    });
    if (!uris || uris.length === 0) {
        return undefined;
    }
    const { promises: fsPromises } = await Promise.resolve().then(() => __importStar(require("fs")));
    const content = await fsPromises.readFile(uris[0].fsPath, "utf8");
    return content;
}
/**
 * Dump table data — prompts for format, executes query, opens result in untitled editor.
 * @param executeSql callback that runs SQL and returns rows (column names + row arrays)
 */
async function handleDumpTableData(element, executeSql) {
    if (element.type !== "table") {
        vscode.window.showWarningMessage("Select a table to dump its data.");
        return;
    }
    const table = getTableFromItem(element);
    if (!table) {
        vscode.window.showErrorMessage("Unable to resolve table metadata.");
        return;
    }
    const formatPick = await vscode.window.showQuickPick([
        { label: "CSV", description: "Comma-separated values, no header row", format: "csv" },
        { label: "CSV with Headers", description: "Comma-separated values with column names", format: "csv-headers" },
        { label: "JSON", description: "Array of objects", format: "json" },
        { label: "Parquet", description: "Apache Parquet (CLI export)", format: "parquet" },
        { label: "Excel", description: "Excel workbook (CLI export)", format: "excel" },
    ], { title: "Dump Table Data — Choose Format", placeHolder: "Select export format" });
    if (!formatPick) {
        return;
    }
    if (formatPick.format === "parquet" || formatPick.format === "excel") {
        const ext = formatPick.format === "parquet" ? "parquet" : "xlsx";
        vscode.window.showInformationMessage(`${formatPick.label} export requires the VoltNueronGrid CLI: vng export --table="${table.schema}"."${table.name}" --format=${ext}`);
        return;
    }
    await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: `Exporting ${table.name}…`, cancellable: false }, async () => {
        const sql = `SELECT * FROM "${table.schema}"."${table.name}" LIMIT 10000;`;
        const result = await executeSql(sql);
        if (!result) {
            vscode.window.showErrorMessage("Failed to fetch table data.");
            return;
        }
        let content;
        let language;
        if (formatPick.format === "csv") {
            content = result.rows.map((row) => row.map((v) => csvCell(v)).join(",")).join("\n");
            language = "csv";
        }
        else if (formatPick.format === "csv-headers") {
            const header = result.columns.map((c) => csvCell(c)).join(",");
            const rows = result.rows.map((row) => row.map((v) => csvCell(v)).join(",")).join("\n");
            content = rows.length > 0 ? `${header}\n${rows}` : header;
            language = "csv";
        }
        else {
            const objects = result.rows.map((row) => {
                const obj = {};
                result.columns.forEach((col, i) => { obj[col] = row[i]; });
                return obj;
            });
            content = JSON.stringify(objects, null, 2);
            language = "json";
        }
        const doc = await vscode.workspace.openTextDocument({ content, language });
        await vscode.window.showTextDocument(doc, { preview: false });
    });
}
function csvCell(value) {
    if (value === null || value === undefined) {
        return "";
    }
    const s = String(value);
    if (s.includes(",") || s.includes('"') || s.includes("\n")) {
        return `"${s.replace(/"/g, '""')}"`;
    }
    return s;
}
/**
 * Prompt for confirmation then return the TRUNCATE SQL for execution.
 * Returns undefined if the user cancels.
 */
async function handleTruncateTable(element) {
    if (element.type !== "table") {
        vscode.window.showWarningMessage("Select a table to truncate.");
        return undefined;
    }
    const table = getTableFromItem(element);
    if (!table) {
        vscode.window.showErrorMessage("Unable to resolve table metadata.");
        return undefined;
    }
    const confirmed = await vscode.window.showWarningMessage(`TRUNCATE TABLE "${table.schema}"."${table.name}"? This deletes ALL rows and cannot be undone.`, { modal: true }, "Truncate");
    if (confirmed !== "Truncate") {
        return undefined;
    }
    return `TRUNCATE TABLE "${table.schema}"."${table.name}";`;
}
// ─── S5-003: Column context menu commands ────────────────────────────────────
function getColumnFromItem(element) {
    if (element.type !== "column" || !element.data) {
        return undefined;
    }
    return element.data;
}
/**
 * Copy the column name to the clipboard.
 */
async function handleCopyColumnName(element) {
    if (element.type !== "column") {
        vscode.window.showWarningMessage("Select a column node to copy its name.");
        return;
    }
    await vscode.env.clipboard.writeText(element.label);
    vscode.window.showInformationMessage(`Copied column name: ${element.label}`);
}
/**
 * Copy the full column definition ({name} {type}) to the clipboard.
 */
async function handleCopyColumnDefinition(element) {
    if (element.type !== "column") {
        vscode.window.showWarningMessage("Select a column node to copy its definition.");
        return;
    }
    const column = getColumnFromItem(element);
    const definition = column ? `${column.name} ${column.type}` : element.label;
    await vscode.env.clipboard.writeText(definition);
    vscode.window.showInformationMessage(`Copied column definition: ${definition}`);
}
/**
 * Interactive ALTER TABLE ADD COLUMN wizard.
 * Returns the generated SQL or undefined if cancelled.
 */
async function handleAddColumnWizard(element) {
    // Resolve the parent table — element may be a column or a table node
    let schemaName;
    let tableName;
    if (element.type === "table") {
        const table = getTableFromItem(element);
        schemaName = table?.schema;
        tableName = table?.name;
    }
    else if (element.type === "column") {
        // Parent table info is not directly on column; use label as fallback
        tableName = element.label;
    }
    if (!tableName) {
        vscode.window.showWarningMessage("Select a table or column node to use the Add Column wizard.");
        return undefined;
    }
    const columnName = await vscode.window.showInputBox({
        title: "Add Column — Name",
        prompt: "Enter the new column name",
        ignoreFocusOut: true,
        validateInput: (v) => (v.trim().length === 0 ? "Column name is required." : undefined),
    });
    if (!columnName) {
        return undefined;
    }
    const columnTypes = [
        "INT", "BIGINT", "SMALLINT", "DECIMAL", "FLOAT", "DOUBLE",
        "VARCHAR(255)", "TEXT", "BOOLEAN", "DATE", "TIMESTAMP", "JSON", "BYTEA",
    ];
    const picked = await vscode.window.showQuickPick(columnTypes, {
        title: "Add Column — Type",
        placeHolder: "Choose a column type",
    });
    if (!picked) {
        return undefined;
    }
    const qualifiedTable = schemaName
        ? `"${schemaName}"."${tableName}"`
        : `"${tableName}"`;
    return `ALTER TABLE ${qualifiedTable} ADD COLUMN "${columnName.trim()}" ${picked};`;
}
//# sourceMappingURL=DatabaseContextCommands.js.map