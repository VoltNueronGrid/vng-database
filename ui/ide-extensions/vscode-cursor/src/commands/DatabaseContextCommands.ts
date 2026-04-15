/**
 * Context Menu Commands for Database Explorer
 */

import * as vscode from "vscode";
import { SchemaTreeItem } from "../providers/DatabaseExplorerProvider";
import { Table, Column } from "../models/Schema";

interface TableNodeData {
  database: string;
  schema: string;
  table: Table;
}

function getTableFromItem(element: SchemaTreeItem): Table | undefined {
  if (element.type !== "table" || !element.data) {
    return undefined;
  }

  const maybeWrapped = element.data as Partial<TableNodeData>;
  if (maybeWrapped.table) {
    return maybeWrapped.table;
  }

  return element.data as Table;
}

/**
 * Generate DDL for a table
 */
export function generateTableDDL(table: Table): string {
  const columns = table.columns
    .map((col) => {
      let def = `"${col.name}" ${col.type}`;
      if (!col.nullable) def += " NOT NULL";
      if (col.isPrimaryKey) def += " PRIMARY KEY";
      if (col.isUnique) def += " UNIQUE";
      if (col.defaultValue) def += ` DEFAULT ${col.defaultValue}`;
      return def;
    })
    .join(",\n  ");

  return `CREATE TABLE "${table.schema}"."${table.name}" (\n  ${columns}\n);`;
}

/**
 * Generate SELECT template
 */
export function generateSelectTemplate(table: Table): string {
  const columns = table.columns.map((c) => `"${c.name}"`).join(",\n  ");
  return `SELECT\n  ${columns}\nFROM "${table.schema}"."${table.name}"\nWHERE 1=1;`;
}

/**
 * Generate INSERT template
 */
export function generateInsertTemplate(table: Table): string {
  const columns = table.columns.map((c) => `"${c.name}"`).join(", ");
  const placeholders = table.columns.map(() => "?").join(", ");
  return `INSERT INTO "${table.schema}"."${table.name}" (${columns})\nVALUES (${placeholders});`;
}

/**
 * Generate UPDATE template
 */
export function generateUpdateTemplate(table: Table): string {
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
export function generateDeleteTemplate(table: Table): string {
  const pk = table.columns.find((c) => c.isPrimaryKey);
  const whereClause = pk ? `WHERE "${pk.name}" = ?;` : "WHERE 1=1;";
  return `DELETE FROM "${table.schema}"."${table.name}"\n${whereClause}`;
}

/**
 * Generate mock INSERT statements
 */
export function generateMockData(table: Table, rowCount: number = 5): string {
  const mockValueGetter = (column: Column): string => {
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

  const inserts: string[] = [];
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
export function exportTableStructure(table: Table): string {
  return JSON.stringify(
    {
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
    },
    null,
    2
  );
}

/**
 * Handle "Copy Name" command
 */
export async function handleCopyName(element: SchemaTreeItem): Promise<void> {
  if (element.type === "table" || element.type === "column") {
    await vscode.env.clipboard.writeText(element.label);
    vscode.window.showInformationMessage(`Copied: ${element.label}`);
  }
}

/**
 * Handle "Show DDL" command
 */
export async function handleShowDDL(element: SchemaTreeItem): Promise<void> {
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
export async function handleSQLTemplate(element: SchemaTreeItem): Promise<void> {
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
export async function handleGenerateMockData(element: SchemaTreeItem): Promise<void> {
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
export async function handleDumpStruct(element: SchemaTreeItem): Promise<void> {
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
export async function handleDropTable(element: SchemaTreeItem): Promise<void> {
  if (element.type === "table") {
    const table = getTableFromItem(element);
    if (!table) {
      vscode.window.showErrorMessage("Unable to resolve table metadata.");
      return;
    }
    const confirmed = await vscode.window.showWarningMessage(
      `Are you sure you want to drop table "${table.schema}"."${table.name}"?`,
      { modal: true },
      "Drop"
    );

    if (confirmed === "Drop") {
      const sql = `DROP TABLE "${table.schema}"."${table.name}";`;
      await showInQuickPick("Drop Table SQL", sql);
    }
  }
}

/**
 * Show content in a quick pick dropdown
 */
async function showInQuickPick(title: string, content: string): Promise<void> {
  const lines = content.split("\n");
  await vscode.window.showQuickPick(lines, {
    title,
    canPickMany: false,
    matchOnDetail: false,
  });
}
