import * as vscode from "vscode";
import { promises as fs } from "fs";
import { readConnection, runConnectionWizard } from "./config";
import { analyzeSql, executeSql, getSchemaRegistry, runConnectivityChecks, toPermissionMessage } from "./client";
import { RuntimeConnection } from "./config";
import { VngActionsProvider } from "./activityView";
import {
  Connection,
  ConnectionSettings as ManagedConnectionSettings,
  createDefaultConnection,
  validateConnectionSettings,
} from "./models";
import {
  ConnectionManager,
  HttpClient,
  QueryExecutionService,
  QueryStreamOptions,
  SchemaManager,
  createConnectionManager,
  createHttpClient,
  createQueryExecutionService,
  createSchemaManager,
} from "./services";
import {
  DatabaseExplorerProvider,
  QueryHistoryProvider,
  createDatabaseExplorerProvider,
  createQueryHistoryProvider,
} from "./providers";
import {
  handleCopyName,
  handleShowDDL,
  handleSQLTemplate,
  handleGenerateMockData,
  handleDumpStruct,
  handleDropTable,
} from "./commands";
import { SchemaTreeItem } from "./providers/DatabaseExplorerProvider";
import { registerSqlEditorFeatures } from "./sql";
import {
  ConnectionManagerMessage,
  ConnectionManagerState,
  WebviewConnectionDraft,
  createConnectionManagerPanel,
} from "./ui/ConnectionManagerWebview";
import { QueryResult, exportAsCSV, exportAsJSON } from "./models";
import { QueryResultsMessage, QueryResultsState, createQueryResultsPanel } from "./ui/QueryResultsWebview";

