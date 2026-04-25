/**
 * DatabaseExplorer TreeView Provider
 * Displays database schema hierarchy: Databases -> Schemas -> Tables -> Columns
 */

import * as vscode from "vscode";
import { Database, Schema, Table, Column, Index, getColumnTypeDisplay } from "../models/Schema";
import { Connection, ConnectionHealthState } from "../models/Connection";
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

export interface SchemaTreeContainerData {
  connectionId?: string;
  database: string;
  schema: string;
  table: string;
  /** Which container this node represents */
  container: "columns" | "indexes" | "triggers" | "views" | "queries" | "types";
  /** Extra payload (e.g. the full Table object so children can read it) */
  tableData?: Table;
}

export interface SchemaTreeIndexData {
  database: string;
  schema: string;
  table: string;
  index: Index;
}

/**
 * All discriminated node kinds in the explorer tree.
 *
 * New in S4:
 *  - "view"       : individual view entry (placeholder, schema level)
 *  - "container"  : generic collapsible folder (Columns / Indexes / Triggers /
 *                   Views / Queries / Types)
 *  - "index"      : a single index entry under the Indexes container
 *  - "trigger"    : placeholder trigger leaf
 */
export type SchemaTreeItem = {
  type:
    | "connection"
    | "database"
    | "schema"
    | "table"
    | "column"
    | "container"
    | "index"
    | "trigger"
    | "view"
    | "loading"
    | "error"
    | "message";
  label: string;
  icon?: string;
  contextValue?: string;
  description?: string;
  command?: vscode.Command;
  data?: any;
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Map diagnostic.state → VS Code codicon for inline status dot */
function connectionStateIcon(state: ConnectionHealthState): string {
  switch (state) {
    case "verified":
      return "$(pass-filled)";
    case "degraded":
      return "$(warning)";
    case "error":
      return "$(error)";
    case "unverified":
    default:
      return "$(circle-large-outline)";
  }
}

/**
 * Format a row count number into a human-readable string.
 * e.g. 1_234_567 → "~1.2M rows", 42 → "42 rows"
 */
function formatRowCount(count: number): string {
  if (count >= 1_000_000) {
    return `~${(count / 1_000_000).toFixed(1)}M rows`;
  }
  if (count >= 1_000) {
    return `~${(count / 1_000).toFixed(1)}K rows`;
  }
  return `${count} rows`;
}

// ---------------------------------------------------------------------------

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
    treeItem.description = element.description;
    treeItem.accessibilityInformation = {
      label: this.getAccessibilityLabel(element),
      role: "treeitem",
    };

    // -----------------------------------------------------------------------
    // Icons
    // -----------------------------------------------------------------------
    if (element.type === "loading") {
      treeItem.iconPath = new vscode.ThemeIcon("loading~spin");
    } else if (element.type === "error") {
      treeItem.iconPath = new vscode.ThemeIcon("error");
      // Show the full error on hover — labels truncate aggressively in the tree.
      treeItem.tooltip = element.label;
    } else if (element.type === "connection") {
      const connection = element.data as Connection;
      treeItem.label = connection.settings.name;
      // Use a ThemeIcon to show the health state; codicons in plain string labels render literally
      const iconId = connection.diagnostic.state === "verified" ? "pass-filled"
        : connection.diagnostic.state === "degraded" ? "warning"
        : connection.diagnostic.state === "error" ? "error"
        : "circle-large-outline";
      treeItem.iconPath = new vscode.ThemeIcon(iconId);
      treeItem.tooltip = new vscode.MarkdownString(
        `**${connection.settings.name}**\n\n` +
          `${connection.settings.baseUrl}\n\n` +
          `State: ${connection.diagnostic.state}` +
          (connection.diagnostic.message ? `\n\n${connection.diagnostic.message}` : "")
      );
    } else if (element.type === "database") {
      treeItem.iconPath = this.getMediaIcon("database");
    } else if (element.type === "schema") {
      treeItem.iconPath = this.getMediaIcon("schema");
    } else if (element.type === "table") {
      treeItem.iconPath = this.getMediaIcon("table");
      // S4-004: row count in description
      const table = (element.data as SchemaTreeTableData & { connectionId?: string }).table;
      if (table.rowCount !== undefined && table.rowCount !== null) {
        treeItem.description = formatRowCount(table.rowCount);
        treeItem.tooltip = `${table.name}\n${formatRowCount(table.rowCount)}${table.comment ? `\n${table.comment}` : ""}`;
      } else {
        treeItem.description = "(rows: unknown)";
        treeItem.tooltip = table.comment ?? table.name;
      }
    } else if (element.type === "column") {
      const column = (element.data as SchemaTreeColumnData).column;
      const typeDisplay = getColumnTypeDisplay(column.type);
      treeItem.iconPath = this.getMediaIcon("column");
      treeItem.description = `${typeDisplay.label}${column.nullable ? " (null)" : ""}${column.isPrimaryKey ? " (PK)" : ""}`;
    } else if (element.type === "container") {
      // S4-002 / S4-003: folder containers
      const container = (element.data as SchemaTreeContainerData).container;
      treeItem.iconPath = this.getContainerIcon(container);
    } else if (element.type === "index") {
      treeItem.iconPath = new vscode.ThemeIcon("symbol-key");
      const idx = (element.data as SchemaTreeIndexData).index;
      const badges: string[] = [];
      if (idx.isPrimary) badges.push("PK");
      if (idx.isUnique) badges.push("UNIQUE");
      treeItem.description = badges.join(", ") || idx.columns.join(", ");
      treeItem.tooltip = `${idx.name}\nColumns: ${idx.columns.join(", ")}${idx.isUnique ? "\nUnique" : ""}${idx.isPrimary ? "\nPrimary" : ""}`;
    } else if (element.type === "trigger") {
      treeItem.iconPath = new vscode.ThemeIcon("zap");
    } else if (element.type === "view") {
      treeItem.iconPath = new vscode.ThemeIcon("eye");
    } else if (element.type === "message") {
      treeItem.iconPath = new vscode.ThemeIcon("info");
    }

    // -----------------------------------------------------------------------
    // Collapsible state
    // -----------------------------------------------------------------------
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
    } else if (element.type === "container") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
    } else if (element.type === "index") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
    } else if (element.type === "trigger") {
      treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
    } else if (element.type === "view") {
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

      // Schema level: show container nodes (S4-002)
      if (element.type === "schema") {
        const connection = this.resolveConnectionForElement(element) ?? this.connection;
        const { database, schema } = element.data as { connectionId?: string; database: string; schema: Schema };
        const tables = await this.schemaManager.getTables(connection, database, schema.name);
        const visibleTables = tables.filter((t) => !t.isSystem);

        if (visibleTables.length === 0) {
          return [{ type: "message", label: "No tables found", contextValue: "message" }];
        }

        // Build the schema-level containers
        const schemaContainerBase = {
          connectionId: connection.id,
          database,
          schema: schema.name,
          table: "",
        };

        return [
          // Tables — shows actual table children
          {
            type: "container" as const,
            label: "Tables",
            description: `${visibleTables.length}`,
            contextValue: "containerTables",
            data: { ...schemaContainerBase, container: "columns", _tables: visibleTables } as any,
          },
          // Views — placeholder
          {
            type: "container" as const,
            label: "Views",
            contextValue: "containerViews",
            data: { ...schemaContainerBase, container: "views" } as SchemaTreeContainerData,
          },
          // Queries — placeholder
          {
            type: "container" as const,
            label: "Queries",
            contextValue: "containerQueries",
            data: { ...schemaContainerBase, container: "queries" } as SchemaTreeContainerData,
          },
          // Types — placeholder
          {
            type: "container" as const,
            label: "Types",
            contextValue: "containerTypes",
            data: { ...schemaContainerBase, container: "types" } as SchemaTreeContainerData,
          },
        ];
      }

      // Container node expansion (S4-002 / S4-003)
      if (element.type === "container") {
        return this.getContainerChildren(element);
      }

      // Table level: show Columns / Indexes / Triggers containers (S4-003)
      if (element.type === "table") {
        const connection = this.resolveConnectionForElement(element) ?? this.connection;
        const { database, schema, table } = element.data as SchemaTreeTableData & { connectionId?: string };

        const containerBase: Omit<SchemaTreeContainerData, "container"> = {
          connectionId: connection.id,
          database,
          schema,
          table: table.name,
          tableData: table,
        };

        return [
          {
            type: "container" as const,
            label: "Columns",
            description: `${table.columns.length}`,
            contextValue: "containerColumns",
            data: { ...containerBase, container: "columns" } as SchemaTreeContainerData,
          },
          {
            type: "container" as const,
            label: "Indexes",
            description: `${table.indexes.length}`,
            contextValue: "containerIndexes",
            data: { ...containerBase, container: "indexes" } as SchemaTreeContainerData,
          },
          {
            type: "container" as const,
            label: "Triggers",
            contextValue: "containerTriggers",
            data: { ...containerBase, container: "triggers" } as SchemaTreeContainerData,
          },
        ];
      }

      return [];
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unknown error";
      console.error("DatabaseExplorerProvider.getChildren error:", message);
      return [{ type: "error", label: `Error: ${message}` }];
    }
  }

  /**
   * Expand a container node into its children.
   */
  private async getContainerChildren(element: SchemaTreeItem): Promise<SchemaTreeItem[]> {
    const data = element.data as SchemaTreeContainerData & { _tables?: Table[] };
    const connection = this.resolveConnectionForElement(element) ?? this.connection;

    // Schema-level "Tables" container  → list table nodes
    if (element.contextValue === "containerTables") {
      const tables: Table[] = data._tables ?? [];
      if (tables.length === 0) {
        return [{ type: "message", label: "No tables found", contextValue: "message" }];
      }
      return tables.map((table) => ({
        type: "table" as const,
        label: table.name,
        data: {
          connectionId: data.connectionId,
          database: data.database,
          schema: data.schema,
          table,
        },
      }));
    }

    // Schema-level placeholder containers
    if (element.contextValue === "containerViews") {
      return [{ type: "message", label: "No views defined", contextValue: "message" }];
    }
    if (element.contextValue === "containerQueries") {
      return [{ type: "message", label: "Query history coming soon", contextValue: "message" }];
    }
    if (element.contextValue === "containerTypes") {
      return [{ type: "message", label: "No custom types defined", contextValue: "message" }];
    }

    // Table-level containers
    if (element.contextValue === "containerColumns") {
      if (!connection) return [];
      const columns = await this.schemaManager.getColumns(
        connection,
        data.database,
        data.schema,
        data.table
      );
      if (columns.length === 0) {
        return [{ type: "message", label: "No columns found", contextValue: "message" }];
      }
      return columns.map((column) => ({
        type: "column" as const,
        label: column.name,
        data: {
          database: data.database,
          schema: data.schema,
          table: data.table,
          column,
        } as SchemaTreeColumnData,
      }));
    }

    if (element.contextValue === "containerIndexes") {
      const table = data.tableData;
      if (!table || table.indexes.length === 0) {
        return [{ type: "message", label: "No indexes defined", contextValue: "message" }];
      }
      return table.indexes.map((index) => ({
        type: "index" as const,
        label: index.name,
        contextValue: "index",
        data: {
          database: data.database,
          schema: data.schema,
          table: data.table,
          index,
        } as SchemaTreeIndexData,
      }));
    }

    if (element.contextValue === "containerTriggers") {
      return [{ type: "message", label: "No triggers defined", contextValue: "message" }];
    }

    return [];
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
    if (element.type === "container") {
      const d = element.data as SchemaTreeContainerData;
      return `container-${d.connectionId ?? "default"}-${d.database}-${d.schema}-${d.table}-${element.contextValue}`;
    }
    if (element.type === "index") {
      const d = element.data as SchemaTreeIndexData;
      return `index-${d.database}-${d.schema}-${d.table}-${d.index.name}`;
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

  /** Return an appropriate ThemeIcon for each container kind */
  private getContainerIcon(container: SchemaTreeContainerData["container"]): vscode.ThemeIcon {
    switch (container) {
      case "columns":
        return new vscode.ThemeIcon("list-unordered");
      case "indexes":
        return new vscode.ThemeIcon("symbol-key");
      case "triggers":
        return new vscode.ThemeIcon("zap");
      case "views":
        return new vscode.ThemeIcon("eye");
      case "queries":
        return new vscode.ThemeIcon("search");
      case "types":
        return new vscode.ThemeIcon("symbol-class");
      default:
        return new vscode.ThemeIcon("folder");
    }
  }

  private getAccessibilityLabel(element: SchemaTreeItem): string {
    if (element.type === "connection") {
      const connection = element.data as Connection;
      return `Connection ${connection.settings.name}, ${connection.isActive ? "active" : "inactive"}, state ${connection.diagnostic.state}`;
    }
    if (element.type === "database") {
      return `Database ${element.label}`;
    }
    if (element.type === "schema") {
      return `Schema ${element.label}`;
    }
    if (element.type === "table") {
      const table = (element.data as SchemaTreeTableData).table;
      const rowPart = table.rowCount !== undefined ? `, ${formatRowCount(table.rowCount)}` : ", rows unknown";
      return `Table ${element.label}${rowPart}`;
    }
    if (element.type === "column") {
      const column = (element.data as SchemaTreeColumnData).column;
      return `Column ${column.name}, type ${getColumnTypeDisplay(column.type).label}${column.isPrimaryKey ? ", primary key" : ""}${column.nullable ? ", nullable" : ""}`;
    }
    if (element.type === "container") {
      return `${element.label} folder`;
    }
    if (element.type === "index") {
      const idx = (element.data as SchemaTreeIndexData).index;
      return `Index ${idx.name}, columns ${idx.columns.join(", ")}${idx.isPrimary ? ", primary" : ""}${idx.isUnique ? ", unique" : ""}`;
    }
    return element.label;
  }
}

export function createDatabaseExplorerProvider(extensionUri: vscode.Uri, schemaManager: SchemaManager): DatabaseExplorerProvider {
  return new DatabaseExplorerProvider(extensionUri, schemaManager);
}
