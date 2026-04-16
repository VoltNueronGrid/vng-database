"use strict";
/**
 * QueryExecutionService: execute queries and manage results
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.QueryExecutionService = void 0;
exports.createQueryExecutionService = createQueryExecutionService;
const QueryResult_1 = require("../models/QueryResult");
const QueryHistory_1 = require("./QueryHistory");
class QueryExecutionService {
    constructor(httpClient, context) {
        this.context = context;
        this.queryHistory = new Map();
        this.queryResultCache = new Map();
        this.maxHistorySize = 100;
        this.historyStorageKey = "vng.queryHistory";
        this.activeExecutions = new Map();
        this.executionSequence = 0;
        this.queryCacheConfig = {
            enabled: true,
            ttlMs: 30 * 1000,
            maxEntries: 100,
        };
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
        const resultId = options?.executionId ?? this.createExecutionId("result", startTime);
        const timeoutMs = options?.timeoutMs ?? connection.settings.advanced.connectionTimeout ?? 30000;
        const controller = new AbortController();
        const cacheKey = this.createQueryCacheKey(connection.id, query);
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
            this.storeCachedResult(cacheKey, result);
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
    async executeStatementsStream(connection, sqlOrStatements, options) {
        const statements = Array.isArray(sqlOrStatements) ? sqlOrStatements : this.parseStatements(sqlOrStatements);
        const total = statements.length;
        const results = [];
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
        const entry = (0, QueryHistory_1.createQueryHistoryEntry)(connectionId, resultId, query, result);
        this.queryHistory.set(entry.id, entry);
        // Trim history to max size
        if (this.queryHistory.size > this.maxHistorySize) {
            const oldestEntryId = (0, QueryHistory_1.findOldestHistoryEntryId)(this.queryHistory.entries());
            if (oldestEntryId) {
                this.queryHistory.delete(oldestEntryId);
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
    dispose() {
        this.configDisposable?.dispose();
    }
    createExecutionId(prefix, timestamp = Date.now()) {
        this.executionSequence += 1;
        return `${prefix}-${timestamp}-${this.executionSequence}`;
    }
    async persistHistory() {
        if (!this.context) {
            return;
        }
        const snapshot = this.getHistory();
        await this.context.globalState.update(this.historyStorageKey, snapshot);
    }
    createQueryCacheKey(connectionId, query) {
        const normalized = query
            .trim()
            .replace(/\s+/g, " ")
            .replace(/\s*;\s*$/, ";");
        return `${connectionId}::${normalized}`;
    }
    getCachedResult(cacheKey, resultId, query, startTime) {
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
    storeCachedResult(cacheKey, result) {
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
    refreshQueryCacheConfig() {
        const workspace = getWorkspace();
        if (!workspace) {
            return;
        }
        const config = workspace.getConfiguration("voltnuerongrid");
        const enabled = config.get("query.cache.enabled", true);
        const ttlSeconds = config.get("query.cache.ttlSeconds", 30);
        const maxEntries = config.get("query.cache.maxEntries", 100);
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
exports.QueryExecutionService = QueryExecutionService;
function getWorkspace() {
    try {
        // Keep runtime vscode dependency optional so Node tests can run without extension host modules.
        const vscodeRuntime = require("vscode");
        const workspace = vscodeRuntime.workspace;
        if (!workspace || typeof workspace.getConfiguration !== "function") {
            return undefined;
        }
        return workspace;
    }
    catch {
        return undefined;
    }
}
function createQueryExecutionService(httpClient, context) {
    return new QueryExecutionService(httpClient, context);
}
//# sourceMappingURL=QueryExecutionService.js.map