// Global service instances
let connectionManager: ConnectionManager;
let httpClient: HttpClient;
let queryExecutionService: QueryExecutionService;
let schemaManager: SchemaManager;
let databaseExplorerProvider: DatabaseExplorerProvider;
let queryHistoryProvider: QueryHistoryProvider;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const output = vscode.window.createOutputChannel("VoltNueronGrid");
  const connectionStatusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  connectionStatusBar.command = "vng.quickSwitchConnection";

  const updateConnectionStatusBar = (): void => {
    const active = connectionManager.getActiveConnection();
    if (!active) {
      connectionStatusBar.hide();
      return;
    }

    const healthIcon = active.isConnected ? "$(pass-filled)" : "$(circle-large-outline)";
    connectionStatusBar.text = `${healthIcon} $(database) ${active.settings.name}`;
    connectionStatusBar.tooltip = [
      `Active connection: ${active.settings.name}`,
      `Mode: ${active.settings.mode}`,
      `Base URL: ${active.settings.baseUrl}`,
      `Health: ${active.isConnected ? "Connected" : "Not verified"}`,
      "Click to switch connections.",
    ].join("\n");
    connectionStatusBar.show();
  };

  // Initialize services
  connectionManager = createConnectionManager(context);
  await connectionManager.initialize();

  httpClient = createHttpClient();
  queryExecutionService = createQueryExecutionService(httpClient, context);
  await queryExecutionService.initialize();
  schemaManager = createSchemaManager(httpClient);

  // Initialize database explorer
  databaseExplorerProvider = createDatabaseExplorerProvider(schemaManager);
  queryHistoryProvider = createQueryHistoryProvider(queryExecutionService);

  output.appendLine("[VoltNueronGrid] Extension activated (v0.2.0)");

  let latestQueryResult: QueryResult | undefined;
  let latestQueryResultState: QueryResultsState | undefined;
  let queryResultsPanel: ReturnType<typeof createQueryResultsPanel> | undefined;

  const buildDefaultQueryResultState = (): QueryResultsState => ({
    operation: "Query Results",
    connectionName: connectionManager.getActiveConnection()?.settings.name ?? "No active connection",
    result: {
      id: "empty",
      query: "",
      status: "success",
      rows: [],
      columns: [],
      rowCount: 0,
      executionTime: 0,
      timestamp: Date.now(),
    },
  });

  const exportLatestQueryResult = async (format: "csv" | "json") => {
    if (!latestQueryResult) {
      vscode.window.showWarningMessage("No query result available to export.");
      return;
    }

    const defaultExtension = format === "csv" ? "csv" : "json";
    const uri = await vscode.window.showSaveDialog({
      saveLabel: `Export ${format.toUpperCase()}`,
      defaultUri: vscode.Uri.file(`vng-query-result-${Date.now()}.${defaultExtension}`),
      filters:
        format === "csv"
          ? { CSV: ["csv"], "All Files": ["*"] }
          : { JSON: ["json"], "All Files": ["*"] },
    });

    if (!uri) {
      return;
    }

    const payload = format === "csv" ? exportAsCSV(latestQueryResult) : exportAsJSON(latestQueryResult);
    await fs.writeFile(uri.fsPath, payload, "utf8");
    vscode.window.showInformationMessage(`Exported query result to ${uri.fsPath}.`);
  };

  const ensureQueryResultsPanel = (): ReturnType<typeof createQueryResultsPanel> => {
    if (queryResultsPanel) {
      return queryResultsPanel;
    }

    const initialState = latestQueryResultState ?? buildDefaultQueryResultState();
    queryResultsPanel = createQueryResultsPanel(context, initialState, async (message: QueryResultsMessage) => {
      if (message.type === "requestExport") {
        await exportLatestQueryResult(message.format);
      }
    });

    queryResultsPanel.panel.onDidDispose(() => {
      queryResultsPanel = undefined;
    });

    context.subscriptions.push(queryResultsPanel.panel);
    return queryResultsPanel;
  };

  const publishQueryResult = async (result: QueryResult, operation: string, connectionName: string) => {
    latestQueryResult = result;
    latestQueryResultState = {
      operation,
      connectionName,
      result,
    };

    const panel = ensureQueryResultsPanel();
    await panel.updateState(latestQueryResultState);
    panel.reveal();
  };

  // Create tree views
  const actionsProvider = new VngActionsProvider();
  const actionsView = vscode.window.createTreeView("vngActions", {
    treeDataProvider: actionsProvider,
    showCollapseAll: false,
  });

  const databaseView = vscode.window.createTreeView("vngDatabaseExplorer", {
    treeDataProvider: databaseExplorerProvider,
    showCollapseAll: true,
  });

  const queryHistoryView = vscode.window.createTreeView("vngQueryHistory", {
    treeDataProvider: queryHistoryProvider,
    showCollapseAll: false,
  });

  // Update database explorer when active connection changes
  const activeConnection = connectionManager.getActiveConnection();
  if (activeConnection) {
    databaseExplorerProvider.setConnection(activeConnection);
    queryHistoryProvider.setActiveConnection(activeConnection.id);
  } else {
    // Backward-compatible migration from legacy single-connection settings.
    const legacyConnection = await readConnection(context);
    if (legacyConnection) {
      const migrated = await upsertManagedConnection(legacyConnection);
      databaseExplorerProvider.setConnection(migrated);
      queryHistoryProvider.setActiveConnection(migrated.id);
    }
  }
  updateConnectionStatusBar();

  const connect = vscode.commands.registerCommand("vng.connectWizard", async () => {
    const connection = await runConnectionWizard(context);
    if (!connection) {
      vscode.window.showInformationMessage("VoltNueronGrid connection wizard canceled.");
      return;
    }

    const managed = await upsertManagedConnection(connection);
    databaseExplorerProvider.setConnection(managed);
    updateConnectionStatusBar();

    vscode.window.showInformationMessage(`Saved VoltNueronGrid connection for ${connection.settings.mode} mode.`);
  });

  const quickSwitchConnection = vscode.commands.registerCommand("vng.quickSwitchConnection", async () => {
    const connections = connectionManager.listConnections();
    if (connections.length === 0) {
      vscode.window.showWarningMessage("No VoltNueronGrid connections configured.");
      return;
    }

    const pick = await vscode.window.showQuickPick(
      connections.map((connection) => ({
        label: connection.settings.name,
        description: `${connection.settings.mode}${connection.isActive ? " • active" : ""}`,
        detail: `${connection.settings.baseUrl}${connection.isConnected ? " • connected" : " • not verified"}`,
        connectionId: connection.id,
      })),
      {
        title: "Switch VoltNueronGrid Connection",
        placeHolder: "Select an active connection",
      }
    );

    if (!pick) {
      return;
    }

    const active = await connectionManager.setActiveConnection(pick.connectionId);
    databaseExplorerProvider.setConnection(active);
    queryHistoryProvider.setActiveConnection(active?.id);
    updateConnectionStatusBar();
    if (active) {
      vscode.window.showInformationMessage(`Active connection set to '${active.settings.name}'.`);
    }
  });

  const manageConnections = vscode.commands.registerCommand("vng.manageConnections", async () => {
    let editor: ConnectionManagerState["editor"] | undefined;

    const defaultDraft = (): WebviewConnectionDraft => ({
      name: "",
      baseUrl: "http://127.0.0.1:8080",
      mode: "admin",
      runtimeTarget: "custom",
      adminKey: "",
      operatorId: "",
      tenantId: "",
      userId: "",
      ssl: {
        enabled: false,
        rejectUnauthorized: true,
      },
      advanced: {
        connectionTimeout: 5000,
        idleTimeout: 300000,
        keepAlive: true,
        maxConnections: 10,
      },
    });

    const draftFromConnection = (connection: Connection): WebviewConnectionDraft => ({
      id: connection.id,
      name: connection.settings.name,
      baseUrl: connection.settings.baseUrl,
      mode: connection.settings.mode,
      runtimeTarget: connection.settings.runtimeTarget,
      adminKey: "",
      operatorId: connection.settings.operatorId ?? "",
      tenantId: connection.settings.tenantId ?? "",
      userId: connection.settings.userId ?? "",
      ssl: {
        enabled: connection.settings.ssl.enabled,
        caPath: connection.settings.ssl.caPath,
        certPath: connection.settings.ssl.certPath,
        keyPath: connection.settings.ssl.keyPath,
        rejectUnauthorized: connection.settings.ssl.rejectUnauthorized ?? true,
      },
      advanced: {
        connectionTimeout: connection.settings.advanced.connectionTimeout ?? 5000,
        idleTimeout: connection.settings.advanced.idleTimeout ?? 300000,
        keepAlive: connection.settings.advanced.keepAlive ?? true,
        maxConnections: connection.settings.advanced.maxConnections ?? 10,
      },
    });

    const buildWebviewState = (): ConnectionManagerState => ({
      connections: connectionManager.listConnections().map((connection) => ({
        id: connection.id,
        name: connection.settings.name,
        mode: connection.settings.mode,
        baseUrl: connection.settings.baseUrl,
        active: connection.isActive,
        connected: connection.isConnected,
      })),
      editor,
    });

    const panel = createConnectionManagerPanel(context, buildWebviewState(), async (message: ConnectionManagerMessage) => {
      if (message.type === "refresh") {
        await panel.updateState(buildWebviewState());
        return;
      }

      if (message.type === "openCreate") {
        editor = {
          mode: "create",
          draft: defaultDraft(),
        };
        await panel.updateState(buildWebviewState());
        return;
      }

      if (message.type === "cancelEdit") {
        editor = undefined;
        await panel.updateState(buildWebviewState());
        return;
      }

      if (message.type === "openEdit") {
        const selected = connectionManager.getConnection(message.id);
        if (!selected) {
          await panel.updateState(buildWebviewState());
          return;
        }
        editor = {
          mode: "edit",
          draft: draftFromConnection(selected),
        };
        await panel.updateState(buildWebviewState());
        return;
      }

      if (message.type === "save") {
        const draft = message.draft;
        if (!draft.name.trim()) {
          vscode.window.showWarningMessage("Connection name is required.");
          await panel.updateState(buildWebviewState());
          return;
        }

        let parsedUrl: URL;
        try {
          parsedUrl = new URL(draft.baseUrl);
          if (parsedUrl.protocol !== "http:" && parsedUrl.protocol !== "https:") {
            throw new Error("Only http/https URLs are supported.");
          }
        } catch {
          vscode.window.showWarningMessage("Enter a valid base URL (http/https).");
          await panel.updateState(buildWebviewState());
          return;
        }

        if (draft.mode === "operator" && (!(draft.operatorId ?? "").trim() || !draft.adminKey?.trim())) {
          vscode.window.showWarningMessage("Operator mode requires Operator ID and Admin Key.");
          await panel.updateState(buildWebviewState());
          return;
        }

        if (draft.mode === "tenant" && (!(draft.tenantId ?? "").trim() || !(draft.userId ?? "").trim())) {
          vscode.window.showWarningMessage("Tenant mode requires Tenant ID and User ID.");
          await panel.updateState(buildWebviewState());
          return;
        }

        const connectionPatch: Partial<ManagedConnectionSettings> = {
          name: draft.name.trim(),
          baseUrl: draft.baseUrl.replace(/\/$/, ""),
          host: parsedUrl.hostname,
          port: parsedUrl.port ? Number(parsedUrl.port) : parsedUrl.protocol === "https:" ? 443 : 80,
          mode: draft.mode,
          runtimeTarget: draft.runtimeTarget,
          operatorId: draft.mode === "operator" ? (draft.operatorId ?? "").trim() : undefined,
          tenantId: draft.mode === "tenant" ? (draft.tenantId ?? "").trim() : undefined,
          userId: draft.mode === "tenant" ? (draft.userId ?? "").trim() : undefined,
          ssl: {
            enabled: draft.ssl.enabled,
            caPath: draft.ssl.caPath?.trim() || undefined,
            certPath: draft.ssl.certPath?.trim() || undefined,
            keyPath: draft.ssl.keyPath?.trim() || undefined,
            rejectUnauthorized: draft.ssl.rejectUnauthorized ?? true,
          },
          advanced: {
            connectionTimeout: draft.advanced.connectionTimeout ?? 5000,
            idleTimeout: draft.advanced.idleTimeout ?? 300000,
            keepAlive: draft.advanced.keepAlive ?? true,
            maxConnections: draft.advanced.maxConnections ?? 10,
          },
          lastUsed: Date.now(),
        };
        if (draft.adminKey?.trim()) {
          connectionPatch.adminKey = draft.adminKey.trim();
        }

        const validationError = validateConnectionSettings(connectionPatch);
        if (validationError) {
          vscode.window.showWarningMessage(validationError);
          await panel.updateState(buildWebviewState());
          return;
        }

        if (editor?.mode === "edit" && draft.id) {
          await connectionManager.updateConnection(draft.id, connectionPatch);
          vscode.window.showInformationMessage("Connection updated.");
        } else {
          const created = await connectionManager.addConnection(
            createDefaultConnection({
              id: `conn-${Date.now()}`,
              ...connectionPatch,
              serverType: "voltnuerongrid",
            })
          );
          await connectionManager.setActiveConnection(created.id);
          vscode.window.showInformationMessage("Connection created.");
        }

        const activeConnection = connectionManager.getActiveConnection();
        databaseExplorerProvider.setConnection(activeConnection);
        queryHistoryProvider.setActiveConnection(activeConnection?.id);
        updateConnectionStatusBar();
        editor = undefined;
        await panel.updateState(buildWebviewState());
        return;
      }

      const selected = connectionManager.getConnection(message.id);
      if (!selected) {
        await panel.updateState(buildWebviewState());
        return;
      }

      if (message.type === "delete") {
        const confirmation = await vscode.window.showWarningMessage(
          `Delete connection '${selected.settings.name}'?`,
          { modal: true },
          "Delete"
        );
        if (confirmation === "Delete") {
          await connectionManager.deleteConnection(message.id);
          const activeConnection = connectionManager.getActiveConnection();
          databaseExplorerProvider.setConnection(activeConnection);
          queryHistoryProvider.setActiveConnection(activeConnection?.id);
          updateConnectionStatusBar();
          if (editor?.mode === "edit" && editor.draft.id === message.id) {
            editor = undefined;
          }
        }

        await panel.updateState(buildWebviewState());
        return;
      }

      if (message.type === "activate") {
        const active = await connectionManager.setActiveConnection(message.id);
        databaseExplorerProvider.setConnection(active);
        queryHistoryProvider.setActiveConnection(active?.id);
        updateConnectionStatusBar();
        if (active) {
          vscode.window.showInformationMessage(`Active connection set to '${active.settings.name}'.`);
        }
        await panel.updateState(buildWebviewState());
        return;
      }

      if (message.type === "test") {
        const result = await httpClient.testConnection(selected);
        connectionManager.setConnectionStatus(message.id, result.isHealthy);
        updateConnectionStatusBar();
        vscode.window.showInformationMessage(
          result.isHealthy ? `Connection test succeeded for '${selected.settings.name}'.` : `Connection test failed: ${result.message}`
        );
        await panel.updateState(buildWebviewState());
      }
    });

    context.subscriptions.push(panel.panel);
  });

  const test = vscode.commands.registerCommand("vng.testConnection", async () => {
    const connection = await ensureConnection(context);
    if (!connection) {
      vscode.window.showWarningMessage("No VoltNueronGrid connection configured.");
      return;
    }

    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "VoltNueronGrid: Testing connectivity",
      },
      async () => {
        try {
          const checks = await runConnectivityChecks(connection);
          const failed = checks.filter((check) => !check.ok);

          const summary = checks
            .map((check) => `${check.method} ${check.endpoint} -> ${check.status} (${check.ok ? "ok" : "failed"})`)
            .join("\n");

          if (failed.length > 0) {
            output.appendLine("[Connectivity] Failed checks:");
            output.appendLine(summary);
            output.show(true);
            vscode.window.showErrorMessage("Connectivity test failed. See VoltNueronGrid output channel for details.");
            return;
          }

          output.appendLine("[Connectivity] Passed checks:");
          output.appendLine(summary);
          vscode.window.showInformationMessage("Connectivity test passed.");
        } catch (error) {
          const message = error instanceof Error ? error.message : "Unknown connectivity error";
          vscode.window.showErrorMessage(`Connectivity test failed: ${message}`);
        }
      }
    );
  });

  const queryRunner = vscode.commands.registerCommand("vng.runQuery", async () => {
    const connection = await ensureConnection(context);
    if (!connection) {
      return;
    }

    const sql = await vscode.window.showInputBox({
      title: "VoltNueronGrid Query Runner",
      prompt: "Enter SQL to execute",
      placeHolder: "SELECT 1;",
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim().length === 0 ? "SQL is required." : undefined),
    });
    if (!sql) {
      return;
    }

    const managedConnection = connectionManager.getActiveConnection();
    if (!managedConnection) {
      vscode.window.showWarningMessage("No active managed connection. Use Manage Connections and activate one.");
      return;
    }

    await executeManagedSqlWithProgress(managedConnection, sql, "Query Runner", {
      stopOnError: true,
    });
  });

  const showQueryResults = vscode.commands.registerCommand("vng.showQueryResults", async () => {
    if (!latestQueryResultState) {
      latestQueryResultState = buildDefaultQueryResultState();
    }
    const panel = ensureQueryResultsPanel();
    await panel.updateState(latestQueryResultState);
    panel.reveal();
  });

  const refreshQueryHistory = vscode.commands.registerCommand("vng.refreshQueryHistory", async () => {
    queryHistoryProvider.refresh();
  });

  const clearQueryHistory = vscode.commands.registerCommand("vng.clearQueryHistory", async () => {
    const active = connectionManager.getActiveConnection();
    const confirmation = await vscode.window.showWarningMessage(
      "Clear query history for the active connection?",
      { modal: true },
      "Clear"
    );
    if (confirmation !== "Clear") {
      return;
    }

    await queryExecutionService.clearHistory(active?.id);
    queryHistoryProvider.refresh();
    vscode.window.showInformationMessage("Query history cleared.");
  });

  const searchQueryHistory = vscode.commands.registerCommand("vng.searchQueryHistory", async () => {
    const active = connectionManager.getActiveConnection();
    const term = await vscode.window.showInputBox({
      title: "Search Query History",
      prompt: "Enter SQL text to search",
      ignoreFocusOut: true,
    });

    if (term === undefined) {
      return;
    }

    const matches = queryExecutionService.searchHistory(term, active?.id);
    if (matches.length === 0) {
      vscode.window.showInformationMessage("No query history entries matched your search.");
      return;
    }

    const pick = await vscode.window.showQuickPick(
      matches.slice(0, 50).map((entry) => ({
        label: entry.query.replace(/\s+/g, " ").trim().slice(0, 120),
        description: `${entry.status} • ${entry.executionTime ?? 0} ms`,
        detail: new Date(entry.timestamp).toLocaleString(),
        entryId: entry.id,
      })),
      {
        title: "Query History Matches",
        placeHolder: "Select an entry to re-run",
      }
    );

    if (!pick) {
      return;
    }

    await vscode.commands.executeCommand("vng.reRunHistoryQuery", pick.entryId);
  });

  const reRunHistoryQuery = vscode.commands.registerCommand("vng.reRunHistoryQuery", async (entryId: string) => {
    const entry = queryExecutionService.getHistoryEntry(entryId);
    if (!entry) {
      vscode.window.showWarningMessage("Query history entry was not found.");
      return;
    }

    const active = connectionManager.getActiveConnection();
    if (!active) {
      vscode.window.showWarningMessage("No active managed connection. Activate a connection before re-running queries.");
      return;
    }

    await executeManagedSqlWithProgress(active, entry.query, "History Re-run", {
      stopOnError: true,
    });
  });

  const cancelActiveQuery = vscode.commands.registerCommand("vng.cancelActiveQuery", async () => {
    const cancelled = queryExecutionService.cancelAllExecutions();
    if (cancelled === 0) {
      vscode.window.showInformationMessage("No active queries to cancel.");
      return;
    }
    vscode.window.showInformationMessage(`Cancelled ${cancelled} active quer${cancelled === 1 ? "y" : "ies"}.`);
  });

  const diagnostics = vscode.commands.registerCommand("vng.analyzeQuery", async () => {
    const connection = await ensureConnection(context);
    if (!connection) {
      return;
    }

    const sql = await vscode.window.showInputBox({
      title: "VoltNueronGrid Query Diagnostics",
      prompt: "Enter SQL to analyze",
      placeHolder: "SELECT * FROM tenant/acme/users;",
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim().length === 0 ? "SQL is required." : undefined),
    });
    if (!sql) {
      return;
    }

    const response = await analyzeSql(connection, sql);
    await presentResponse("Query Diagnostics", response.status, response.bodyText, connection, output);
  });

  const schema = vscode.commands.registerCommand("vng.showSchemaRegistry", async () => {
    const connection = await ensureConnection(context);
    if (!connection) {
      return;
    }

    const response = await getSchemaRegistry(connection);
    await presentResponse("Schema Registry", response.status, response.bodyText, connection, output);
  });

  const focusPanel = vscode.commands.registerCommand("vng.focusPanel", async () => {
    // Open the contributed activity container first.
    await vscode.commands.executeCommand("workbench.view.extension.vngExplorer");
    // Then focus the concrete view to ensure visibility.
    await vscode.commands.executeCommand("vngActions.focus");
  });

  // Database Explorer commands
  const refreshSchema = vscode.commands.registerCommand("vng.refreshSchema", async () => {
    try {
      const activeConnection = connectionManager.getActiveConnection();
      if (!activeConnection) {
        vscode.window.showWarningMessage("No connection active");
        return;
      }

      await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Window,
          title: "Refreshing database schema...",
        },
        async () => {
          schemaManager.invalidateCache(activeConnection.id);
          databaseExplorerProvider.refresh();
        }
      );

      vscode.window.showInformationMessage("Schema refreshed successfully");
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unknown error";
      vscode.window.showErrorMessage(`Failed to refresh schema: ${message}`);
    }
  });

  const copyName = vscode.commands.registerCommand("vng.copyName", async (element: SchemaTreeItem) => {
    await handleCopyName(element);
  });

  const showTableDDL = vscode.commands.registerCommand("vng.showTableDDL", async (element: SchemaTreeItem) => {
    await handleShowDDL(element);
  });

  const showSQLTemplate = vscode.commands.registerCommand("vng.showSQLTemplate", async (element: SchemaTreeItem) => {
    await handleSQLTemplate(element);
  });

  const generateMockData = vscode.commands.registerCommand("vng.generateMockData", async (element: SchemaTreeItem) => {
    await handleGenerateMockData(element);
  });

  const dumpTableStruct = vscode.commands.registerCommand("vng.dumpTableStruct", async (element: SchemaTreeItem) => {
    await handleDumpStruct(element);
  });

  const dropTable = vscode.commands.registerCommand("vng.dropTable", async (element: SchemaTreeItem) => {
    await handleDropTable(element);
  });

  const sqlDisposables = registerSqlEditorFeatures({
    context,
    output,
    getConnection: () => ensureConnection(context),
    connectionManager,
    queryExecutionService,
    schemaManager,
    onQueryResult: async (result, operation, connectionName) => {
      await publishQueryResult(result, operation, connectionName);
      queryHistoryProvider.refresh();
    },
  });

  const executeManagedSqlWithProgress = async (
    connection: Connection,
    sql: string,
    operation: string,
    streamOptions?: Pick<QueryStreamOptions, "stopOnError">
  ): Promise<void> => {
    const statements = queryExecutionService.parseStatements(sql);
    const executionId = `exec-${Date.now()}`;
    const timeoutMs = connection.settings.advanced.connectionTimeout ?? 30000;

    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: `VoltNueronGrid: ${operation}`,
        cancellable: true,
      },
      async (_progress, cancellationToken) => {
        const controller = new AbortController();
        cancellationToken.onCancellationRequested(() => {
          controller.abort();
          queryExecutionService.cancelAllExecutions();
        });

        await queryExecutionService.executeStatementsStream(connection, statements, {
          executionId,
          timeoutMs,
          stopOnError: streamOptions?.stopOnError ?? true,
          signal: controller.signal,
          onResult: async (result, index, total) => {
            output.appendLine(`[${operation}] statement ${index}/${total} status=${result.status} rows=${result.rowCount} time=${result.executionTime}ms`);
            if (result.status !== "success" && result.error) {
              output.appendLine(`[${operation}] ${result.error.message}`);
            }
            output.appendLine("---");
            output.show(true);

            await publishQueryResult(result, `${operation} (${index}/${total})`, connection.settings.name);
            queryHistoryProvider.refresh();
          },
        });
      }
    );
  };

  context.subscriptions.push(
    connect,
    manageConnections,
    quickSwitchConnection,
    test,
    queryRunner,
    showQueryResults,
    refreshQueryHistory,
    clearQueryHistory,
    searchQueryHistory,
    reRunHistoryQuery,
    cancelActiveQuery,
    diagnostics,
    schema,
    focusPanel,
    refreshSchema,
    copyName,
    showTableDDL,
    showSQLTemplate,
    generateMockData,
    dumpTableStruct,
    dropTable,
    ...sqlDisposables,
    actionsView,
    databaseView,
    queryHistoryView,
    connectionStatusBar,
    output
  );
}

