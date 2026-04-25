"use strict";
/**
 * DatabaseExplorer TreeView Provider
 * Displays database schema hierarchy: Databases -> Schemas -> Tables -> Columns
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
exports.DatabaseExplorerProvider = void 0;
exports.createDatabaseExplorerProvider = createDatabaseExplorerProvider;
const vscode = __importStar(require("vscode"));
const Schema_1 = require("../models/Schema");
const DatabaseExplorerTree_1 = require("./DatabaseExplorerTree");
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
/** Map diagnostic.state → VS Code codicon for inline status dot */
function connectionStateIcon(state) {
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
function formatRowCount(count) {
    if (count >= 1000000) {
        return `~${(count / 1000000).toFixed(1)}M rows`;
    }
    if (count >= 1000) {
        return `~${(count / 1000).toFixed(1)}K rows`;
    }
    return `${count} rows`;
}
// ---------------------------------------------------------------------------
class DatabaseExplorerProvider {
    constructor(extensionUri, schemaManager) {
        this._onDidChangeTreeData = new vscode.EventEmitter();
        this.onDidChangeTreeData = this._onDidChangeTreeData.event;
        this.connection = null;
        this.connections = [];
        this.expandedItems = new Set();
        this.extensionUri = extensionUri;
        this.schemaManager = schemaManager;
    }
    /**
     * Set the active connection
     */
    setConnection(connection) {
        this.connection = connection;
        this.expandedItems.clear();
        this.refresh();
    }
    setConnections(connections) {
        this.connections = [...connections];
        this.refresh();
    }
    /**
     * Refresh the tree view
     */
    refresh(item) {
        this._onDidChangeTreeData.fire(item);
    }
    /**
     * Get tree item
     */
    getTreeItem(element) {
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
        }
        else if (element.type === "error") {
            treeItem.iconPath = new vscode.ThemeIcon("error");
            // Show the full error on hover — labels truncate aggressively in the tree.
            treeItem.tooltip = element.label;
        }
        else if (element.type === "connection") {
            const connection = element.data;
            treeItem.label = connection.settings.name;
            // Use a ThemeIcon to show the health state; codicons in plain string labels render literally
            const iconId = connection.diagnostic.state === "verified" ? "pass-filled"
                : connection.diagnostic.state === "degraded" ? "warning"
                    : connection.diagnostic.state === "error" ? "error"
                        : "circle-large-outline";
            treeItem.iconPath = new vscode.ThemeIcon(iconId);
            treeItem.tooltip = new vscode.MarkdownString(`**${connection.settings.name}**\n\n` +
                `${connection.settings.baseUrl}\n\n` +
                `State: ${connection.diagnostic.state}` +
                (connection.diagnostic.message ? `\n\n${connection.diagnostic.message}` : ""));
        }
        else if (element.type === "database") {
            treeItem.iconPath = this.getMediaIcon("database");
        }
        else if (element.type === "schema") {
            treeItem.iconPath = this.getMediaIcon("schema");
        }
        else if (element.type === "table") {
            treeItem.iconPath = this.getMediaIcon("table");
            // S4-004: row count in description
            const table = element.data.table;
            if (table.rowCount !== undefined && table.rowCount !== null) {
                treeItem.description = formatRowCount(table.rowCount);
                treeItem.tooltip = `${table.name}\n${formatRowCount(table.rowCount)}${table.comment ? `\n${table.comment}` : ""}`;
            }
            else {
                treeItem.description = "(rows: unknown)";
                treeItem.tooltip = table.comment ?? table.name;
            }
        }
        else if (element.type === "column") {
            const column = element.data.column;
            const typeDisplay = (0, Schema_1.getColumnTypeDisplay)(column.type);
            treeItem.iconPath = this.getMediaIcon("column");
            treeItem.description = `${typeDisplay.label}${column.nullable ? " (null)" : ""}${column.isPrimaryKey ? " (PK)" : ""}`;
        }
        else if (element.type === "container") {
            // S4-002 / S4-003: folder containers
            const container = element.data.container;
            treeItem.iconPath = this.getContainerIcon(container);
        }
        else if (element.type === "index") {
            treeItem.iconPath = new vscode.ThemeIcon("symbol-key");
            const idx = element.data.index;
            const badges = [];
            if (idx.isPrimary)
                badges.push("PK");
            if (idx.isUnique)
                badges.push("UNIQUE");
            treeItem.description = badges.join(", ") || idx.columns.join(", ");
            treeItem.tooltip = `${idx.name}\nColumns: ${idx.columns.join(", ")}${idx.isUnique ? "\nUnique" : ""}${idx.isPrimary ? "\nPrimary" : ""}`;
        }
        else if (element.type === "trigger") {
            treeItem.iconPath = new vscode.ThemeIcon("zap");
        }
        else if (element.type === "view") {
            treeItem.iconPath = new vscode.ThemeIcon("eye");
        }
        else if (element.type === "message") {
            treeItem.iconPath = new vscode.ThemeIcon("info");
        }
        // -----------------------------------------------------------------------
        // Collapsible state
        // -----------------------------------------------------------------------
        if (element.type === "loading" || element.type === "error" || element.type === "message") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
        }
        else if (element.type === "connection") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
        }
        else if (element.type === "database") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
        }
        else if (element.type === "schema") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
        }
        else if (element.type === "table") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
        }
        else if (element.type === "column") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
        }
        else if (element.type === "container") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
        }
        else if (element.type === "index") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
        }
        else if (element.type === "trigger") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
        }
        else if (element.type === "view") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
        }
        return treeItem;
    }
    /**
     * Get children of an element
     */
    async getChildren(element) {
        try {
            if (!element) {
                if (this.connections.length === 0) {
                    // Empty children so `viewsWelcome` in package.json shows (Screenshot-2 style).
                    return [];
                }
                return this.connections.map((connection) => {
                    const presentation = (0, DatabaseExplorerTree_1.describeConnectionNode)(connection);
                    return {
                        type: "connection",
                        label: connection.settings.name,
                        description: presentation.description,
                        contextValue: presentation.contextValue,
                        data: connection,
                    };
                });
            }
            if (element.type === "connection") {
                const connection = element.data;
                if (!(0, DatabaseExplorerTree_1.shouldExpandConnectionToDatabases)(connection)) {
                    return [
                        {
                            type: "message",
                            label: (0, DatabaseExplorerTree_1.describeConnectionNode)(connection).browseMessage,
                            contextValue: "message",
                        },
                    ];
                }
                const databases = await this.schemaManager.getDatabases(connection);
                if (databases.length === 0) {
                    return [{ type: "message", label: "No databases found", contextValue: "message" }];
                }
                return databases.map((db) => ({
                    type: "database",
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
                    type: "schema",
                    label: schema.name || "default",
                    data: { connectionId: connection.id, database: database.name, schema },
                }));
            }
            // Schema level: show container nodes (S4-002)
            if (element.type === "schema") {
                const connection = this.resolveConnectionForElement(element) ?? this.connection;
                const { database, schema } = element.data;
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
                        type: "container",
                        label: "Tables",
                        description: `${visibleTables.length}`,
                        contextValue: "containerTables",
                        data: { ...schemaContainerBase, container: "columns", _tables: visibleTables },
                    },
                    // Views — placeholder
                    {
                        type: "container",
                        label: "Views",
                        contextValue: "containerViews",
                        data: { ...schemaContainerBase, container: "views" },
                    },
                    // Queries — placeholder
                    {
                        type: "container",
                        label: "Queries",
                        contextValue: "containerQueries",
                        data: { ...schemaContainerBase, container: "queries" },
                    },
                    // Types — placeholder
                    {
                        type: "container",
                        label: "Types",
                        contextValue: "containerTypes",
                        data: { ...schemaContainerBase, container: "types" },
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
                const { database, schema, table } = element.data;
                const containerBase = {
                    connectionId: connection.id,
                    database,
                    schema,
                    table: table.name,
                    tableData: table,
                };
                return [
                    {
                        type: "container",
                        label: "Columns",
                        description: `${table.columns.length}`,
                        contextValue: "containerColumns",
                        data: { ...containerBase, container: "columns" },
                    },
                    {
                        type: "container",
                        label: "Indexes",
                        description: `${table.indexes.length}`,
                        contextValue: "containerIndexes",
                        data: { ...containerBase, container: "indexes" },
                    },
                    {
                        type: "container",
                        label: "Triggers",
                        contextValue: "containerTriggers",
                        data: { ...containerBase, container: "triggers" },
                    },
                ];
            }
            return [];
        }
        catch (error) {
            const message = error instanceof Error ? error.message : "Unknown error";
            console.error("DatabaseExplorerProvider.getChildren error:", message);
            return [{ type: "error", label: `Error: ${message}` }];
        }
    }
    /**
     * Expand a container node into its children.
     */
    async getContainerChildren(element) {
        const data = element.data;
        const connection = this.resolveConnectionForElement(element) ?? this.connection;
        // Schema-level "Tables" container  → list table nodes
        if (element.contextValue === "containerTables") {
            const tables = data._tables ?? [];
            if (tables.length === 0) {
                return [{ type: "message", label: "No tables found", contextValue: "message" }];
            }
            return tables.map((table) => ({
                type: "table",
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
            if (!connection)
                return [];
            const columns = await this.schemaManager.getColumns(connection, data.database, data.schema, data.table);
            if (columns.length === 0) {
                return [{ type: "message", label: "No columns found", contextValue: "message" }];
            }
            return columns.map((column) => ({
                type: "column",
                label: column.name,
                data: {
                    database: data.database,
                    schema: data.schema,
                    table: data.table,
                    column,
                },
            }));
        }
        if (element.contextValue === "containerIndexes") {
            const table = data.tableData;
            if (!table || table.indexes.length === 0) {
                return [{ type: "message", label: "No indexes defined", contextValue: "message" }];
            }
            return table.indexes.map((index) => ({
                type: "index",
                label: index.name,
                contextValue: "index",
                data: {
                    database: data.database,
                    schema: data.schema,
                    table: data.table,
                    index,
                },
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
    getParent(element) {
        // Could implement if needed for performance
        return null;
    }
    /**
     * Get unique ID for an item
     */
    getItemId(element) {
        if (element.type === "database") {
            const data = element.data;
            const database = this.unwrapDatabase(data);
            const connectionId = this.unwrapConnectionId(data) ?? this.connection?.id ?? "default";
            return `db-${connectionId}-${database.name}`;
        }
        if (element.type === "connection") {
            return `connection-${element.data.id}`;
        }
        if (element.type === "schema") {
            const { connectionId, database, schema } = element.data;
            return `schema-${connectionId ?? this.connection?.id ?? "default"}-${database}-${schema.name}`;
        }
        if (element.type === "table") {
            const { connectionId, database, schema, table } = element.data;
            return `table-${connectionId ?? this.connection?.id ?? "default"}-${database}-${schema}-${table.name}`;
        }
        if (element.type === "column") {
            const { database, schema, table, column } = element.data;
            return `col-${database}-${schema}-${table}-${column.name}`;
        }
        if (element.type === "container") {
            const d = element.data;
            return `container-${d.connectionId ?? "default"}-${d.database}-${d.schema}-${d.table}-${element.contextValue}`;
        }
        if (element.type === "index") {
            const d = element.data;
            return `index-${d.database}-${d.schema}-${d.table}-${d.index.name}`;
        }
        return `${element.type}-${element.label}`;
    }
    resolveConnectionForElement(element) {
        const connectionId = this.unwrapConnectionId(element.data);
        if (!connectionId) {
            return this.connection;
        }
        return this.connections.find((connection) => connection.id === connectionId) ?? this.connection;
    }
    unwrapConnectionId(data) {
        if (!data || typeof data !== "object") {
            return undefined;
        }
        return "connectionId" in data ? data.connectionId : undefined;
    }
    unwrapDatabase(data) {
        return "database" in data ? data.database : data;
    }
    getMediaIcon(name) {
        return {
            light: vscode.Uri.joinPath(this.extensionUri, "media", `${name}-light.svg`),
            dark: vscode.Uri.joinPath(this.extensionUri, "media", `${name}-dark.svg`),
        };
    }
    /** Return an appropriate ThemeIcon for each container kind */
    getContainerIcon(container) {
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
    getAccessibilityLabel(element) {
        if (element.type === "connection") {
            const connection = element.data;
            return `Connection ${connection.settings.name}, ${connection.isActive ? "active" : "inactive"}, state ${connection.diagnostic.state}`;
        }
        if (element.type === "database") {
            return `Database ${element.label}`;
        }
        if (element.type === "schema") {
            return `Schema ${element.label}`;
        }
        if (element.type === "table") {
            const table = element.data.table;
            const rowPart = table.rowCount !== undefined ? `, ${formatRowCount(table.rowCount)}` : ", rows unknown";
            return `Table ${element.label}${rowPart}`;
        }
        if (element.type === "column") {
            const column = element.data.column;
            return `Column ${column.name}, type ${(0, Schema_1.getColumnTypeDisplay)(column.type).label}${column.isPrimaryKey ? ", primary key" : ""}${column.nullable ? ", nullable" : ""}`;
        }
        if (element.type === "container") {
            return `${element.label} folder`;
        }
        if (element.type === "index") {
            const idx = element.data.index;
            return `Index ${idx.name}, columns ${idx.columns.join(", ")}${idx.isPrimary ? ", primary" : ""}${idx.isUnique ? ", unique" : ""}`;
        }
        return element.label;
    }
}
exports.DatabaseExplorerProvider = DatabaseExplorerProvider;
function createDatabaseExplorerProvider(extensionUri, schemaManager) {
    return new DatabaseExplorerProvider(extensionUri, schemaManager);
}
//# sourceMappingURL=DatabaseExplorerProvider.js.map