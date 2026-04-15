/**
 * QueryExecutionService: execute queries and manage results
 */

import * as vscode from "vscode";
import { Connection } from "../models/Connection";
import { QueryResult, QueryHistoryEntry, parseQueryResult } from "../models/QueryResult";
import { HttpClient } from "./HttpClient";

export interface QueryExecutionOptions {
  timeoutMs?: number;
  executionId?: string;
  signal?: AbortSignal;
}

export interface QueryStreamOptions extends QueryExecutionOptions {
  onResult?: (result: QueryResult, index: number, total: number) => Promise<void> | void;
  stopOnError?: boolean;
}

export class QueryExecutionService {
  private httpClient: HttpClient;
  private queryHistory: Map<string, QueryHistoryEntry> = new Map();
  private maxHistorySize = 100;
  private readonly historyStorageKey = "vng.queryHistory";
  private readonly activeExecutions: Map<string, AbortController> = new Map();

  constructor(httpClient: HttpClient, private readonly context?: vscode.ExtensionContext) {
    this.httpClient = httpClient;
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
    const resultId = options?.executionId ?? `result-${startTime}`;
    const timeoutMs = options?.timeoutMs ?? connection.settings.advanced.connectionTimeout ?? 30000;
    const controller = new AbortController();

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
    for (const query of queries) {
      const result = await this.executeQuery(connection, query, options);
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
    const streamId = options?.executionId ?? `stream-${Date.now()}`;

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
    const entry: QueryHistoryEntry = {
      id: `hist-${Date.now()}`,
      query,
      connectionId,
      timestamp: Date.now(),
      executionTime: result.executionTime,
      status: result.status === "success" ? "success" : "error",
      resultId,
    };

    this.queryHistory.set(entry.id, entry);

    // Trim history to max size
    if (this.queryHistory.size > this.maxHistorySize) {
      const oldest = Array.from(this.queryHistory.entries())
        .sort((a, b) => a[1].timestamp - b[1].timestamp)[0];
      if (oldest) {
        this.queryHistory.delete(oldest[0]);
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

  private async persistHistory(): Promise<void> {
    if (!this.context) {
      return;
    }

    const snapshot = this.getHistory();
    await this.context.globalState.update(this.historyStorageKey, snapshot);
  }
}

export function createQueryExecutionService(httpClient: HttpClient, context?: vscode.ExtensionContext): QueryExecutionService {
  return new QueryExecutionService(httpClient, context);
}
