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
        treeItem.description = element.type === "column" ? element.data?.type : element.description;
        treeItem.accessibilityInformation = {
            label: this.getAccessibilityLabel(element),
            role: "treeitem",
        };
        // Icons
        if (element.type === "loading") {
            treeItem.iconPath = new vscode.ThemeIcon("loading~spin");
        }
        else if (element.type === "error") {
            treeItem.iconPath = new vscode.ThemeIcon("error");
        }
        else if (element.type === "connection") {
            treeItem.iconPath = this.getMediaIcon(element.data.isActive ? "connection-active" : "connection-inactive");
            treeItem.tooltip = `${element.data.settings.name}\n${element.data.settings.baseUrl}`;
        }
        else if (element.type === "database") {
            treeItem.iconPath = this.getMediaIcon("database");
        }
        else if (element.type === "schema") {
            treeItem.iconPath = this.getMediaIcon("schema");
        }
        else if (element.type === "table") {
            treeItem.iconPath = this.getMediaIcon("table");
        }
        else if (element.type === "column") {
            const column = element.data;
            const typeDisplay = (0, Schema_1.getColumnTypeDisplay)(column.type);
            treeItem.iconPath = this.getMediaIcon("column");
            treeItem.description = `${typeDisplay.label}${column.nullable ? " (null)" : ""}${column.isPrimaryKey ? " (PK)" : ""}`;
        }
        else if (element.type === "emptyState") {
            treeItem.iconPath = new vscode.ThemeIcon("add");
            treeItem.tooltip = "Create a new VoltNueronGrid connection";
        }
        else if (element.type === "message") {
            treeItem.iconPath = new vscode.ThemeIcon("info");
        }
        // Collapsible state
        if (element.type === "loading" || element.type === "error" || element.type === "emptyState" || element.type === "message") {
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
        return treeItem;
    }
    /**
     * Get children of an element
     */
    async getChildren(element) {
        try {
            if (!element) {
                if (this.connections.length === 0) {
                    return [
                        {
                            type: "emptyState",
                            label: (0, DatabaseExplorerTree_1.getEmptyConnectionMessage)(),
                            contextValue: "emptyState",
                            command: {
                                command: "vng.newConnection",
                                title: "Create New Connection",
                            },
                        },
                    ];
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
            // Schema level: show tables
            if (element.type === "schema") {
                const connection = this.resolveConnectionForElement(element) ?? this.connection;
                const { database, schema } = element.data;
                const tables = await this.schemaManager.getTables(connection, database, schema.name);
                if (tables.length === 0) {
                    return [{ type: "message", label: "No tables found", contextValue: "message" }];
                }
                return tables
                    .filter((t) => !t.isSystem) // Hide system tables by default
                    .map((table) => ({
                    type: "table",
                    label: table.name,
                    data: { connectionId: connection.id, database, schema: schema.name, table },
                }));
            }
            // Table level: show columns
            if (element.type === "table") {
                const connection = this.resolveConnectionForElement(element) ?? this.connection;
                const { database, schema, table } = element.data;
                const columns = await this.schemaManager.getColumns(connection, database, schema, table.name);
                if (columns.length === 0) {
                    return [{ type: "message", label: "No columns found", contextValue: "message" }];
                }
                return columns.map((column) => ({
                    type: "column",
                    label: column.name,
                    data: {
                        database,
                        schema,
                        table: table.name,
                        column,
                    },
                }));
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
    getAccessibilityLabel(element) {
        if (element.type === "connection") {
            const connection = element.data;
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
            const column = element.data.column;
            return `Column ${column.name}, type ${(0, Schema_1.getColumnTypeDisplay)(column.type).label}${column.isPrimaryKey ? ", primary key" : ""}${column.nullable ? ", nullable" : ""}`;
        }
        if (element.type === "emptyState") {
            return "No connections available. Activate create new connection.";
        }
        return element.label;
    }
}
exports.DatabaseExplorerProvider = DatabaseExplorerProvider;
function createDatabaseExplorerProvider(extensionUri, schemaManager) {
    return new DatabaseExplorerProvider(extensionUri, schemaManager);
}
//# sourceMappingURL=DatabaseExplorerProvider.js.map