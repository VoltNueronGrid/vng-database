/**
 * QueryExecutionService: execute queries and manage results
 */

import type * as vscode from "vscode";
import { Connection } from "../models/Connection";
import { QueryResult, QueryHistoryEntry, parseQueryResult } from "../models/QueryResult";
import { HttpClient } from "./HttpClient";
import { createQueryHistoryEntry, findOldestHistoryEntryId } from "./QueryHistory";

export interface QueryExecutionOptions {
  timeoutMs?: number;
  executionId?: string;
  signal?: AbortSignal;
}

export interface QueryStreamOptions extends QueryExecutionOptions {
  onResult?: (result: QueryResult, index: number, total: number) => Promise<void> | void;
  stopOnError?: boolean;
}

interface QueryResultCacheConfig {
  enabled: boolean;
  ttlMs: number;
  maxEntries: number;
}

interface WorkspaceConfigApi {
  getConfiguration: (section: string) => {
    get: <T>(path: string, defaultValue: T) => T;
  };
  onDidChangeConfiguration: (listener: (event: { affectsConfiguration: (section: string) => boolean }) => void) => {
    dispose: () => void;
  };
}

export class QueryExecutionService {
  private httpClient: HttpClient;
  private queryHistory: Map<string, QueryHistoryEntry> = new Map();
  private queryResultCache: Map<string, { result: QueryResult; timestamp: number }> = new Map();
  private maxHistorySize = 100;
  private readonly historyStorageKey = "vng.queryHistory";
  private readonly activeExecutions: Map<string, AbortController> = new Map();
  private executionSequence = 0;
  private readonly configDisposable?: vscode.Disposable;
  private queryCacheConfig: QueryResultCacheConfig = {
    enabled: true,
    ttlMs: 30 * 1000,
    maxEntries: 100,
  };

