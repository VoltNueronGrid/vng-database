"use strict";
/**
 * SchemaManager: fetch and cache database schema
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
exports.SchemaManager = void 0;
exports.createSchemaManager = createSchemaManager;
const vscode = __importStar(require("vscode"));
const Schema_1 = require("../models/Schema");
class SchemaManager {
    constructor(httpClient) {
        this.cache = new Map();
        this.cacheConfig = {
            enabled: true,
            ttlMs: 5 * 60 * 1000,
        };
        this.httpClient = httpClient;
        this.refreshCacheConfig();
        this.configDisposable = vscode.workspace.onDidChangeConfiguration((event) => {
            if (event.affectsConfiguration("voltnuerongrid.schema.cache")) {
                this.refreshCacheConfig();
            }
        });
    }
    /**
     * Get schema registry for a connection (with caching)
     */
    async getSchemaRegistry(connection, ignoreCache = false) {
        const cacheKey = connection.id;
        // Check cache
        if (!ignoreCache && this.cacheConfig.enabled) {
            const cached = this.cache.get(cacheKey);
            if (cached && Date.now() - cached.timestamp < this.cacheConfig.ttlMs) {
                return cached.data;
            }
        }
        try {
            const response = await this.httpClient.getSchemaRegistry(connection);
            if (response.status !== 200) {
                // Surface the server body when present — tree labels truncate, but
                // tooltips carry the full message so the root cause stays visible.
                let bodySnippet = "";
                if (response.data && typeof response.data === "object") {
                    try {
                        bodySnippet = ` — ${JSON.stringify(response.data).slice(0, 240)}`;
                    }
                    catch { /* ignore */ }
                }
                else if (typeof response.data === "string" && response.data.length > 0) {
                    bodySnippet = ` — ${response.data.slice(0, 240)}`;
                }
                const detail = response.error ?? `HTTP ${response.status}${bodySnippet}`;
                throw new Error(`Failed to fetch schema: ${detail}`);
            }
            const raw = response.data;
            const databases = Array.isArray(raw?.databases) ? raw.databases.map(normalizeDatabase) : [];
            const registry = {
                databases,
                timestamp: Date.now(),
            };
            // Cache result
            if (this.cacheConfig.enabled) {
                this.cache.set(cacheKey, { data: registry, timestamp: Date.now() });
            }
            return registry;
        }
        catch (error) {
            throw new Error(`Schema fetch error: ${error instanceof Error ? error.message : String(error)}`);
        }
    }
    /**
     * Get all databases
     */
    async getDatabases(connection) {
        const registry = await this.getSchemaRegistry(connection);
        return registry.databases;
    }
    /**
     * Get schemas for a database
     */
    async getSchemas(connection, databaseName) {
        const registry = await this.getSchemaRegistry(connection);
        const database = registry.databases.find((db) => db.name === databaseName);
        return database?.schemas || [];
    }
    /**
     * Get tables for a schema
     */
    async getTables(connection, databaseName, schemaName) {
        const schemas = await this.getSchemas(connection, databaseName);
        const schema = schemas.find((s) => s.name === schemaName);
        return schema?.tables || [];
    }
    /**
     * Get table details
     */
    async getTable(connection, databaseName, schemaName, tableName) {
        const tables = await this.getTables(connection, databaseName, schemaName);
        return tables.find((t) => t.name === tableName) || null;
    }
    /**
     * Get columns for a table
     */
    async getColumns(connection, databaseName, schemaName, tableName) {
        const table = await this.getTable(connection, databaseName, schemaName, tableName);
        return table?.columns || [];
    }
    /**
     * Search for tables across all schemas
     */
    async searchTables(connection, query) {
        const registry = await this.getSchemaRegistry(connection);
        const results = [];
        const lowerQuery = query.toLowerCase();
        for (const database of registry.databases) {
            for (const schema of database.schemas) {
                for (const table of schema.tables) {
                    if (table.name.toLowerCase().includes(lowerQuery)) {
                        results.push({ database: database.name, schema: schema.name, table });
                    }
                }
            }
        }
        return results;
    }
    /**
     * Search for columns across all tables
     */
    async searchColumns(connection, query) {
        const registry = await this.getSchemaRegistry(connection);
        const results = [];
        const lowerQuery = query.toLowerCase();
        for (const database of registry.databases) {
            for (const schema of database.schemas) {
                for (const table of schema.tables) {
                    for (const column of table.columns) {
                        if (column.name.toLowerCase().includes(lowerQuery)) {
                            results.push({
                                database: database.name,
                                schema: schema.name,
                                table: table.name,
                                column,
                            });
                        }
                    }
                }
            }
        }
        return results;
    }
    /**
     * Get suggested column names for autocomplete
     */
    async getColumnNames(connection, databaseName, schemaName, tableName) {
        const columns = await this.getColumns(connection, databaseName, schemaName, tableName);
        return columns.map((col) => col.name);
    }
    /**
     * Get suggested table names for autocomplete
     */
    async getTableNames(connection, databaseName, schemaName) {
        const tables = await this.getTables(connection, databaseName, schemaName);
        return tables.map((t) => t.name);
    }
    /**
     * Invalidate cache for a connection
     */
    invalidateCache(connectionId) {
        this.cache.delete(connectionId);
    }
    /**
     * Clear all caches
     */
    clearCache() {
        this.cache.clear();
    }
    /**
     * Dispose configuration listeners
     */
    dispose() {
        this.configDisposable.dispose();
    }
    /**
     * Get cache stats for debugging
     */
    getCacheStats() {
        const now = Date.now();
        return Array.from(this.cache.entries()).map(([connectionId, { timestamp }]) => ({
            connectionId,
            age: now - timestamp,
        }));
    }
    refreshCacheConfig() {
        const config = vscode.workspace.getConfiguration("voltnuerongrid");
        const enabled = config.get("schema.cache.enabled", true);
        const ttlSeconds = config.get("schema.cache.ttlSeconds", 300);
        this.cacheConfig = {
            enabled,
            ttlMs: Math.max(5, ttlSeconds) * 1000,
        };
        if (!enabled) {
            this.cache.clear();
        }
    }
}
exports.SchemaManager = SchemaManager;
function createSchemaManager(httpClient) {
    return new SchemaManager(httpClient);
}
// ── Server → TypeScript model normalizers ─────────────────────────────────────
// The server uses snake_case (data_type, primary_key) while our TS model uses
// camelCase (type, isPrimaryKey). Map them here so all downstream code works.
function normalizeColumn(raw) {
    const typeStr = String(raw["data_type"] ?? raw["type"] ?? "UNKNOWN");
    return {
        name: String(raw["name"] ?? ""),
        type: (0, Schema_1.parseColumnType)(typeStr),
        nullable: Boolean(raw["nullable"] ?? true),
        isPrimaryKey: Boolean(raw["primary_key"] ?? raw["isPrimaryKey"] ?? false),
        isUnique: Boolean(raw["is_unique"] ?? raw["isUnique"] ?? false),
        isForeignKey: Boolean(raw["is_foreign_key"] ?? raw["isForeignKey"] ?? false),
        defaultValue: raw["default_value"] != null ? String(raw["default_value"]) : raw["defaultValue"] != null ? String(raw["defaultValue"]) : undefined,
        comment: raw["comment"] != null ? String(raw["comment"]) : undefined,
    };
}
function normalizeTable(raw) {
    const rawCols = Array.isArray(raw["columns"]) ? raw["columns"] : [];
    const rawIdxs = Array.isArray(raw["indexes"]) ? raw["indexes"] : [];
    return {
        name: String(raw["name"] ?? ""),
        schema: String(raw["schema"] ?? "public"),
        columns: rawCols.map(normalizeColumn),
        indexes: rawIdxs.map((idx) => ({
            name: String(idx["name"] ?? ""),
            columns: Array.isArray(idx["columns"]) ? idx["columns"].map(String) : [],
            isUnique: Boolean(idx["is_unique"] ?? idx["isUnique"] ?? false),
            isPrimary: Boolean(idx["is_primary"] ?? idx["isPrimary"] ?? false),
        })),
        comment: raw["comment"] != null ? String(raw["comment"]) : undefined,
        rowCount: raw["row_count"] != null ? Number(raw["row_count"]) : raw["rowCount"] != null ? Number(raw["rowCount"]) : undefined,
        isSystem: Boolean(raw["is_system"] ?? raw["isSystem"] ?? false),
    };
}
function normalizeSchema(raw) {
    const rawTables = Array.isArray(raw["tables"]) ? raw["tables"] : [];
    return {
        name: String(raw["name"] ?? "public"),
        database: String(raw["database"] ?? "default"),
        tables: rawTables.map(normalizeTable),
    };
}
function normalizeDatabase(raw) {
    const rawSchemas = Array.isArray(raw["schemas"]) ? raw["schemas"] : [];
    return {
        name: String(raw["name"] ?? "default"),
        schemas: rawSchemas.map(normalizeSchema),
    };
}
//# sourceMappingURL=SchemaManager.js.map