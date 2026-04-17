/**
 * DatabaseExplorer TreeView Provider
 * Displays database schema hierarchy: Databases -> Schemas -> Tables -> Columns
 */

import * as vscode from "vscode";
import { Database, Schema, Table, Column, getColumnTypeDisplay } from "../models/Schema";
import { Connection } from "../models/Connection";
import { SchemaManager } from "../services/SchemaManager";
import { describeConnectionNode, shouldExpandConnectionToDatabases } from "./DatabaseExplorerTree";

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
  type: "connection" | "database" | "schema" | "table" | "column" | "loading" | "error" | "message";
  label: string;
  icon?: string;
  contextValue?: string;
  description?: string;
  command?: vscode.Command;
  data?: any;
};

export class DatabaseExplorerProvider implements vscode.TreeDataProvider<SchemaTreeItem> {
  private _onDidChangeTreeData: vscode.EventEmitter<SchemaTreeItem | undefined | null | void> =
    new vscode.EventEmitter<SchemaTreeItem | undefined | null | void>();
  readonly onDidChangeTreeData: vscode.Event<SchemaTreeItem | undefined | null | void> =
    this._onDidChangeTreeData.event;

  private schemaManager: SchemaManager;
  private extensionUri: vscode.Uri;
  private connection: Connection | null = null;
  private connections: Connection[] = [];
  private expandedItems: Set<string> = new Set();

