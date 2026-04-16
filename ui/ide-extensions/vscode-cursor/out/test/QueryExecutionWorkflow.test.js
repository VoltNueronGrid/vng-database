"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const Connection_1 = require("../models/Connection");
const QueryExecutionService_1 = require("../services/QueryExecutionService");
const QueryHistory_1 = require("../services/QueryHistory");
const QueryHistoryTree_1 = require("../providers/QueryHistoryTree");
const QueryResultsState_1 = require("../ui/QueryResultsState");
(0, node_test_1.default)("executeStatementsStream drives query results state and connection-scoped history", async () => {
    const connection = {
        id: "conn-workflow",
        settings: (0, Connection_1.createDefaultConnection)({
            id: "conn-workflow",
            name: "Workflow Connection",
            advanced: { connectionTimeout: 5000 },
        }),
        isActive: true,
        isConnected: true,
    };
    const httpClient = {
        async executeQuery(_connection, query, options) {
            if (query.includes("select 1")) {
                return {
                    status: 200,
                    data: [{ value: 1 }],
                    headers: {},
                };
            }
            return {
                status: 503,
                data: { detail: "statement failed" },
                error: `request ${options?.requestId} timeout`,
                headers: {},
            };
        },
    };
    const service = new QueryExecutionService_1.QueryExecutionService(httpClient);
    const publishedStates = [];
    const results = await service.executeStatementsStream(connection, ["select 1;", "select broken;"], {
        executionId: "flow",
        stopOnError: true,
        onResult: (result, index, total) => {
            publishedStates.push((0, QueryResultsState_1.createQueryResultsState)(result, `Workflow (${index}/${total})`, connection.settings.name));
        },
    });
    strict_1.default.equal(results.length, 2);
    strict_1.default.equal(results[0].status, "success");
    strict_1.default.equal(results[1].status, "error");
    strict_1.default.equal(results[1].error?.code, "TIMEOUT");
    strict_1.default.equal(publishedStates.length, 2);
    strict_1.default.equal(publishedStates[0].connectionName, "Workflow Connection");
    strict_1.default.equal(publishedStates[1].operation, "Workflow (2/2)");
    const history = service.getHistory(connection.id);
    strict_1.default.equal(history.length, 2);
    strict_1.default.equal(history[0].connectionId, connection.id);
    strict_1.default.equal(history[1].connectionId, connection.id);
    const historyItems = (0, QueryHistoryTree_1.buildQueryHistoryItems)(history);
    strict_1.default.equal(historyItems.length, 2);
    strict_1.default.equal(historyItems[0].type, "entry");
    strict_1.default.match(historyItems[0].label, /select/);
    const presentation = (0, QueryHistoryTree_1.describeQueryHistoryEntry)(history[0], () => "10:00:00", () => "2026-04-16 10:00:00");
    strict_1.default.match(presentation.description, /ms/);
    strict_1.default.match(presentation.tooltip, /Status:/);
});
(0, node_test_1.default)("query execution history keeps cancelled state and supports search plus clear by connection", async () => {
    const connectionA = {
        id: "conn-a",
        settings: (0, Connection_1.createDefaultConnection)({ id: "conn-a", name: "Conn A" }),
        isActive: true,
        isConnected: true,
    };
    const connectionB = {
        id: "conn-b",
        settings: (0, Connection_1.createDefaultConnection)({ id: "conn-b", name: "Conn B" }),
        isActive: false,
        isConnected: true,
    };
    const httpClient = {
        async executeQuery(_connection, query) {
            if (query.includes("cancel")) {
                return {
                    status: 0,
                    error: "request aborted by user",
                    headers: {},
                };
            }
            return {
                status: 200,
                data: [{ ok: true }],
                headers: {},
            };
        },
    };
    const service = new QueryExecutionService_1.QueryExecutionService(httpClient);
    const cancelled = await service.executeQuery(connectionA, "select cancel;");
    const succeeded = await service.executeQuery(connectionB, "select ok;");
    strict_1.default.equal(cancelled.status, "cancelled");
    strict_1.default.equal(succeeded.status, "success");
    const historyA = service.getHistory(connectionA.id);
    strict_1.default.equal(historyA.length, 1);
    strict_1.default.equal(historyA[0].status, "cancelled");
    const searchA = service.searchHistory("cancel", connectionA.id);
    strict_1.default.equal(searchA.length, 1);
    strict_1.default.equal(searchA[0].connectionId, connectionA.id);
    await service.clearHistory(connectionA.id);
    strict_1.default.equal(service.getHistory(connectionA.id).length, 0);
    strict_1.default.equal(service.getHistory(connectionB.id).length, 1);
});
(0, node_test_1.default)("query execution service initializes from persisted history, parses statements, and clears global history", async () => {
    const updates = [];
    const persistedEntry = (0, QueryHistory_1.createQueryHistoryEntry)("conn-persisted", "result-persisted", "select 1;", {
        id: "result-persisted",
        query: "select 1;",
        status: "success",
        rows: [{ value: 1 }],
        columns: [{ name: "value", type: "number", index: 0 }],
        rowCount: 1,
        executionTime: 3,
        timestamp: 100,
    }, 100);
    const context = {
        globalState: {
            get(key, defaultValue) {
                if (key === "vng.queryHistory") {
                    return [persistedEntry];
                }
                return defaultValue;
            },
            async update(key, value) {
                updates.push({ key, value });
            },
        },
    };
    const httpClient = {
        async executeQuery() {
            return {
                status: 200,
                data: [{ ok: true }],
                headers: {},
            };
        },
    };
    const service = new QueryExecutionService_1.QueryExecutionService(httpClient, context);
    await service.initialize();
    strict_1.default.equal(service.getHistory("conn-persisted").length, 1);
    strict_1.default.deepEqual(service.parseStatements("select 1;\n\nselect 2;  "), ["select 1;", "select 2;"]);
    const results = await service.executeMultiple({
        id: "conn-persisted",
        settings: (0, Connection_1.createDefaultConnection)({ id: "conn-persisted", name: "Persisted" }),
        isActive: true,
        isConnected: true,
    }, ["select 1;", "select 2;"], { executionId: "multi" });
    strict_1.default.equal(results.length, 2);
    strict_1.default.equal(service.getHistory("conn-persisted").length, 3);
    await service.clearHistory();
    strict_1.default.equal(service.getHistory().length, 0);
    strict_1.default.equal(updates.at(-1)?.key, "vng.queryHistory");
    strict_1.default.deepEqual(updates.at(-1)?.value, []);
});
(0, node_test_1.default)("query execution service tracks active executions and supports cancellation helpers", async () => {
    const connection = {
        id: "conn-cancel",
        settings: (0, Connection_1.createDefaultConnection)({ id: "conn-cancel", name: "Cancel" }),
        isActive: true,
        isConnected: true,
    };
    const httpClient = {
        executeQuery(_connection, _query, options) {
            return new Promise((_resolve, reject) => {
                if (options?.signal) {
                    options.signal.addEventListener("abort", () => reject(new Error("aborted by caller")), { once: true });
                }
            });
        },
    };
    const service = new QueryExecutionService_1.QueryExecutionService(httpClient);
    const executionPromise = service.executeQuery(connection, "select wait;", { executionId: "exec-1" });
    strict_1.default.deepEqual(service.getActiveExecutionIds(), ["exec-1"]);
    strict_1.default.equal(service.cancelExecution("missing"), false);
    strict_1.default.equal(service.cancelExecution("exec-1"), true);
    const cancelledResult = await executionPromise;
    strict_1.default.equal(cancelledResult.status, "cancelled");
    strict_1.default.deepEqual(service.getActiveExecutionIds(), []);
    const firstPending = service.executeQuery(connection, "select wait 1;", { executionId: "exec-2" });
    const secondPending = service.executeQuery(connection, "select wait 2;", { executionId: "exec-3" });
    strict_1.default.equal(service.cancelAllExecutions(), 2);
    const cancelled = await Promise.all([firstPending, secondPending]);
    strict_1.default.equal(cancelled[0].status, "cancelled");
    strict_1.default.equal(cancelled[1].status, "cancelled");
});
(0, node_test_1.default)("query history helpers preserve status mapping and oldest-entry lookup", () => {
    strict_1.default.equal((0, QueryHistory_1.toQueryHistoryStatus)("success"), "success");
    strict_1.default.equal((0, QueryHistory_1.toQueryHistoryStatus)("cancelled"), "cancelled");
    strict_1.default.equal((0, QueryHistory_1.toQueryHistoryStatus)("error"), "error");
    const oldestEntryId = (0, QueryHistory_1.findOldestHistoryEntryId)([
        [
            "newer",
            (0, QueryHistory_1.createQueryHistoryEntry)("conn-1", "result-newer", "select newer;", {
                id: "result-newer",
                query: "select newer;",
                status: "success",
                rows: [],
                columns: [],
                rowCount: 0,
                executionTime: 1,
                timestamp: 200,
            }, 200),
        ],
        [
            "older",
            (0, QueryHistory_1.createQueryHistoryEntry)("conn-1", "result-older", "select older;", {
                id: "result-older",
                query: "select older;",
                status: "error",
                rows: [],
                columns: [],
                rowCount: 0,
                executionTime: 1,
                timestamp: 100,
                error: { message: "failed" },
            }, 100),
        ],
    ]);
    strict_1.default.equal(oldestEntryId, "older");
});
(0, node_test_1.default)("query result state helpers produce empty and populated snapshots", () => {
    const emptyState = (0, QueryResultsState_1.createDefaultQueryResultsState)("Dev Connection", 123);
    strict_1.default.equal(emptyState.connectionName, "Dev Connection");
    strict_1.default.equal(emptyState.result.id, "empty");
    strict_1.default.equal(emptyState.result.timestamp, 123);
    const populatedState = (0, QueryResultsState_1.createQueryResultsState)({
        id: "result-1",
        query: "select 1;",
        status: "success",
        rows: [{ value: 1 }],
        columns: [{ name: "value", type: "number", index: 0 }],
        rowCount: 1,
        executionTime: 5,
        timestamp: 456,
    }, "History Re-run", "Dev Connection");
    strict_1.default.equal(populatedState.operation, "History Re-run");
    strict_1.default.equal(populatedState.result.rowCount, 1);
});
(0, node_test_1.default)("query execution service reuses cached successful query results within TTL", async () => {
    const connection = {
        id: "conn-cache",
        settings: (0, Connection_1.createDefaultConnection)({ id: "conn-cache", name: "Cache" }),
        isActive: true,
        isConnected: true,
    };
    let executeCount = 0;
    const context = {
        globalState: {
            get(_key, defaultValue) {
                return defaultValue;
            },
            async update() {
                return;
            },
        },
    };
    const httpClient = {
        async executeQuery() {
            executeCount += 1;
            return {
                status: 200,
                data: [{ cached: true }],
                headers: {},
            };
        },
    };
    const service = new QueryExecutionService_1.QueryExecutionService(httpClient, context);
    const first = await service.executeQuery(connection, "select * from cache_test;");
    const second = await service.executeQuery(connection, " select   *  from   cache_test ; ");
    strict_1.default.equal(first.status, "success");
    strict_1.default.equal(second.status, "success");
    strict_1.default.equal(executeCount, 1);
    strict_1.default.notEqual(first.id, second.id);
    strict_1.default.equal(service.getHistory(connection.id).length, 2);
});
//# sourceMappingURL=QueryExecutionWorkflow.test.js.map