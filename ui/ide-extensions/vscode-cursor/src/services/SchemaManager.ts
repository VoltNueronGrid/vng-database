/**
 * SchemaManager: fetch and cache database schema
 */

import * as vscode from "vscode";
import { Connection } from "../models/Connection";
import { Database, Schema, Table, Column, SchemaRegistry, parseColumnType } from "../models/Schema";
import { HttpClient } from "./HttpClient";

interface SchemaCacheConfig {
  enabled: boolean;
  ttlMs: number;
}

export class SchemaManager {
  private httpClient: HttpClient;
  private cache: Map<string, { data: SchemaRegistry; timestamp: number }> = new Map();
  private cacheConfig: SchemaCacheConfig = {
    enabled: true,
    ttlMs: 5 * 60 * 1000,
  };
  private readonly configDisposable: vscode.Disposable;

  constructor(httpClient: HttpClient) {
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
  async getSchemaRegistry(connection: Connection, ignoreCache = false): Promise<SchemaRegistry> {
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
        const detail = response.error ?? `HTTP ${response.status}`;
        throw new Error(`Failed to fetch schema: ${detail}`);
      }

      const raw = response.data as Record<string, unknown> | undefined;
      const databases = Array.isArray(raw?.databases) ? (raw!.databases as Record<string, unknown>[]).map(normalizeDatabase) : [];
      const registry: SchemaRegistry = {
        databases,
        timestamp: Date.now(),
      };

      // Cache result
      if (this.cacheConfig.enabled) {
        this.cache.set(cacheKey, { data: registry, timestamp: Date.now() });
      }

      return registry;
    } catch (error) {
      throw new Error(`Schema fetch error: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  /**
   * Get all databases
   */
  async getDatabases(connection: Connection): Promise<Database[]> {
    const registry = await this.getSchemaRegistry(connection);
    return registry.databases;
  }

  /**
   * Get schemas for a database
   */
  async getSchemas(connection: Connection, databaseName: string): Promise<Schema[]> {
    const registry = await this.getSchemaRegistry(connection);
    const database = registry.databases.find((db) => db.name === databaseName);
    return database?.schemas || [];
  }

  /**
   * Get tables for a schema
   */
  async getTables(connection: Connection, databaseName: string, schemaName: string): Promise<Table[]> {
    const schemas = await this.getSchemas(connection, databaseName);
    const schema = schemas.find((s) => s.name === schemaName);
    return schema?.tables || [];
  }

  /**
   * Get table details
   */
  async getTable(
    connection: Connection,
    databaseName: string,
    schemaName: string,
    tableName: string
  ): Promise<Table | null> {
    const tables = await this.getTables(connection, databaseName, schemaName);
    return tables.find((t) => t.name === tableName) || null;
  }

  /**
   * Get columns for a table
   */
  async getColumns(
    connection: Connection,
    databaseName: string,
    schemaName: string,
    tableName: string
  ): Promise<Column[]> {
    const table = await this.getTable(connection, databaseName, schemaName, tableName);
    return table?.columns || [];
  }

  /**
   * Search for tables across all schemas
   */
  async searchTables(connection: Connection, query: string): Promise<Array<{ database: string; schema: string; table: Table }>> {
    const registry = await this.getSchemaRegistry(connection);
    const results: Array<{ database: string; schema: string; table: Table }> = [];
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
  async searchColumns(
    connection: Connection,
    query: string
  ): Promise<Array<{ database: string; schema: string; table: string; column: Column }>> {
    const registry = await this.getSchemaRegistry(connection);
    const results: Array<{ database: string; schema: string; table: string; column: Column }> = [];
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
  async getColumnNames(
    connection: Connection,
    databaseName: string,
    schemaName: string,
    tableName: string
  ): Promise<string[]> {
    const columns = await this.getColumns(connection, databaseName, schemaName, tableName);
    return columns.map((col) => col.name);
  }

  /**
   * Get suggested table names for autocomplete
   */
  async getTableNames(connection: Connection, databaseName: string, schemaName: string): Promise<string[]> {
    const tables = await this.getTables(connection, databaseName, schemaName);
    return tables.map((t) => t.name);
  }

  /**
   * Invalidate cache for a connection
   */
  invalidateCache(connectionId: string): void {
    this.cache.delete(connectionId);
  }

  /**
   * Clear all caches
   */
  clearCache(): void {
    this.cache.clear();
  }

  /**
   * Dispose configuration listeners
   */
  dispose(): void {
    this.configDisposable.dispose();
  }

  /**
   * Get cache stats for debugging
   */
  getCacheStats(): { connectionId: string; age: number }[] {
    const now = Date.now();
    return Array.from(this.cache.entries()).map(([connectionId, { timestamp }]) => ({
      connectionId,
      age: now - timestamp,
    }));
  }

  private refreshCacheConfig(): void {
    const config = vscode.workspace.getConfiguration("voltnuerongrid");
    const enabled = config.get<boolean>("schema.cache.enabled", true);
    const ttlSeconds = config.get<number>("schema.cache.ttlSeconds", 300);
    this.cacheConfig = {
      enabled,
      ttlMs: Math.max(5, ttlSeconds) * 1000,
    };

    if (!enabled) {
      this.cache.clear();
    }
  }
}

export function createSchemaManager(httpClient: HttpClient): SchemaManager {
  return new SchemaManager(httpClient);
}

// ── Server → TypeScript model normalizers ─────────────────────────────────────
// The server uses snake_case (data_type, primary_key) while our TS model uses
// camelCase (type, isPrimaryKey). Map them here so all downstream code works.

function normalizeColumn(raw: Record<string, unknown>): Column {
  const typeStr = String(raw["data_type"] ?? raw["type"] ?? "UNKNOWN");
  return {
    name: String(raw["name"] ?? ""),
    type: parseColumnType(typeStr),
    nullable: Boolean(raw["nullable"] ?? true),
    isPrimaryKey: Boolean(raw["primary_key"] ?? raw["isPrimaryKey"] ?? false),
    isUnique: Boolean(raw["is_unique"] ?? raw["isUnique"] ?? false),
    isForeignKey: Boolean(raw["is_foreign_key"] ?? raw["isForeignKey"] ?? false),
    defaultValue: raw["default_value"] != null ? String(raw["default_value"]) : raw["defaultValue"] != null ? String(raw["defaultValue"]) : undefined,
    comment: raw["comment"] != null ? String(raw["comment"]) : undefined,
  };
}

function normalizeTable(raw: Record<string, unknown>): Table {
  const rawCols = Array.isArray(raw["columns"]) ? (raw["columns"] as Record<string, unknown>[]) : [];
  const rawIdxs = Array.isArray(raw["indexes"]) ? (raw["indexes"] as Record<string, unknown>[]) : [];
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

function normalizeSchema(raw: Record<string, unknown>): Schema {
  const rawTables = Array.isArray(raw["tables"]) ? (raw["tables"] as Record<string, unknown>[]) : [];
  return {
    name: String(raw["name"] ?? "public"),
    database: String(raw["database"] ?? "default"),
    tables: rawTables.map(normalizeTable),
  };
}

function normalizeDatabase(raw: Record<string, unknown>): Database {
  const rawSchemas = Array.isArray(raw["schemas"]) ? (raw["schemas"] as Record<string, unknown>[]) : [];
  return {
    name: String(raw["name"] ?? "default"),
    schemas: rawSchemas.map(normalizeSchema),
  };
}
