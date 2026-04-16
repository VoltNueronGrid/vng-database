import test from "node:test";
import assert from "node:assert/strict";
import { createDefaultConnection } from "../models/Connection";
import { QueryExecutionService } from "../services/QueryExecutionService";
import { createQueryHistoryEntry, findOldestHistoryEntryId, toQueryHistoryStatus } from "../services/QueryHistory";
import { buildQueryHistoryItems, describeQueryHistoryEntry } from "../providers/QueryHistoryTree";
import { QueryResultsState, createDefaultQueryResultsState, createQueryResultsState } from "../ui/QueryResultsState";

test("executeStatementsStream drives query results state and connection-scoped history", async () => {
  const connection = {
    id: "conn-workflow",
    settings: createDefaultConnection({
      id: "conn-workflow",
      name: "Workflow Connection",
      advanced: { connectionTimeout: 5000 },
    }),
    isActive: true,
    isConnected: true,
  };

  const httpClient = {
    async executeQuery(_connection: unknown, query: string, options?: { requestId?: string }) {
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

  const service = new QueryExecutionService(httpClient as never);
  const publishedStates: QueryResultsState[] = [];

  const results = await service.executeStatementsStream(connection, ["select 1;", "select broken;"], {
    executionId: "flow",
    stopOnError: true,
    onResult: (result, index, total) => {
      publishedStates.push(createQueryResultsState(result, `Workflow (${index}/${total})`, connection.settings.name));
    },
  });

  assert.equal(results.length, 2);
  assert.equal(results[0].status, "success");
  assert.equal(results[1].status, "error");
  assert.equal(results[1].error?.code, "TIMEOUT");
  assert.equal(publishedStates.length, 2);
  assert.equal(publishedStates[0].connectionName, "Workflow Connection");
  assert.equal(publishedStates[1].operation, "Workflow (2/2)");

  const history = service.getHistory(connection.id);
  assert.equal(history.length, 2);
  assert.equal(history[0].connectionId, connection.id);
  assert.equal(history[1].connectionId, connection.id);

  const historyItems = buildQueryHistoryItems(history);
  assert.equal(historyItems.length, 2);
  assert.equal(historyItems[0].type, "entry");
  assert.match(historyItems[0].label, /select/);

  const presentation = describeQueryHistoryEntry(history[0], () => "10:00:00", () => "2026-04-16 10:00:00");
  assert.match(presentation.description, /ms/);
  assert.match(presentation.tooltip, /Status:/);
});

test("query execution history keeps cancelled state and supports search plus clear by connection", async () => {
  const connectionA = {
    id: "conn-a",
    settings: createDefaultConnection({ id: "conn-a", name: "Conn A" }),
    isActive: true,
    isConnected: true,
  };
  const connectionB = {
    id: "conn-b",
    settings: createDefaultConnection({ id: "conn-b", name: "Conn B" }),
    isActive: false,
    isConnected: true,
  };

  const httpClient = {
    async executeQuery(_connection: { id: string }, query: string) {
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

  const service = new QueryExecutionService(httpClient as never);

  const cancelled = await service.executeQuery(connectionA, "select cancel;");
  const succeeded = await service.executeQuery(connectionB, "select ok;");

  assert.equal(cancelled.status, "cancelled");
  assert.equal(succeeded.status, "success");

  const historyA = service.getHistory(connectionA.id);
  assert.equal(historyA.length, 1);
  assert.equal(historyA[0].status, "cancelled");

  const searchA = service.searchHistory("cancel", connectionA.id);
  assert.equal(searchA.length, 1);
  assert.equal(searchA[0].connectionId, connectionA.id);

  await service.clearHistory(connectionA.id);
  assert.equal(service.getHistory(connectionA.id).length, 0);
  assert.equal(service.getHistory(connectionB.id).length, 1);
});

test("query execution service initializes from persisted history, parses statements, and clears global history", async () => {
  const updates: Array<{ key: string; value: unknown }> = [];
  const persistedEntry = createQueryHistoryEntry(
    "conn-persisted",
    "result-persisted",
    "select 1;",
    {
      id: "result-persisted",
      query: "select 1;",
      status: "success",
      rows: [{ value: 1 }],
      columns: [{ name: "value", type: "number", index: 0 }],
      rowCount: 1,
      executionTime: 3,
      timestamp: 100,
    },
    100
  );

  const context = {
    globalState: {
      get<T>(key: string, defaultValue: T): T {
        if (key === "vng.queryHistory") {
          return [persistedEntry] as T;
        }

        return defaultValue;
      },
      async update(key: string, value: unknown) {
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

  const service = new QueryExecutionService(httpClient as never, context as never);
  await service.initialize();

  assert.equal(service.getHistory("conn-persisted").length, 1);
  assert.deepEqual(service.parseStatements("select 1;\n\nselect 2;  "), ["select 1;", "select 2;"]);

  const results = await service.executeMultiple(
    {
      id: "conn-persisted",
      settings: createDefaultConnection({ id: "conn-persisted", name: "Persisted" }),
      isActive: true,
      isConnected: true,
    },
    ["select 1;", "select 2;"],
    { executionId: "multi" }
  );

  assert.equal(results.length, 2);
  assert.equal(service.getHistory("conn-persisted").length, 3);

  await service.clearHistory();
  assert.equal(service.getHistory().length, 0);
  assert.equal(updates.at(-1)?.key, "vng.queryHistory");
  assert.deepEqual(updates.at(-1)?.value, []);
});

test("query execution service tracks active executions and supports cancellation helpers", async () => {
  const connection = {
    id: "conn-cancel",
    settings: createDefaultConnection({ id: "conn-cancel", name: "Cancel" }),
    isActive: true,
    isConnected: true,
  };

  const httpClient = {
    executeQuery(_connection: unknown, _query: string, options?: { signal?: AbortSignal }) {
      return new Promise((_resolve, reject) => {
        if (options?.signal) {
          options.signal.addEventListener(
            "abort",
            () => reject(new Error("aborted by caller")),
            { once: true }
          );
        }
      });
    },
  };

  const service = new QueryExecutionService(httpClient as never);
  const executionPromise = service.executeQuery(connection, "select wait;", { executionId: "exec-1" });

  assert.deepEqual(service.getActiveExecutionIds(), ["exec-1"]);
  assert.equal(service.cancelExecution("missing"), false);
  assert.equal(service.cancelExecution("exec-1"), true);

  const cancelledResult = await executionPromise;
  assert.equal(cancelledResult.status, "cancelled");
  assert.deepEqual(service.getActiveExecutionIds(), []);

  const firstPending = service.executeQuery(connection, "select wait 1;", { executionId: "exec-2" });
  const secondPending = service.executeQuery(connection, "select wait 2;", { executionId: "exec-3" });
  assert.equal(service.cancelAllExecutions(), 2);

  const cancelled = await Promise.all([firstPending, secondPending]);
  assert.equal(cancelled[0].status, "cancelled");
  assert.equal(cancelled[1].status, "cancelled");
});

test("query history helpers preserve status mapping and oldest-entry lookup", () => {
  assert.equal(toQueryHistoryStatus("success"), "success");
  assert.equal(toQueryHistoryStatus("cancelled"), "cancelled");
  assert.equal(toQueryHistoryStatus("error"), "error");

  const oldestEntryId = findOldestHistoryEntryId([
    [
      "newer",
      createQueryHistoryEntry(
        "conn-1",
        "result-newer",
        "select newer;",
        {
          id: "result-newer",
          query: "select newer;",
          status: "success",
          rows: [],
          columns: [],
          rowCount: 0,
          executionTime: 1,
          timestamp: 200,
        },
        200
      ),
    ],
    [
      "older",
      createQueryHistoryEntry(
        "conn-1",
        "result-older",
        "select older;",
        {
          id: "result-older",
          query: "select older;",
          status: "error",
          rows: [],
          columns: [],
          rowCount: 0,
          executionTime: 1,
          timestamp: 100,
          error: { message: "failed" },
        },
        100
      ),
    ],
  ]);

  assert.equal(oldestEntryId, "older");
});

test("query result state helpers produce empty and populated snapshots", () => {
  const emptyState = createDefaultQueryResultsState("Dev Connection", 123);
  assert.equal(emptyState.connectionName, "Dev Connection");
  assert.equal(emptyState.result.id, "empty");
  assert.equal(emptyState.result.timestamp, 123);

  const populatedState = createQueryResultsState(
    {
      id: "result-1",
      query: "select 1;",
      status: "success",
      rows: [{ value: 1 }],
      columns: [{ name: "value", type: "number", index: 0 }],
      rowCount: 1,
      executionTime: 5,
      timestamp: 456,
    },
    "History Re-run",
    "Dev Connection"
  );

  assert.equal(populatedState.operation, "History Re-run");
  assert.equal(populatedState.result.rowCount, 1);
});

test("query execution service reuses cached successful query results within TTL", async () => {
  const connection = {
    id: "conn-cache",
    settings: createDefaultConnection({ id: "conn-cache", name: "Cache" }),
    isActive: true,
    isConnected: true,
  };

  let executeCount = 0;
  const context = {
    globalState: {
      get<T>(_key: string, defaultValue: T): T {
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

  const service = new QueryExecutionService(httpClient as never, context as never);

  const first = await service.executeQuery(connection, "select * from cache_test;");
  const second = await service.executeQuery(connection, " select   *  from   cache_test ; ");

  assert.equal(first.status, "success");
  assert.equal(second.status, "success");
  assert.equal(executeCount, 1);
  assert.notEqual(first.id, second.id);
  assert.equal(service.getHistory(connection.id).length, 2);
});