export function deactivate(): void {
  // Clean up service resources if needed
  schemaManager.clearCache();
  schemaManager.dispose();
}

async function ensureConnection(context: vscode.ExtensionContext): Promise<RuntimeConnection | undefined> {
  const activeManagedConnection = connectionManager.getActiveConnection();
  if (activeManagedConnection) {
    return toRuntimeConnection(activeManagedConnection);
  }

  const current = await readConnection(context);
  if (current) {
    await upsertManagedConnection(current);
    return current;
  }

  const configured = await runConnectionWizard(context);
  if (configured) {
    await upsertManagedConnection(configured);
  }
  return configured;
}

async function presentResponse(
  operation: string,
  status: number,
  bodyText: string,
  connection: RuntimeConnection,
  output: vscode.OutputChannel
): Promise<void> {
  output.appendLine(`[${operation}] HTTP ${status}`);
  output.appendLine(bodyText || "(empty response)");
  output.appendLine("---");
  output.show(true);

  if (status === 200) {
    vscode.window.showInformationMessage(`${operation} completed successfully.`);
    return;
  }

  const message = toPermissionMessage(status, connection.settings.mode);
  if (message) {
    vscode.window.showWarningMessage(`${operation}: ${message}`);
    return;
  }

  vscode.window.showErrorMessage(`${operation} failed with HTTP ${status}.`);
}

