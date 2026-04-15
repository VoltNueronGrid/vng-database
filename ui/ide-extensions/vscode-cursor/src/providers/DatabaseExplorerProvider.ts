/**
 * DatabaseExplorer TreeView Provider
 * Displays database schema hierarchy: Databases -> Schemas -> Tables -> Columns
 */

import * as vscode from "vscode";
import { Database, Schema, Table, Column, getColumnTypeDisplay } from "../models/Schema";
import { Connection } from "../models/Connection";
import { SchemaManager } from "../services/SchemaManager";

export interface SchemaTreeTableData {
  database: string;
  schema: string;
  table: Table;
}

export interface SchemaTreeColumnData {
  database: string;
  schema: string;
  table: string;
  column: Column;
}

export type SchemaTreeItem = {
  type: "database" | "schema" | "table" | "column" | "loading" | "error";
  label: string;
  icon?: string;
  data?: any;
};

export class DatabaseExplorerProvider implements vscode.TreeDataProvider<SchemaTreeItem> {
  private _onDidChangeTreeData: vscode.EventEmitter<SchemaTreeItem | undefined | null | void> =
    new vscode.EventEmitter<SchemaTreeItem | undefined | null | void>();
  readonly onDidChangeTreeData: vscode.Event<SchemaTreeItem | undefined | null | void> =
    this._onDidChangeTreeData.event;

  private schemaManager: SchemaManager;
  private connection: Connection | null = null;
  private expandedItems: Set<string> = new Set();

  constructor(schemaManager: SchemaManager) {
    this.schemaManager = schemaManager;
  }

  /**
   * Set the active connection
   */
  setConnection(connection: Connection | null): void {
    this.connection = connection;
    this.expandedItems.clear();
    this.refresh();
  }

  /**
   * Refresh the tree view
   */
  refresh(item?: SchemaTreeItem): void {
    this._onDidChangeTreeData.fire(item);
  }

  /**
   * Get tree item
   */
  getTreeItem(element: SchemaTreeItem): vscode.TreeItem {
    const treeItem = new vscode.TreeItem(element.label);

    treeItem.id = this.getItemId(element);
    treeItem.contextValue = element.type;
    treeItem.description = element.type === "column" ? (element.data as Column)?.type : undefined;

    // Icons
    if (element.type === "loading") {
      treeItem.iconPath = new vscode.ThemeIcon("loading~spin");
    } else if (element.type === "error") {
      treeItem.iconPath = new vscode.ThemeIcon("error");
    } else if (element.type === "database") {
      treeItem.iconPath = new vscode.ThemeIcon("database");
    } else if (element.type === "schema") {
      treeItem.iconPath = new vscode.ThemeIcon("folder");
    } else if (element.type === "table") {
      treeItem.iconPath = new vscode.ThemeIcon("table");
    } else if (element.type === "column") {
      const column = element.data as Column;
      const typeDisplay = getColumnTypeDisplay(column.type);
      treeItem.iconPath = new vscode.ThemeIcon("symbol-field");
      treeItem.description = `${typeDisplay.label}${column.nullable ? " (null)" : ""}${column.isPrimaryKey ? " (PK)" : ""}`;
    }

    // Collapsible state
    if (element.type === "loading" || element.type === "error") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
    } else if (element.type === "database") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
    } else if (element.type === "schema") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
    } else if (element.type === "table") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
    } else if (element.type === "column") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
    }

    return treeItem;
  }

  /**
   * Get children of an element
   */
  async getChildren(element?: SchemaTreeItem): Promise<SchemaTreeItem[]> {
    if (!this.connection) {
      return [];
    }

    try {
      // Root level: show databases
      if (!element) {
        const databases = await this.schemaManager.getDatabases(this.connection);
        if (databases.length === 0) {
          return [{ type: "error", label: "No databases found" }];
        }
        return databases.map((db) => ({
          type: "database" as const,
          label: db.name,
          data: db,
        }));
      }

      // Database level: show schemas
      if (element.type === "database") {
        const database = element.data as Database;
        const schemas = await this.schemaManager.getSchemas(this.connection, database.name);
        if (schemas.length === 0) {
          return [{ type: "error", label: "No schemas found" }];
        }
        return schemas.map((schema) => ({
          type: "schema" as const,
          label: schema.name || "default",
          data: { database: database.name, schema },
        }));
      }

      // Schema level: show tables
      if (element.type === "schema") {
        const { database, schema } = element.data as { database: string; schema: Schema };
        const tables = await this.schemaManager.getTables(this.connection, database, schema.name);
        if (tables.length === 0) {
          return [{ type: "error", label: "No tables found" }];
        }
        return tables
          .filter((t) => !t.isSystem) // Hide system tables by default
          .map((table) => ({
            type: "table" as const,
            label: table.name,
            data: { database, schema: schema.name, table },
          }));
      }

      // Table level: show columns
      if (element.type === "table") {
        const { database, schema, table } = element.data as SchemaTreeTableData;
        const columns = await this.schemaManager.getColumns(this.connection, database, schema, table.name);
        if (columns.length === 0) {
          return [{ type: "error", label: "No columns found" }];
        }
        return columns.map((column) => ({
          type: "column" as const,
          label: column.name,
          data: {
            database,
            schema,
            table: table.name,
            column,
          } as SchemaTreeColumnData,
        }));
      }

      return [];
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unknown error";
      console.error("DatabaseExplorerProvider.getChildren error:", message);
      return [{ type: "error", label: `Error: ${message}` }];
    }
  }

  /**
   * Get parent of an element
   */
  getParent(element: SchemaTreeItem): vscode.ProviderResult<SchemaTreeItem> {
    // Could implement if needed for performance
    return null;
  }

  /**
   * Get unique ID for an item
   */
  private getItemId(element: SchemaTreeItem): string {
    if (element.type === "database") {
      return `db-${(element.data as Database).name}`;
    }
    if (element.type === "schema") {
      const { database, schema } = element.data as { database: string; schema: Schema };
      return `schema-${database}-${schema.name}`;
    }
    if (element.type === "table") {
      const { database, schema, table } = element.data as { database: string; schema: string; table: Table };
      return `table-${database}-${schema}-${table.name}`;
    }
    if (element.type === "column") {
      const { database, schema, table, column } = element.data as SchemaTreeColumnData;
      return `col-${database}-${schema}-${table}-${column.name}`;
    }
    return `${element.type}-${element.label}`;
  }
}

export function createDatabaseExplorerProvider(schemaManager: SchemaManager): DatabaseExplorerProvider {
  return new DatabaseExplorerProvider(schemaManager);
}
