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
class DatabaseExplorerProvider {
    constructor(schemaManager) {
        this._onDidChangeTreeData = new vscode.EventEmitter();
        this.onDidChangeTreeData = this._onDidChangeTreeData.event;
        this.connection = null;
        this.expandedItems = new Set();
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
        treeItem.contextValue = element.type;
        treeItem.description = element.type === "column" ? element.data?.type : undefined;
        // Icons
        if (element.type === "loading") {
            treeItem.iconPath = new vscode.ThemeIcon("loading~spin");
        }
        else if (element.type === "error") {
            treeItem.iconPath = new vscode.ThemeIcon("error");
        }
        else if (element.type === "database") {
            treeItem.iconPath = new vscode.ThemeIcon("database");
        }
        else if (element.type === "schema") {
            treeItem.iconPath = new vscode.ThemeIcon("folder");
        }
        else if (element.type === "table") {
            treeItem.iconPath = new vscode.ThemeIcon("table");
        }
        else if (element.type === "column") {
            const column = element.data;
            const typeDisplay = (0, Schema_1.getColumnTypeDisplay)(column.type);
            treeItem.iconPath = new vscode.ThemeIcon("symbol-field");
            treeItem.description = `${typeDisplay.label}${column.nullable ? " (null)" : ""}${column.isPrimaryKey ? " (PK)" : ""}`;
        }
        // Collapsible state
        if (element.type === "loading" || element.type === "error") {
            treeItem.collapsibleState = vscode.TreeItemCollapsibleState.None;
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
                    type: "database",
                    label: db.name,
                    data: db,
                }));
            }
            // Database level: show schemas
            if (element.type === "database") {
                const database = element.data;
                const schemas = await this.schemaManager.getSchemas(this.connection, database.name);
                if (schemas.length === 0) {
                    return [{ type: "error", label: "No schemas found" }];
                }
                return schemas.map((schema) => ({
                    type: "schema",
                    label: schema.name || "default",
                    data: { database: database.name, schema },
                }));
            }
            // Schema level: show tables
            if (element.type === "schema") {
                const { database, schema } = element.data;
                const tables = await this.schemaManager.getTables(this.connection, database, schema.name);
                if (tables.length === 0) {
                    return [{ type: "error", label: "No tables found" }];
                }
                return tables
                    .filter((t) => !t.isSystem) // Hide system tables by default
                    .map((table) => ({
                    type: "table",
                    label: table.name,
                    data: { database, schema: schema.name, table },
                }));
            }
            // Table level: show columns
            if (element.type === "table") {
                const { database, schema, table } = element.data;
                const columns = await this.schemaManager.getColumns(this.connection, database, schema, table.name);
                if (columns.length === 0) {
                    return [{ type: "error", label: "No columns found" }];
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
            return `db-${element.data.name}`;
        }
        if (element.type === "schema") {
            const { database, schema } = element.data;
            return `schema-${database}-${schema.name}`;
        }
        if (element.type === "table") {
            const { database, schema, table } = element.data;
            return `table-${database}-${schema}-${table.name}`;
        }
        if (element.type === "column") {
            const { database, schema, table, column } = element.data;
            return `col-${database}-${schema}-${table}-${column.name}`;
        }
        return `${element.type}-${element.label}`;
    }
}
exports.DatabaseExplorerProvider = DatabaseExplorerProvider;
function createDatabaseExplorerProvider(schemaManager) {
    return new DatabaseExplorerProvider(schemaManager);
}
//# sourceMappingURL=DatabaseExplorerProvider.js.map