/**
 * Context Menu Commands for Database Explorer
 */

import * as vscode from "vscode";
import { SchemaTreeItem } from "../providers/DatabaseExplorerProvider";
import { Table } from "../models/Schema";
import {
  generateDeleteTemplate as buildDeleteTemplate,
  generateMockData as buildMockData,
  generateTruncateTableSql as buildTruncateTableSql,
  generateUpdateTemplate as buildUpdateTemplate,
} from "./TableContextSql";

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
  return buildUpdateTemplate(table);
}

/**
 * Generate DELETE template
 */
export function generateDeleteTemplate(table: Table): string {
  return buildDeleteTemplate(table);
}

/**
 * Generate mock INSERT statements
 */
export function generateMockData(table: Table, rowCount: number = 5): string {
  return buildMockData(table, rowCount);
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
 * Generate TRUNCATE statement
 */
export function generateTruncateTableSql(table: Table): string {
  return buildTruncateTableSql(table);
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
 * Handle "Truncate" command
 */
export async function handleTruncateTable(element: SchemaTreeItem): Promise<void> {
  if (element.type === "table") {
    const table = getTableFromItem(element);
    if (!table) {
      vscode.window.showErrorMessage("Unable to resolve table metadata.");
      return;
    }
    const confirmed = await vscode.window.showWarningMessage(
      `Are you sure you want to truncate table "${table.schema}"."${table.name}"? This will remove all rows.`,
      { modal: true },
      "Truncate"
    );

    if (confirmed === "Truncate") {
      const sql = generateTruncateTableSql(table);
      await showInQuickPick("Truncate Table SQL", sql);
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
