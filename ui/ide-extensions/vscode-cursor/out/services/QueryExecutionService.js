"use strict";
/**
 * QueryExecutionService: execute queries and manage results
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.QueryExecutionService = void 0;
exports.createQueryExecutionService = createQueryExecutionService;
const QueryResult_1 = require("../models/QueryResult");
class QueryExecutionService {
    constructor(httpClient, context) {
        this.context = context;
        this.queryHistory = new Map();
        this.maxHistorySize = 100;
        this.historyStorageKey = "vng.queryHistory";
        this.activeExecutions = new Map();
        this.httpClient = httpClient;
    }
    async initialize() {
        if (!this.context) {
            return;
        }
        const stored = this.context.globalState.get(this.historyStorageKey, []);
        this.queryHistory.clear();
        for (const entry of stored.slice(0, this.maxHistorySize)) {
            this.queryHistory.set(entry.id, entry);
        }
    }
    /**
     * Execute a query
     */
    async executeQuery(connection, query, options) {
        const startTime = Date.now();
        const resultId = options?.executionId ?? `result-${startTime}`;
        const timeoutMs = options?.timeoutMs ?? connection.settings.advanced.connectionTimeout ?? 30000;
        const controller = new AbortController();
        const handleAbort = () => controller.abort();
        if (options?.signal) {
            if (options.signal.aborted) {
                controller.abort();
            }
            else {
                options.signal.addEventListener("abort", handleAbort, { once: true });
            }
        }
        this.activeExecutions.set(resultId, controller);
        try {
            // Validate query
            if (!query || query.trim().length === 0) {
                const invalidResult = {
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
            let result;
            if (response.status === 200) {
                result = (0, QueryResult_1.parseQueryResult)(query, response.data, executionTime);
                result.id = resultId;
            }
            else {
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
        }
        catch (error) {
            const executionTime = Date.now() - startTime;
            const errorMessage = error instanceof Error ? error.message : "Unknown error";
            const isCancelled = errorMessage.toLowerCase().includes("aborted") || errorMessage.toLowerCase().includes("abort");
            const isTimeout = errorMessage.toLowerCase().includes("timeout");
            const result = {
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
        }
        finally {
            this.activeExecutions.delete(resultId);
            if (options?.signal) {
                options.signal.removeEventListener("abort", handleAbort);
            }
        }
    }
    /**
     * Execute multiple queries
     */
    async executeMultiple(connection, queries, options) {
        const results = [];
        for (const query of queries) {
            const result = await this.executeQuery(connection, query, options);
            results.push(result);
        }
        return results;
    }
    async executeStatementsStream(connection, sqlOrStatements, options) {
        const statements = Array.isArray(sqlOrStatements) ? sqlOrStatements : this.parseStatements(sqlOrStatements);
        const total = statements.length;
        const results = [];
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
    parseStatements(sql) {
        return sql
            .split(";")
            .map((s) => s.trim())
            .filter((s) => s.length > 0)
            .map((s) => s + ";");
    }
    /**
     * Add query to history
     */
    async addToHistory(connectionId, resultId, query, result) {
        const entry = {
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
    getHistory(connectionId) {
        return Array.from(this.queryHistory.values())
            .filter((entry) => !connectionId || entry.connectionId === connectionId)
            .sort((a, b) => b.timestamp - a.timestamp);
    }
    /**
     * Get history entry by ID
     */
    getHistoryEntry(id) {
        return this.queryHistory.get(id) || null;
    }
    /**
     * Clear history
     */
    async clearHistory(connectionId) {
        if (!connectionId) {
            this.queryHistory.clear();
        }
        else {
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
    searchHistory(query, connectionId) {
        const lowerQuery = query.toLowerCase();
        return this.getHistory(connectionId).filter((entry) => entry.query.toLowerCase().includes(lowerQuery));
    }
    cancelExecution(executionId) {
        const controller = this.activeExecutions.get(executionId);
        if (!controller) {
            return false;
        }
        controller.abort();
        return true;
    }
    cancelAllExecutions() {
        const count = this.activeExecutions.size;
        for (const controller of this.activeExecutions.values()) {
            controller.abort();
        }
        return count;
    }
    getActiveExecutionIds() {
        return Array.from(this.activeExecutions.keys());
    }
    async persistHistory() {
        if (!this.context) {
            return;
        }
        const snapshot = this.getHistory();
        await this.context.globalState.update(this.historyStorageKey, snapshot);
    }
}
exports.QueryExecutionService = QueryExecutionService;
function createQueryExecutionService(httpClient, context) {
    return new QueryExecutionService(httpClient, context);
}
//# sourceMappingURL=QueryExecutionService.js.map