  constructor(extensionUri: vscode.Uri, schemaManager: SchemaManager) {
    this.extensionUri = extensionUri;
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

  setConnections(connections: Connection[]): void {
    this.connections = [...connections];
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
    treeItem.contextValue = element.contextValue ?? element.type;
    treeItem.command = element.command;
    treeItem.description = element.type === "column" ? (element.data as Column)?.type : element.description;
    treeItem.accessibilityInformation = {
      label: this.getAccessibilityLabel(element),
      role: "treeitem",
    };

    // Icons
    if (element.type === "loading") {
      treeItem.iconPath = new vscode.ThemeIcon("loading~spin");
    } else if (element.type === "error") {
      treeItem.iconPath = new vscode.ThemeIcon("error");
    } else if (element.type === "connection") {
      treeItem.iconPath = this.getMediaIcon((element.data as Connection).isActive ? "connection-active" : "connection-inactive");
      treeItem.tooltip = `${(element.data as Connection).settings.name}\n${(element.data as Connection).settings.baseUrl}`;
    } else if (element.type === "database") {
      treeItem.iconPath = this.getMediaIcon("database");
    } else if (element.type === "schema") {
      treeItem.iconPath = this.getMediaIcon("schema");
    } else if (element.type === "table") {
      treeItem.iconPath = this.getMediaIcon("table");
    } else if (element.type === "column") {
      const column = element.data as Column;
      const typeDisplay = getColumnTypeDisplay(column.type);
      treeItem.iconPath = this.getMediaIcon("column");
      treeItem.description = `${typeDisplay.label}${column.nullable ? " (null)" : ""}${column.isPrimaryKey ? " (PK)" : ""}`;
    } else if (element.type === "message") {
      treeItem.iconPath = new vscode.ThemeIcon("info");
    }

    // Collapsible state
    if (element.type === "loading" || element.type === "error" || element.type === "message") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
    } else if (element.type === "connection") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
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
    try {
      if (!element) {
        if (this.connections.length === 0) {
          // Empty children so `viewsWelcome` in package.json shows (Screenshot-2 style).
          return [];
        }
        return this.connections.map((connection) => {
          const presentation = describeConnectionNode(connection);
          return {
            type: "connection" as const,
            label: connection.settings.name,
            description: presentation.description,
            contextValue: presentation.contextValue,
            data: connection,
          };
        });
      }

      if (element.type === "connection") {
        const connection = element.data as Connection;
        if (!shouldExpandConnectionToDatabases(connection)) {
          return [
            {
              type: "message",
              label: describeConnectionNode(connection).browseMessage,
              contextValue: "message",
            },
          ];
        }

        const databases = await this.schemaManager.getDatabases(connection);
        if (databases.length === 0) {
          return [{ type: "message", label: "No databases found", contextValue: "message" }];
        }
        return databases.map((db) => ({
          type: "database" as const,
          label: db.name,
          data: { connectionId: connection.id, database: db },
        }));
      }

      if (!this.connection) {
        return [];
      }

      // Database level: show schemas
      if (element.type === "database") {
        const connection = this.resolveConnectionForElement(element) ?? this.connection;
        const database = this.unwrapDatabase(element.data);
        const schemas = await this.schemaManager.getSchemas(connection, database.name);
        if (schemas.length === 0) {
          return [{ type: "message", label: "No schemas found", contextValue: "message" }];
        }
        return schemas.map((schema) => ({
          type: "schema" as const,
          label: schema.name || "default",
          data: { connectionId: connection.id, database: database.name, schema },
        }));
      }

      // Schema level: show tables
      if (element.type === "schema") {
        const connection = this.resolveConnectionForElement(element) ?? this.connection;
        const { database, schema } = element.data as { connectionId?: string; database: string; schema: Schema };
        const tables = await this.schemaManager.getTables(connection, database, schema.name);
        if (tables.length === 0) {
          return [{ type: "message", label: "No tables found", contextValue: "message" }];
        }
        return tables
          .filter((t) => !t.isSystem) // Hide system tables by default
          .map((table) => ({
            type: "table" as const,
            label: table.name,
            data: { connectionId: connection.id, database, schema: schema.name, table },
          }));
      }

      // Table level: show columns
      if (element.type === "table") {
        const connection = this.resolveConnectionForElement(element) ?? this.connection;
        const { database, schema, table } = element.data as SchemaTreeTableData & { connectionId?: string };
        const columns = await this.schemaManager.getColumns(connection, database, schema, table.name);
        if (columns.length === 0) {
          return [{ type: "message", label: "No columns found", contextValue: "message" }];
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
      const data = element.data as { connectionId?: string; database: Database } | Database;
      const database = this.unwrapDatabase(data);
      const connectionId = this.unwrapConnectionId(data) ?? this.connection?.id ?? "default";
      return `db-${connectionId}-${database.name}`;
    }
    if (element.type === "connection") {
      return `connection-${(element.data as Connection).id}`;
    }
    if (element.type === "schema") {
      const { connectionId, database, schema } = element.data as { connectionId?: string; database: string; schema: Schema };
      return `schema-${connectionId ?? this.connection?.id ?? "default"}-${database}-${schema.name}`;
    }
    if (element.type === "table") {
      const { connectionId, database, schema, table } = element.data as {
        connectionId?: string;
        database: string;
        schema: string;
        table: Table;
      };
      return `table-${connectionId ?? this.connection?.id ?? "default"}-${database}-${schema}-${table.name}`;
    }
    if (element.type === "column") {
      const { database, schema, table, column } = element.data as SchemaTreeColumnData;
      return `col-${database}-${schema}-${table}-${column.name}`;
    }
    return `${element.type}-${element.label}`;
  }

  private resolveConnectionForElement(element: SchemaTreeItem): Connection | null {
    const connectionId = this.unwrapConnectionId(element.data);
    if (!connectionId) {
      return this.connection;
    }
    return this.connections.find((connection) => connection.id === connectionId) ?? this.connection;
  }

  private unwrapConnectionId(data: unknown): string | undefined {
    if (!data || typeof data !== "object") {
      return undefined;
    }
    return "connectionId" in data ? (data as { connectionId?: string }).connectionId : undefined;
  }

  private unwrapDatabase(data: { connectionId?: string; database: Database } | Database): Database {
    return "database" in data ? data.database : data;
  }

  private getMediaIcon(name: string): { light: vscode.Uri; dark: vscode.Uri } {
    return {
      light: vscode.Uri.joinPath(this.extensionUri, "media", `${name}-light.svg`),
      dark: vscode.Uri.joinPath(this.extensionUri, "media", `${name}-dark.svg`),
    };
  }

  private getAccessibilityLabel(element: SchemaTreeItem): string {
    if (element.type === "connection") {
      const connection = element.data as Connection;
      return `Connection ${connection.settings.name}, ${connection.isActive ? "active" : "inactive"}, ${connection.isConnected ? "connected" : "not verified"}`;
    }
    if (element.type === "database") {
      return `Database ${element.label}`;
    }
    if (element.type === "schema") {
      return `Schema ${element.label}`;
    }
    if (element.type === "table") {
      return `Table ${element.label}`;
    }
    if (element.type === "column") {
      const column = (element.data as SchemaTreeColumnData).column;
      return `Column ${column.name}, type ${getColumnTypeDisplay(column.type).label}${column.isPrimaryKey ? ", primary key" : ""}${column.nullable ? ", nullable" : ""}`;
    }
    return element.label;
  }
}

export function createDatabaseExplorerProvider(extensionUri: vscode.Uri, schemaManager: SchemaManager): DatabaseExplorerProvider {
  return new DatabaseExplorerProvider(extensionUri, schemaManager);
}