/**
 * Get global service instances (for use by other modules)
 */
export function getServices() {
  return {
    connectionManager,
    httpClient,
    queryExecutionService,
    schemaManager,
  };
}

async function upsertManagedConnection(runtimeConnection: RuntimeConnection): Promise<Connection> {
  const managedSettings = toManagedSettings(runtimeConnection);

  const existing = connectionManager
    .listConnections()
    .find(
      (candidate) =>
        candidate.settings.baseUrl === managedSettings.baseUrl &&
        candidate.settings.mode === managedSettings.mode &&
        candidate.settings.operatorId === managedSettings.operatorId &&
        candidate.settings.tenantId === managedSettings.tenantId &&
        candidate.settings.userId === managedSettings.userId
    );

  if (existing) {
    const updated = await connectionManager.updateConnection(existing.id, managedSettings);
    const active = await connectionManager.setActiveConnection(existing.id);
    return active ?? updated ?? existing;
  }

  const created = await connectionManager.addConnection(managedSettings);
  const active = await connectionManager.setActiveConnection(created.id);
  return active ?? created;
}

function toManagedSettings(runtimeConnection: RuntimeConnection): ManagedConnectionSettings {
  const url = new URL(runtimeConnection.settings.baseUrl);
  const host = url.hostname;
  const port = url.port ? Number(url.port) : url.protocol === "https:" ? 443 : 80;
  const baseName = `${runtimeConnection.settings.mode}-${host}:${port}`;

  return createDefaultConnection({
    id: `conn-${Date.now()}`,
    name: `Runtime ${baseName}`,
    serverType: "voltnuerongrid",
    runtimeTarget: runtimeConnection.settings.runtimeTarget,
    baseUrl: runtimeConnection.settings.baseUrl,
    host,
    port,
    mode: runtimeConnection.settings.mode,
    adminKey: runtimeConnection.adminApiKey,
    operatorId: runtimeConnection.settings.operatorId,
    tenantId: runtimeConnection.settings.tenantId,
    userId: runtimeConnection.settings.userId,
  });
}

function toRuntimeConnection(connection: Connection): RuntimeConnection {
  return {
    settings: {
      baseUrl: connection.settings.baseUrl,
      runtimeTarget: connection.settings.runtimeTarget,
      mode: connection.settings.mode,
      operatorId: connection.settings.operatorId,
      tenantId: connection.settings.tenantId,
      userId: connection.settings.userId,
    },
    adminApiKey: connection.settings.adminKey,
  };
}