  constructor(httpClient: HttpClient, private readonly context?: vscode.ExtensionContext) {
    this.httpClient = httpClient;
    this.refreshQueryCacheConfig();

    const workspace = getWorkspace();
    if (workspace) {
      this.configDisposable = workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration("voltnuerongrid.query.cache")) {
          this.refreshQueryCacheConfig();
        }
      });
    }
  }

  async initialize(): Promise<void> {
    if (!this.context) {
      return;
    }

    const stored = this.context.globalState.get<QueryHistoryEntry[]>(this.historyStorageKey, []);
    this.queryHistory.clear();
    for (const entry of stored.slice(0, this.maxHistorySize)) {
      this.queryHistory.set(entry.id, entry);
    }
  }

  /**
   * Execute a query
   */
  async executeQuery(connection: Connection, query: string, options?: QueryExecutionOptions): Promise<QueryResult> {
    const startTime = Date.now();
    const resultId = options?.executionId ?? this.createExecutionId("result", startTime);
    const timeoutMs = options?.timeoutMs ?? connection.settings.advanced.connectionTimeout ?? 30000;
    const controller = new AbortController();
    const cacheKey = this.createQueryCacheKey(connection.id, query);

    const handleAbort = () => controller.abort();
    if (options?.signal) {
      if (options.signal.aborted) {
        controller.abort();
      } else {
        options.signal.addEventListener("abort", handleAbort, { once: true });
      }
    }

    this.activeExecutions.set(resultId, controller);

    try {
      // Validate query
      if (!query || query.trim().length === 0) {
        const invalidResult: QueryResult = {
          id: resultId,
          query,
          status: "error",
          rows: [],
          columns: [],
          rowCount: 0,
          executionTime: 0,
          timestamp: startTime,
          error: {
            message: "Query cannot be empty",
            code: "EMPTY_QUERY",
          },
        };
        await this.addToHistory(connection.id, resultId, query, invalidResult);
        return invalidResult;
      }

      const cachedResult = this.getCachedResult(cacheKey, resultId, query, startTime);
      if (cachedResult) {
        await this.addToHistory(connection.id, resultId, query, cachedResult);
        return cachedResult;
      }

      // Execute query
      const response = await this.httpClient.executeQuery(connection, query, {
        timeoutMs,
        signal: controller.signal,
        requestId: resultId,
      });
      const executionTime = Date.now() - startTime;

      let result: QueryResult;
      if (response.status === 200) {
        result = parseQueryResult(query, response.data, executionTime);
        result.id = resultId;
      } else {
        const responseError = (response.error || "").toLowerCase();
        const isTimeout = responseError.includes("timeout");
        const isCancelled = responseError.includes("aborted") || responseError.includes("abort");
        result = {
          id: resultId,
          query,
          status: isCancelled ? "cancelled" : "error",
          rows: [],
          columns: [],
          rowCount: 0,
          executionTime,
          timestamp: Date.now(),
          error: {
            message: response.error || "Query execution failed",
            code: isTimeout ? "TIMEOUT" : isCancelled ? "CANCELLED" : String(response.status),
            detail: response.data?.detail,
          },
        };
      }

      // Add to history
      await this.addToHistory(connection.id, resultId, query, result);
      this.storeCachedResult(cacheKey, result);

      return result;
    } catch (error) {
      const executionTime = Date.now() - startTime;
      const errorMessage = error instanceof Error ? error.message : "Unknown error";
      const isCancelled = errorMessage.toLowerCase().includes("aborted") || errorMessage.toLowerCase().includes("abort");
      const isTimeout = errorMessage.toLowerCase().includes("timeout");

      const result: QueryResult = {
        id: resultId,
        query,
        status: isCancelled ? "cancelled" : "error",
        rows: [],
        columns: [],
        rowCount: 0,
        executionTime,
        timestamp: Date.now(),
        error: {
          message: errorMessage,
          code: isTimeout ? "TIMEOUT" : isCancelled ? "CANCELLED" : "EXECUTION_ERROR",
        },
      };

      await this.addToHistory(connection.id, resultId, query, result);
      return result;
    } finally {
      this.activeExecutions.delete(resultId);
      if (options?.signal) {
        options.signal.removeEventListener("abort", handleAbort);
      }
    }
  }

  /**
   * Execute multiple queries
   */
  async executeMultiple(connection: Connection, queries: string[], options?: QueryExecutionOptions): Promise<QueryResult[]> {
    const results: QueryResult[] = [];
    const executionGroupId = options?.executionId;

    for (let index = 0; index < queries.length; index += 1) {
      const query = queries[index];
      const result = await this.executeQuery(connection, query, {
        ...options,
        executionId: executionGroupId ? `${executionGroupId}-${index + 1}` : options?.executionId,
      });
      results.push(result);
    }
    return results;
  }

  async executeStatementsStream(
    connection: Connection,
    sqlOrStatements: string | string[],
    options?: QueryStreamOptions
  ): Promise<QueryResult[]> {
    const statements = Array.isArray(sqlOrStatements) ? sqlOrStatements : this.parseStatements(sqlOrStatements);
    const total = statements.length;
    const results: QueryResult[] = [];
    const stopOnError = options?.stopOnError ?? true;
    const streamId = options?.executionId ?? this.createExecutionId("stream");

    for (let index = 0; index < statements.length; index += 1) {
      const statement = statements[index];
      const result = await this.executeQuery(connection, statement, {
        timeoutMs: options?.timeoutMs,
        signal: options?.signal,
        executionId: `${streamId}-${index + 1}`,
      });

      results.push(result);

      if (options?.onResult) {
        await options.onResult(result, index + 1, total);
      }

      if (result.status !== "success" && stopOnError) {
        break;
      }
    }

    return results;
  }

  /**
   * Parse and split multiple statements
   */
  parseStatements(sql: string): string[] {
    return sql
      .split(";")
      .map((s) => s.trim())
      .filter((s) => s.length > 0)
      .map((s) => s + ";");
  }

  /**
   * Add query to history
   */
  private async addToHistory(
    connectionId: string,
    resultId: string,
    query: string,
    result: QueryResult
  ): Promise<void> {
    const entry = createQueryHistoryEntry(connectionId, resultId, query, result);

    this.queryHistory.set(entry.id, entry);

    // Trim history to max size
    if (this.queryHistory.size > this.maxHistorySize) {
      const oldestEntryId = findOldestHistoryEntryId(this.queryHistory.entries());
      if (oldestEntryId) {
        this.queryHistory.delete(oldestEntryId);
      }
    }

    await this.persistHistory();
  }

  /**
   * Get query history for a connection
   */
  getHistory(connectionId?: string): QueryHistoryEntry[] {
    return Array.from(this.queryHistory.values())
      .filter((entry) => !connectionId || entry.connectionId === connectionId)
      .sort((a, b) => b.timestamp - a.timestamp);
  }

  /**
   * Get history entry by ID
   */
  getHistoryEntry(id: string): QueryHistoryEntry | null {
    return this.queryHistory.get(id) || null;
  }

  /**
   * Clear history
   */
  async clearHistory(connectionId?: string): Promise<void> {
    if (!connectionId) {
      this.queryHistory.clear();
    } else {
      const toDelete = Array.from(this.queryHistory.entries())
        .filter(([, entry]) => entry.connectionId === connectionId)
        .map(([id]) => id);
      toDelete.forEach((id) => this.queryHistory.delete(id));
    }

    await this.persistHistory();
  }

  /**
   * Search history
   */
  searchHistory(query: string, connectionId?: string): QueryHistoryEntry[] {
    const lowerQuery = query.toLowerCase();
    return this.getHistory(connectionId).filter((entry) =>
      entry.query.toLowerCase().includes(lowerQuery)
    );
  }

  cancelExecution(executionId: string): boolean {
    const controller = this.activeExecutions.get(executionId);
    if (!controller) {
      return false;
    }
    controller.abort();
    return true;
  }

  cancelAllExecutions(): number {
    const count = this.activeExecutions.size;
    for (const controller of this.activeExecutions.values()) {
      controller.abort();
    }
    return count;
  }

  getActiveExecutionIds(): string[] {
    return Array.from(this.activeExecutions.keys());
  }

  dispose(): void {
    this.configDisposable?.dispose();
  }

  private createExecutionId(prefix: string, timestamp = Date.now()): string {
    this.executionSequence += 1;
    return `${prefix}-${timestamp}-${this.executionSequence}`;
  }

  private async persistHistory(): Promise<void> {
    if (!this.context) {
      return;
    }

    const snapshot = this.getHistory();
    await this.context.globalState.update(this.historyStorageKey, snapshot);
  }

  private createQueryCacheKey(connectionId: string, query: string): string {
    const normalized = query
      .trim()
      .replace(/\s+/g, " ")
      .replace(/\s*;\s*$/, ";");

    return `${connectionId}::${normalized}`;
  }

  private getCachedResult(cacheKey: string, resultId: string, query: string, startTime: number): QueryResult | undefined {
    if (!this.queryCacheConfig.enabled) {
      return undefined;
    }

    const cached = this.queryResultCache.get(cacheKey);
    if (!cached) {
      return undefined;
    }

    if (Date.now() - cached.timestamp > this.queryCacheConfig.ttlMs) {
      this.queryResultCache.delete(cacheKey);
      return undefined;
    }

    return {
      ...cached.result,
      id: resultId,
      query,
      executionTime: Math.min(cached.result.executionTime, 1),
      timestamp: startTime,
    };
  }

  private storeCachedResult(cacheKey: string, result: QueryResult): void {
    if (!this.queryCacheConfig.enabled || result.status !== "success") {
      return;
    }

    this.queryResultCache.set(cacheKey, {
      result: { ...result },
      timestamp: Date.now(),
    });

    if (this.queryResultCache.size <= this.queryCacheConfig.maxEntries) {
      return;
    }

    const oldestEntry = this.queryResultCache.keys().next().value;
    if (oldestEntry) {
      this.queryResultCache.delete(oldestEntry);
    }
  }

  private refreshQueryCacheConfig(): void {
    const workspace = getWorkspace();
    if (!workspace) {
      return;
    }

    const config = workspace.getConfiguration("voltnuerongrid");
    const enabled = config.get<boolean>("query.cache.enabled", true);
    const ttlSeconds = config.get<number>("query.cache.ttlSeconds", 30);
    const maxEntries = config.get<number>("query.cache.maxEntries", 100);
    this.queryCacheConfig = {
      enabled,
      ttlMs: Math.max(1, ttlSeconds) * 1000,
      maxEntries: Math.max(1, maxEntries),
    };

    if (!enabled) {
      this.queryResultCache.clear();
    }
  }
}

function getWorkspace(): WorkspaceConfigApi | undefined {
  try {
    // Keep runtime vscode dependency optional so Node tests can run without extension host modules.
    const vscodeRuntime = require("vscode") as { workspace?: WorkspaceConfigApi };
    const workspace = vscodeRuntime.workspace;
    if (!workspace || typeof workspace.getConfiguration !== "function") {
      return undefined;
    }

    return workspace;
  } catch {
    return undefined;
  }
}

export function createQueryExecutionService(httpClient: HttpClient, context?: vscode.ExtensionContext): QueryExecutionService {
  return new QueryExecutionService(httpClient, context);
}
