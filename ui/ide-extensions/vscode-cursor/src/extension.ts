import * as vscode from "vscode";
import { promises as fs } from "fs";
import { readConnection } from "./config";
import { analyzeSql, executeSql, getSchemaRegistry, runConnectivityChecks, toPermissionMessage } from "./client";
import { RuntimeConnection } from "./config";
import {
  Connection,
  ConnectionSettings as ManagedConnectionSettings,
  createDefaultConnection,
  TableEditorTarget,
  validateConnectionSettings,
} from "./models";
import {
  ConnectionManager,
  HttpClient,
  QueryExecutionService,
  QueryStreamOptions,
  SchemaManager,
  TableEditorService,
  createConnectionManager,
  createHttpClient,
  createQueryExecutionService,
  createSchemaManager,
  createTableEditorService,
  redactSecrets,
  toSafeErrorMessage,
} from "./services";
import {
  DatabaseExplorerProvider,
  QueryHistoryProvider,
  createDatabaseExplorerProvider,
  createQueryHistoryProvider,
} from "./providers";
import {
  buildConnectionStatusSummary,
  getConnectionHostLabel,
  handleCopyName,
  toConnectionExportJson,
  runAlterTableWizard,
  runCreateTableWizard,
  handleShowDDL,
  handleSQLTemplate,
  handleGenerateMockData,
  handleDumpStruct,
  handleDropTable,
  handleTruncateTable,
  registerSettingsCommands,
} from "./commands";
import { SchemaTreeItem } from "./providers/DatabaseExplorerProvider";
import { registerSqlEditorFeatures } from "./sql";
import {
  ConnectionManagerMessage,
  ConnectionManagerState,
  WebviewConnectionDraft,
  createConnectionManagerPanel,
} from "./ui/ConnectionManagerWebview";
import { ConnectionEditorMessage, ConnectionEditorState, createConnectionEditorPanel } from "./ui/ConnectionEditorWebview";
import { QueryResult, exportAsCSV, exportAsJSON } from "./models";
import { QueryResultsMessage, createQueryResultsPanel } from "./ui/QueryResultsWebview";
import { QueryResultsState, createDefaultQueryResultsState, createQueryResultsState } from "./ui/QueryResultsState";
import { TableEditorMessage, TableEditorState, createTableEditorPanel } from "./ui/TableEditorWebview";
import { readTransportInjectionFromConfig } from "./transportConfig";
import { appendTransportLogLine } from "./transportLog";

// Global service instances
let connectionManager: ConnectionManager;
let httpClient: HttpClient;
let queryExecutionService: QueryExecutionService;
let schemaManager: SchemaManager;
let tableEditorService: TableEditorService;
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

    const transport = readTransportInjectionFromConfig();
    const healthIcon = active.state === "verified" ? "$(pass-filled)" : active.state === "degraded" ? "$(warning)" : "$(circle-large-outline)";
    connectionStatusBar.text = `${healthIcon} $(database) ${active.settings.name}`;
    connectionStatusBar.tooltip = [
      `Active connection: ${active.settings.name}`,
      `Mode: ${active.settings.mode}`,
      `Base URL: ${active.settings.baseUrl}`,
      `Transport (settings): ${transport.transportMode}${
        transport.nativeEndpoint ? ` — native ${transport.nativeEndpoint}` : ""
      }`,
      `Health: ${active.state}`,
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
  tableEditorService = createTableEditorService(httpClient, schemaManager);

  // Initialize database explorer
  databaseExplorerProvider = createDatabaseExplorerProvider(context.extensionUri, schemaManager);
  queryHistoryProvider = createQueryHistoryProvider(queryExecutionService);

  const transportInject = readTransportInjectionFromConfig();
  output.appendLine(
    `[VoltNueronGrid] Extension activated (v0.3.2) — transportMode=${transportInject.transportMode}` +
      (transportInject.nativeEndpoint ? ` nativeEndpoint=${transportInject.nativeEndpoint}` : "")
  );
  appendTransportLogLine(
    `activated transportMode=${transportInject.transportMode}` +
      (transportInject.nativeEndpoint ? ` nativeEndpoint=${transportInject.nativeEndpoint}` : "") +
      " (query data-plane remains HTTP until native execution is integrated)"
  );

  let latestQueryResult: QueryResult | undefined;
  let latestQueryResultState: QueryResultsState | undefined;
  let queryResultsPanel: ReturnType<typeof createQueryResultsPanel> | undefined;
  let connectionManagerPanel: ReturnType<typeof createConnectionManagerPanel> | undefined;
  let connectionEditorPanel: ReturnType<typeof createConnectionEditorPanel> | undefined;
  let connectionEditorState: ConnectionEditorState | undefined;
  let tableEditorState: TableEditorState | undefined;
  let tableEditorPanel: ReturnType<typeof createTableEditorPanel> | undefined;

  const buildDefaultQueryResultState = (): QueryResultsState =>
    createDefaultQueryResultsState(connectionManager.getActiveConnection()?.settings.name ?? "No active connection");

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
    latestQueryResultState = createQueryResultsState(result, operation, connectionName);

    const panel = ensureQueryResultsPanel();
    await panel.updateState(latestQueryResultState);
    panel.reveal();
  };

  const syncTableEditorPanel = async () => {
    if (tableEditorPanel && tableEditorState) {
      await tableEditorPanel.updateState(tableEditorState);
    }
  };

  const defaultConnectionDraft = (): WebviewConnectionDraft => ({
    name: "",
    group: "",
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
    group: connection.settings.group ?? "",
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

  const buildConnectionManagerState = (): ConnectionManagerState => ({
    connections: connectionManager.listConnections().map((connection) => ({
      id: connection.id,
      name: connection.settings.name,
      mode: connection.settings.mode,
      baseUrl: connection.settings.baseUrl,
      active: connection.isActive,
      connected: connection.isConnected,
    })),
  });

  const syncConnectionViews = async () => {
    const activeConnection = connectionManager.getActiveConnection();
    databaseExplorerProvider.setConnections(connectionManager.listConnections());
    databaseExplorerProvider.setConnection(activeConnection);
    queryHistoryProvider.setActiveConnection(activeConnection?.id);
    updateConnectionStatusBar();
    if (connectionManagerPanel) {
      await connectionManagerPanel.updateState(buildConnectionManagerState());
    }
  };

  /**
   * "Verified" means the last HTTP health probe to `${baseUrl}/health` succeeded (HTTP 200).
   * Active = selected profile; verified = runtime reachable with current credentials.
   */
  const verifyConnectionHealth = async (connection: Connection, options?: { silent?: boolean }): Promise<boolean> => {
    try {
      const result = await httpClient.testConnection(connection);
      connectionManager.setConnectionStatus(connection.id, result.isHealthy, {
        lastCheckedAt: Date.now(),
        reason: result.isHealthy ? "health_ok" : "health_probe_failed",
        detail: result.message,
      });
      if (!result.isHealthy) {
        const detail = redactSecrets(result.message);
        output.appendLine(`[Health] '${connection.settings.name}' not verified: ${detail}`);
        if (!options?.silent) {
          vscode.window.showWarningMessage(formatConnectionFailureMessage(connection, `Could not verify '${connection.settings.name}'`, detail));
        }
      }
      return result.isHealthy;
    } catch (error) {
      connectionManager.setConnectionStatus(connection.id, false);
      connectionManager.setConnectionState(connection.id, "error", {
        lastCheckedAt: Date.now(),
        reason: "health_probe_exception",
        detail: toSafeErrorMessage(error, "Health check failed."),
      });
      const detail = toSafeErrorMessage(error, "Health check failed.");
      output.appendLine(`[Health] '${connection.settings.name}' not verified: ${detail}`);
      if (!options?.silent) {
        vscode.window.showWarningMessage(
          formatConnectionFailureMessage(connection, `Could not verify '${connection.settings.name}'`, redactSecrets(detail))
        );
      }
      return false;
    }
  };

  const verifyActiveProfileHealth = async (options?: { silent?: boolean }): Promise<void> => {
    const active = connectionManager.getActiveConnection();
    if (!active) {
      return;
    }
    await verifyConnectionHealth(active, options);
    await syncConnectionViews();
  };

  const closeConnectionEditorPanel = () => {
    connectionEditorState = undefined;
    if (connectionEditorPanel) {
      connectionEditorPanel.panel.dispose();
      connectionEditorPanel = undefined;
    }
  };

  const saveConnectionDraft = async (draft: WebviewConnectionDraft, mode: "create" | "edit") => {
    try {
      if (!draft.name.trim()) {
        vscode.window.showWarningMessage("Connection name is required.");
        return false;
      }

      let parsedUrl: URL;
      try {
        parsedUrl = new URL(draft.baseUrl);
        if (parsedUrl.protocol !== "http:" && parsedUrl.protocol !== "https:") {
          throw new Error("Only http/https URLs are supported.");
        }
      } catch {
        vscode.window.showWarningMessage("Enter a valid base URL (http/https).");
        return false;
      }

      const existingConnection = draft.id ? connectionManager.getConnection(draft.id) : undefined;
      const hasAdminKey = Boolean((draft.adminKey ?? "").trim() || (existingConnection?.settings.adminKey ?? "").trim());

      if ((draft.mode === "admin" || draft.mode === "operator") && !hasAdminKey) {
        vscode.window.showWarningMessage(`${draft.mode === "admin" ? "Admin" : "Operator"} mode requires an Admin Key.`);
        return false;
      }

      if (draft.mode === "operator" && !(draft.operatorId ?? "").trim()) {
        vscode.window.showWarningMessage("Operator mode requires Operator ID.");
        return false;
      }

      if (draft.mode === "tenant" && !(draft.tenantId ?? "").trim()) {
        vscode.window.showWarningMessage("Tenant mode requires Tenant ID.");
        return false;
      }

      if (draft.mode === "tenant" && !(draft.userId ?? "").trim()) {
        vscode.window.showWarningMessage("Tenant mode requires User ID.");
        return false;
      }

      const connectionPatch: Partial<ManagedConnectionSettings> = {
        name: draft.name.trim(),
        group: (draft.group ?? "").trim() || undefined,
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
        return false;
      }

      await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Notification,
          title: mode === "edit" ? "Updating connection" : "Creating connection",
        },
        async () => {
          if (mode === "edit" && draft.id) {
            await connectionManager.updateConnection(draft.id, connectionPatch);
          } else {
            const created = await connectionManager.addConnection(
              createDefaultConnection({
                id: `conn-${Date.now()}`,
                ...connectionPatch,
                serverType: "voltnuerongrid",
              })
            );
            await connectionManager.setActiveConnection(created.id);
          }
        }
      );

      vscode.window.showInformationMessage(mode === "edit" ? "Connection updated." : "Connection created.");
      await syncConnectionViews();
      await verifyActiveProfileHealth({ silent: true });
      closeConnectionEditorPanel();
      return true;
    } catch (error) {
      const safeMessage = toSafeErrorMessage(error, "Unexpected connection error.");
      output.appendLine(`[Connection] ${mode === "edit" ? "Update" : "Create"} failed: ${safeMessage}`);
      output.show(true);
      vscode.window.showErrorMessage(`${mode === "edit" ? "Update" : "Create"} connection failed. Check output for details.`);
      return false;
    }
  };

  const openConnectionEditor = async (state: ConnectionEditorState) => {
    connectionEditorState = state;
    if (!connectionEditorPanel) {
      connectionEditorPanel = createConnectionEditorPanel(context, state, async (message: ConnectionEditorMessage) => {
        if (message.type === "cancel") {
          closeConnectionEditorPanel();
          return;
        }

        await saveConnectionDraft(message.draft, connectionEditorState?.mode ?? "create");
      });
      connectionEditorPanel.panel.onDidDispose(() => {
        connectionEditorState = undefined;
        connectionEditorPanel = undefined;
      });
      context.subscriptions.push(connectionEditorPanel.panel);
      return;
    }

    await connectionEditorPanel.updateState(state);
    connectionEditorPanel.panel.reveal(vscode.ViewColumn.Beside);
  };

  const resolveConnectionId = (input?: SchemaTreeItem | string): string | undefined => {
    if (!input) {
      return undefined;
    }
    if (typeof input === "string") {
      return input;
    }
    if (input.type === "connection") {
      return (input.data as Connection).id;
    }
    return undefined;
  };

  const resolveConnectionFromInputOrActive = (input?: SchemaTreeItem | string): Connection | undefined => {
    const connectionId = resolveConnectionId(input);
    if (connectionId) {
      return connectionManager.getConnection(connectionId) ?? undefined;
    }
    return connectionManager.getActiveConnection() ?? undefined;
  };

  const buildConnectionRemediationHint = (connection: Connection, rawMessage: string): string | undefined => {
    const message = rawMessage.toLowerCase();

    if (
      message.includes("401") ||
      message.includes("403") ||
      message.includes("unauthorized") ||
      message.includes("forbidden")
    ) {
      if (connection.settings.mode === "admin" || connection.settings.mode === "operator") {
        return "Verify Admin Key in the connection profile and confirm it matches VNG_ADMIN_API_KEY.";
      }
      return "Verify tenant credentials (Tenant ID and User ID) and make sure they map to a valid role binding.";
    }

    if (message.includes("operator") || message.includes("x-vng-operator-id")) {
      return "Set a valid Operator ID for this profile and confirm it has role bindings on the server.";
    }

    if (
      message.includes("tenant") ||
      message.includes("x-vng-tenant-id") ||
      message.includes("x-vng-user-id")
    ) {
      return "Set Tenant ID and User ID in the profile and verify they are scoped to the expected tenant.";
    }

    if (
      message.includes("timeout") ||
      message.includes("timed out") ||
      message.includes("abort") ||
      message.includes("etimedout")
    ) {
      return "Check network reachability to Base URL and consider increasing Connection Timeout in Advanced settings.";
    }

    if (
      message.includes("econnrefused") ||
      message.includes("fetch failed") ||
      message.includes("enotfound") ||
      message.includes("getaddrinfo") ||
      message.includes("dns")
    ) {
      return "Confirm the server is running and Base URL host/port are correct for this runtime target.";
    }

    if (message.includes("ssl") || message.includes("certificate") || message.includes("tls")) {
      return "Review TLS settings (CA/cert/key paths) and verify certificate trust for the target endpoint.";
    }

    return undefined;
  };

  const formatConnectionFailureMessage = (connection: Connection, operation: string, detail: string): string => {
    const hint = buildConnectionRemediationHint(connection, detail);
    return hint
      ? `${operation} failed: ${detail}\nHint: ${hint}`
      : `${operation} failed: ${detail}`;
  };

  const notifyConnectionFailure = (operation: string, error: unknown, connection?: Connection): void => {
    const safeMessage = toSafeErrorMessage(error, "Unexpected connection error.");
    output.appendLine(`[Connection] ${operation} failed: ${safeMessage}`);
    output.show(true);
    if (!connection) {
      vscode.window.showErrorMessage(`${operation} failed. Check VoltNueronGrid output for details.`);
      return;
    }
    vscode.window.showErrorMessage(formatConnectionFailureMessage(connection, operation, redactSecrets(safeMessage)));
  };

  const resolveTableEditorConnection = (): Connection | undefined =>
    connectionManager.getConnection(tableEditorState?.connectionId ?? "") ?? undefined;

  const handleTableEditorMessage = async (message: TableEditorMessage): Promise<void> => {
    if (!tableEditorState) {
      return;
    }

    try {
      switch (message.type) {
        case "ready":
          await syncTableEditorPanel();
          return;
        case "updateCell":
          tableEditorState = {
            ...tableEditorState,
            session: tableEditorService.updateCell(tableEditorState.session, message.rowId, message.columnName, message.value),
          };
          await syncTableEditorPanel();
          return;
        case "addRow":
          tableEditorState = {
            ...tableEditorState,
            session: tableEditorService.addDraftRow(tableEditorState.session),
          };
          await syncTableEditorPanel();
          return;
        case "toggleDeleteRow":
          tableEditorState = {
            ...tableEditorState,
            session: tableEditorService.toggleDeleteRow(tableEditorState.session, message.rowId),
          };
          await syncTableEditorPanel();
          return;
        case "discard": {
          const connection = resolveTableEditorConnection();
          if (!connection) {
            vscode.window.showWarningMessage("The table editor connection is no longer available.");
            return;
          }
          tableEditorState = {
            ...tableEditorState,
            session: await tableEditorService.discardChanges(connection, tableEditorState.session),
          };
          await syncTableEditorPanel();
          return;
        }
        case "refresh": {
          const connection = resolveTableEditorConnection();
          if (!connection) {
            vscode.window.showWarningMessage("The table editor connection is no longer available.");
            return;
          }
          tableEditorState = {
            ...tableEditorState,
            session: await tableEditorService.openSession(
              connection,
              tableEditorState.session.target,
              tableEditorState.session.page,
              tableEditorState.session.pageSize,
              "Rows refreshed."
            ),
          };
          await syncTableEditorPanel();
          return;
        }
        case "changePage": {
          if (tableEditorState.session.dirty) {
            vscode.window.showWarningMessage("Save or discard changes before navigating pages.");
            return;
          }
          const connection = resolveTableEditorConnection();
          if (!connection) {
            vscode.window.showWarningMessage("The table editor connection is no longer available.");
            return;
          }
          tableEditorState = {
            ...tableEditorState,
            session: await tableEditorService.changePage(connection, tableEditorState.session, message.direction),
          };
          await syncTableEditorPanel();
          return;
        }
        case "save": {
          const connection = resolveTableEditorConnection();
          if (!connection) {
            vscode.window.showWarningMessage("The table editor connection is no longer available.");
            return;
          }
          tableEditorState = {
            ...tableEditorState,
            session: await tableEditorService.saveSession(connection, tableEditorState.session),
          };
          schemaManager.invalidateCache(connection.id);
          databaseExplorerProvider.refresh();
          await syncTableEditorPanel();
          return;
        }
        case "copyPendingSql": {
          const pendingSql = tableEditorState.session.pendingSaveSql;
          if (!pendingSql || pendingSql.length === 0) {
            vscode.window.showInformationMessage("No pending SQL is available.");
            return;
          }
          const document = await vscode.workspace.openTextDocument({
            language: "sql",
            content: pendingSql.join("\n\n"),
          });
          await vscode.window.showTextDocument(document, vscode.ViewColumn.Beside);
          return;
        }
        default:
          return;
      }
    } catch (error) {
      const messageText = error instanceof Error ? error.message : String(error);
      tableEditorState = {
        ...tableEditorState,
        session: {
          ...tableEditorState.session,
          errorMessage: messageText,
          infoMessage: undefined,
        },
      };
      await syncTableEditorPanel();
    }
  };

  const openTableEditorForTarget = async (activeConnection: Connection, target: TableEditorTarget): Promise<void> => {
    tableEditorState = {
      connectionId: activeConnection.id,
      connectionName: activeConnection.settings.name,
      session: await tableEditorService.openSession(activeConnection, target),
    };

    const panel = ensureTableEditorPanel();
    await panel.updateState(tableEditorState);
    panel.reveal();
  };

  const ensureTableEditorPanel = (): ReturnType<typeof createTableEditorPanel> => {
    if (tableEditorPanel && tableEditorState) {
      return tableEditorPanel;
    }

    const initialState =
      tableEditorState ?? {
        connectionId: connectionManager.getActiveConnection()?.id ?? "",
        connectionName: connectionManager.getActiveConnection()?.settings.name ?? "No active connection",
        session: {
          target: { database: "", schema: "", tableName: "" },
          table: { name: "", schema: "", columns: [], indexes: [] },
          columns: [],
          capabilities: {
            canInsert: false,
            canUpdate: false,
            canDelete: false,
            keyColumns: [],
          },
          rows: [],
          page: 1,
          pageSize: 50,
          hasNextPage: false,
          dirty: false,
          infoMessage: "Open a table from the Database Explorer to start editing.",
        },
      };

    tableEditorPanel = createTableEditorPanel(context, initialState, handleTableEditorMessage);

    tableEditorPanel.panel.onDidDispose(() => {
      tableEditorPanel = undefined;
    });

    context.subscriptions.push(tableEditorPanel.panel);
    return tableEditorPanel;
  };

  // Create tree views
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
    databaseExplorerProvider.setConnections(connectionManager.listConnections());
    databaseExplorerProvider.setConnection(activeConnection);
    queryHistoryProvider.setActiveConnection(activeConnection.id);
  } else {
    // Backward-compatible migration from legacy single-connection settings.
    const legacyConnection = await readConnection(context);
    if (legacyConnection) {
      const migrated = await upsertManagedConnection(legacyConnection);
      databaseExplorerProvider.setConnections(connectionManager.listConnections());
      databaseExplorerProvider.setConnection(migrated);
      queryHistoryProvider.setActiveConnection(migrated.id);
    }
  }
  databaseExplorerProvider.setConnections(connectionManager.listConnections());
  updateConnectionStatusBar();
  void verifyActiveProfileHealth({ silent: true });

  const quickSwitchConnection = vscode.commands.registerCommand("vng.quickSwitchConnection", async () => {
    try {
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
      await syncConnectionViews();
      await verifyActiveProfileHealth({ silent: true });
      if (active) {
        vscode.window.showInformationMessage(`Active connection set to '${active.settings.name}'.`);
      }
    } catch (error) {
      notifyConnectionFailure("Quick switch connection", error);
    }
  });

  const manageConnections = vscode.commands.registerCommand("vng.manageConnections", async () => {
    if (connectionManagerPanel) {
      await connectionManagerPanel.updateState(buildConnectionManagerState());
      connectionManagerPanel.panel.reveal(vscode.ViewColumn.One);
      return;
    }

    connectionManagerPanel = createConnectionManagerPanel(context, buildConnectionManagerState(), async (message: ConnectionManagerMessage) => {
      if (message.type === "refresh") {
        await connectionManagerPanel?.updateState(buildConnectionManagerState());
        return;
      }

      if (message.type === "openCreate") {
        await openConnectionEditor({
          mode: "create",
          draft: defaultConnectionDraft(),
        });
        return;
      }

      if (message.type === "openEdit") {
        const selected = connectionManager.getConnection(message.id);
        if (!selected) {
          return;
        }
        await openConnectionEditor({
          mode: "edit",
          draft: draftFromConnection(selected),
        });
        return;
      }

      const selected = connectionManager.getConnection(message.id);
      if (!selected) {
        await connectionManagerPanel?.updateState(buildConnectionManagerState());
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
          await syncConnectionViews();
        }

        await connectionManagerPanel?.updateState(buildConnectionManagerState());
        return;
      }

      if (message.type === "activate") {
        try {
          const active = await connectionManager.setActiveConnection(message.id);
          await syncConnectionViews();
          await verifyActiveProfileHealth({ silent: true });
          if (active) {
            vscode.window.showInformationMessage(`Active connection set to '${active.settings.name}'.`);
          }
        } catch (error) {
          notifyConnectionFailure("Activate profile", error, selected);
        }
        await connectionManagerPanel?.updateState(buildConnectionManagerState());
        return;
      }

      if (message.type === "test") {
        try {
          const result = await httpClient.testConnection(selected);
          connectionManager.setConnectionStatus(message.id, result.isHealthy, {
            lastCheckedAt: Date.now(),
            reason: result.isHealthy ? "manual_test_ok" : "manual_test_failed",
            detail: result.message,
          });
          await syncConnectionViews();
          if (result.isHealthy) {
            vscode.window.showInformationMessage(`Connection test succeeded for '${selected.settings.name}'.`);
          } else {
            const detail = redactSecrets(result.message);
            vscode.window.showErrorMessage(formatConnectionFailureMessage(selected, "Connection test", detail));
          }
        } catch (error) {
          notifyConnectionFailure("Test connection", error, selected);
        }
        await connectionManagerPanel?.updateState(buildConnectionManagerState());
      }
    });

    connectionManagerPanel.panel.onDidDispose(() => {
      connectionManagerPanel = undefined;
    });

    context.subscriptions.push(connectionManagerPanel.panel);
  });

  const newConnection = vscode.commands.registerCommand("vng.newConnection", async () => {
    await openConnectionEditor({
      mode: "create",
      draft: defaultConnectionDraft(),
    });
  });

  const editConnection = vscode.commands.registerCommand("vng.editConnection", async (input?: SchemaTreeItem | string) => {
    const connectionId = resolveConnectionId(input);
    if (!connectionId) {
      vscode.window.showWarningMessage("Select a connection to edit.");
      return;
    }
    const connection = connectionManager.getConnection(connectionId);
    if (!connection) {
      vscode.window.showWarningMessage("The selected connection is no longer available.");
      return;
    }

    await openConnectionEditor({
      mode: "edit",
      draft: draftFromConnection(connection),
    });
  });

  const connectConnection = vscode.commands.registerCommand("vng.connectConnection", async (input?: SchemaTreeItem | string) => {
    try {
      const connectionId = resolveConnectionId(input);
      if (!connectionId) {
        vscode.window.showWarningMessage("Select a connection to connect.");
        return;
      }

      const selected = connectionManager.getConnection(connectionId);
      if (!selected) {
        vscode.window.showWarningMessage("The selected connection is no longer available.");
        return;
      }

      await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Notification,
          title: "Connecting profile",
        },
        async () => {
          const testResult = await httpClient.testConnection(selected);
          connectionManager.setConnectionStatus(connectionId, testResult.isHealthy, {
            lastCheckedAt: Date.now(),
            reason: testResult.isHealthy ? "connect_ok" : "connect_failed",
            detail: testResult.message,
          });
          if (!testResult.isHealthy) {
            const detail = redactSecrets(testResult.message);
            vscode.window.showErrorMessage(formatConnectionFailureMessage(selected, "Connection test", detail));
            await syncConnectionViews();
            return;
          }

          const active = await connectionManager.setActiveConnection(connectionId);
          await syncConnectionViews();
          if (active) {
            vscode.window.showInformationMessage(`Connected and activated '${active.settings.name}'.`);
          }
        }
      );
    } catch (error) {
      const selected = connectionManager.getConnection(resolveConnectionId(input) ?? "");
      notifyConnectionFailure("Connect profile", error, selected ?? undefined);
    }
  });

  const disconnectConnection = vscode.commands.registerCommand("vng.disconnectConnection", async () => {
    try {
      const active = connectionManager.getActiveConnection();
      if (!active) {
        vscode.window.showInformationMessage("No active connection to disconnect.");
        return;
      }

      await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Notification,
          title: "Disconnecting profile",
        },
        async () => {
          await connectionManager.clearActiveConnection();
          await syncConnectionViews();
          vscode.window.showInformationMessage(`Disconnected '${active.settings.name}'.`);
        }
      );
    } catch (error) {
      notifyConnectionFailure("Disconnect profile", error);
    }
  });

  const copyConnectionHost = vscode.commands.registerCommand("vng.copyConnectionHost", async (input?: SchemaTreeItem | string) => {
    const selected = resolveConnectionFromInputOrActive(input);
    if (!selected) {
      vscode.window.showWarningMessage("Select a connection to copy host.");
      return;
    }
    const host = getConnectionHostLabel(selected);
    await vscode.env.clipboard.writeText(host);
    vscode.window.showInformationMessage(`Copied host '${host}'.`);
  });

  const copyConnectionJson = vscode.commands.registerCommand("vng.copyConnectionJson", async (input?: SchemaTreeItem | string) => {
    const selected = resolveConnectionFromInputOrActive(input);
    if (!selected) {
      vscode.window.showWarningMessage("Select a connection to copy JSON.");
      return;
    }
    await vscode.env.clipboard.writeText(toConnectionExportJson(selected));
    vscode.window.showInformationMessage(`Copied connection JSON for '${selected.settings.name}'.`);
  });

  const showConnectionStatus = vscode.commands.registerCommand("vng.showConnectionStatus", async (input?: SchemaTreeItem | string) => {
    const selected = resolveConnectionFromInputOrActive(input);
    if (!selected) {
      vscode.window.showWarningMessage("Select a connection to inspect status.");
      return;
    }
    const historyCount = queryExecutionService.getHistory(selected.id).length;
    const lines = buildConnectionStatusSummary(selected, historyCount);
    output.appendLine("[Connection Status]");
    for (const line of lines) {
      output.appendLine(line);
    }
    output.appendLine("---");
    output.show(true);
    vscode.window.showInformationMessage(lines.join(" | "));
  });

  const viewConnectionHistory = vscode.commands.registerCommand("vng.viewConnectionHistory", async (input?: SchemaTreeItem | string) => {
    const selected = resolveConnectionFromInputOrActive(input);
    if (!selected) {
      vscode.window.showWarningMessage("Select a connection to view history.");
      return;
    }

    const entries = queryExecutionService.getHistory(selected.id);
    if (entries.length === 0) {
      vscode.window.showInformationMessage(`No query history for '${selected.settings.name}'.`);
      return;
    }

    const pick = await vscode.window.showQuickPick(
      entries.slice(0, 50).map((entry) => ({
        label: entry.query.replace(/\s+/g, " ").trim().slice(0, 120),
        description: `${entry.status} • ${entry.executionTime ?? 0} ms`,
        detail: new Date(entry.timestamp).toLocaleString(),
        entryId: entry.id,
      })),
      {
        title: `History: ${selected.settings.name}`,
        placeHolder: "Select an entry to re-run",
      }
    );
    if (!pick) {
      return;
    }
    await vscode.commands.executeCommand("vng.reRunHistoryQuery", pick.entryId);
  });

  const importConnectionKey = vscode.commands.registerCommand("vng.importConnectionKey", async (input?: SchemaTreeItem | string) => {
    const selected = resolveConnectionFromInputOrActive(input);
    if (!selected) {
      vscode.window.showWarningMessage("Select a connection to import admin key.");
      return;
    }

    const key = await vscode.window.showInputBox({
      title: `Import Admin Key: ${selected.settings.name}`,
      prompt: "Paste VNG admin key",
      password: true,
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim().length === 0 ? "Admin key is required." : undefined),
    });
    if (key === undefined) {
      return;
    }

    await connectionManager.updateConnection(selected.id, { adminKey: key.trim() });
    await syncConnectionViews();
    vscode.window.showInformationMessage(`Admin key imported for '${selected.settings.name}'.`);
  });

  const deleteConnectionCommand = vscode.commands.registerCommand("vng.deleteConnection", async (input?: SchemaTreeItem | string) => {
    const connectionId = resolveConnectionId(input);
    if (!connectionId) {
      vscode.window.showWarningMessage("Select a connection to delete.");
      return;
    }

    const selected = connectionManager.getConnection(connectionId);
    if (!selected) {
      vscode.window.showWarningMessage("The selected connection is no longer available.");
      return;
    }

    const confirmation = await vscode.window.showWarningMessage(
      `Delete connection '${selected.settings.name}'?`,
      { modal: true },
      "Delete"
    );
    if (confirmation !== "Delete") {
      return;
    }

    await connectionManager.deleteConnection(connectionId);
    await syncConnectionViews();
    vscode.window.showInformationMessage(`Deleted '${selected.settings.name}'.`);
  });

  const test = vscode.commands.registerCommand("vng.testConnection", async (input?: SchemaTreeItem | string) => {
    const selectedConnectionId = resolveConnectionId(input);
    const managedConnection = selectedConnectionId ? connectionManager.getConnection(selectedConnectionId) : null;
    const connection = managedConnection ? toRuntimeConnection(managedConnection) : await ensureConnection(context);
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

          if (managedConnection) {
            connectionManager.setConnectionStatus(managedConnection.id, failed.length === 0, {
              lastCheckedAt: Date.now(),
              reason: failed.length === 0 ? "connectivity_checks_ok" : "connectivity_checks_failed",
              detail: failed.length === 0 ? "all checks passed" : failed.map((f) => `${f.endpoint}:${f.status}`).join(", "),
            });
            await syncConnectionViews();
          }

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
          notifyConnectionFailure("Connectivity test", error);
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
    await vscode.commands.executeCommand("workbench.view.extension.vngExplorer");
    await vscode.commands.executeCommand("vngDatabaseExplorer.focus");
  });

  // Database Explorer commands
  const refreshSchema = vscode.commands.registerCommand("vng.refreshSchema", async (input?: SchemaTreeItem | string) => {
    try {
      const selectedConnectionId = resolveConnectionId(input);
      const targetConnection = selectedConnectionId
        ? connectionManager.getConnection(selectedConnectionId)
        : connectionManager.getActiveConnection();
      if (!targetConnection) {
        vscode.window.showWarningMessage("No connection active");
        return;
      }

      await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Window,
          title: "Refreshing database schema...",
        },
        async () => {
          schemaManager.invalidateCache(targetConnection.id);
          databaseExplorerProvider.refresh();
        }
      );

      vscode.window.showInformationMessage("Schema refreshed successfully");
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unknown error";
      vscode.window.showErrorMessage(`Failed to refresh schema: ${message}`);
    }
  });

  const filterConnections = vscode.commands.registerCommand("vng.filterConnections", async () => {
    const query = await vscode.window.showInputBox({
      title: "Filter Connections",
      prompt: "Filter by name, group, mode, or base URL",
      placeHolder: "Example: localmachine, tenant, 127.0.0.1",
      ignoreFocusOut: true,
    });
    if (query === undefined) {
      return;
    }

    const normalizedQuery = query.trim().toLowerCase();
    const candidates = connectionManager.listConnections().filter((connection) => {
      if (!normalizedQuery) {
        return true;
      }
      return [
        connection.settings.name,
        connection.settings.group ?? "",
        connection.settings.mode,
        connection.settings.baseUrl,
      ]
        .join(" ")
        .toLowerCase()
        .includes(normalizedQuery);
    });

    if (candidates.length === 0) {
      vscode.window.showInformationMessage("No connections matched the current filter.");
      return;
    }

    const pick = await vscode.window.showQuickPick(
      candidates.map((connection) => ({
        label: connection.settings.name,
        description: `${connection.settings.group ?? "localmachine"} • ${connection.settings.mode}`,
        detail: connection.settings.baseUrl,
        connectionId: connection.id,
      })),
      {
        title: "Filtered Connections",
        placeHolder: "Select a connection to activate",
      }
    );

    if (!pick) {
      return;
    }

    const active = await connectionManager.setActiveConnection(pick.connectionId);
    await syncConnectionViews();
    await verifyActiveProfileHealth({ silent: true });
    if (active) {
      vscode.window.showInformationMessage(`Active connection set to '${active.settings.name}'.`);
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

  const truncateTable = vscode.commands.registerCommand("vng.truncateTable", async (element: SchemaTreeItem) => {
    await handleTruncateTable(element);
  });

  const editTable = vscode.commands.registerCommand("vng.editTable", async (element?: SchemaTreeItem) => {
    if (!element) {
      await vscode.commands.executeCommand("vng.openTableEditorPicker");
      return;
    }
    await vscode.commands.executeCommand("vng.openTableEditor", element);
  });

  const openTableEditor = vscode.commands.registerCommand("vng.openTableEditor", async (element: SchemaTreeItem) => {
    if (element.type !== "table") {
      vscode.window.showWarningMessage("Select a table to open the inline editor.");
      return;
    }

    const activeConnection = connectionManager.getActiveConnection();
    if (!activeConnection) {
      vscode.window.showWarningMessage("Activate a managed connection before opening the table editor.");
      return;
    }

    const payload = element.data as { database: string; schema: string; table: { name: string } };
    const target: TableEditorTarget = {
      database: payload.database,
      schema: payload.schema,
      tableName: payload.table.name,
    };

    try {
      await openTableEditorForTarget(activeConnection, target);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      vscode.window.showErrorMessage(`Failed to open table editor: ${message}`);
    }
  });

  const openTableEditorPicker = vscode.commands.registerCommand("vng.openTableEditorPicker", async () => {
    const activeConnection = connectionManager.getActiveConnection();
    if (!activeConnection) {
      vscode.window.showWarningMessage("Activate a managed connection before opening the table editor.");
      return;
    }

    const registry = await schemaManager.getSchemaRegistry(activeConnection, false);
    const tablePicks = registry.databases.flatMap((database) =>
      database.schemas.flatMap((schema) =>
        schema.tables
          .filter((table) => !table.isSystem)
          .map((table) => ({
            label: table.name,
            description: schema.name,
            detail: database.name,
            target: {
              database: database.name,
              schema: schema.name,
              tableName: table.name,
            } as TableEditorTarget,
          }))
      )
    );

    if (tablePicks.length === 0) {
      vscode.window.showWarningMessage("No tables are available in the schema registry.");
      return;
    }

    const picked = await vscode.window.showQuickPick(tablePicks, {
      title: "Open Table Editor",
      placeHolder: "Choose table",
      canPickMany: false,
    });

    if (!picked) {
      return;
    }

    await openTableEditorForTarget(activeConnection, picked.target);
  });

  const createTableWizard = vscode.commands.registerCommand("vng.createTableWizard", async (element?: SchemaTreeItem) => {
    const activeConnection = connectionManager.getActiveConnection();
    if (!activeConnection) {
      vscode.window.showWarningMessage("Activate a managed connection before using schema wizard.");
      return;
    }

    await runCreateTableWizard(
      activeConnection,
      schemaManager,
      async (sql, operation) => {
        await executeManagedSqlWithProgress(activeConnection, sql, operation, { stopOnError: true });
        schemaManager.invalidateCache(activeConnection.id);
        databaseExplorerProvider.refresh();
      },
      element
    );
  });

  const alterTableWizard = vscode.commands.registerCommand("vng.alterTableWizard", async (element?: SchemaTreeItem) => {
    const activeConnection = connectionManager.getActiveConnection();
    if (!activeConnection) {
      vscode.window.showWarningMessage("Activate a managed connection before using schema wizard.");
      return;
    }

    await runAlterTableWizard(
      activeConnection,
      schemaManager,
      async (sql, operation) => {
        await executeManagedSqlWithProgress(activeConnection, sql, operation, { stopOnError: true });
        schemaManager.invalidateCache(activeConnection.id);
        databaseExplorerProvider.refresh();
      },
      element
    );
  });

  const tableEditorSave = vscode.commands.registerCommand("vng.tableEditor.save", async () => {
    await handleTableEditorMessage({ type: "save" });
  });

  const tableEditorAddRow = vscode.commands.registerCommand("vng.tableEditor.addRow", async () => {
    await handleTableEditorMessage({ type: "addRow" });
  });

  const tableEditorDiscard = vscode.commands.registerCommand("vng.tableEditor.discard", async () => {
    await handleTableEditorMessage({ type: "discard" });
  });

  const tableEditorRefresh = vscode.commands.registerCommand("vng.tableEditor.refresh", async () => {
    await handleTableEditorMessage({ type: "refresh" });
  });

  // Register settings panel command
  registerSettingsCommands(context, context.extensionUri);

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
    newConnection,
    editConnection,
    connectConnection,
    disconnectConnection,
    deleteConnectionCommand,
    copyConnectionHost,
    copyConnectionJson,
    showConnectionStatus,
    viewConnectionHistory,
    importConnectionKey,
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
    filterConnections,
    copyName,
    showTableDDL,
    showSQLTemplate,
    generateMockData,
    dumpTableStruct,
    dropTable,
    truncateTable,
    editTable,
    openTableEditor,
    openTableEditorPicker,
    createTableWizard,
    alterTableWizard,
    tableEditorSave,
    tableEditorAddRow,
    tableEditorDiscard,
    tableEditorRefresh,
    ...sqlDisposables,
    databaseView,
    queryHistoryView,
    connectionStatusBar,
    output
  );
}

export function deactivate(): void {
  // Clean up service resources if needed
  queryExecutionService.dispose();
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
    const migrated = await upsertManagedConnection(current);
    return toRuntimeConnection(migrated);
  }

  const choice = await vscode.window.showInformationMessage(
    "No VoltNueronGrid connection configured. Open the VoltNueronGrid sidebar → Database → Create New Connection.",
    "Create Connection"
  );
  if (choice === "Create Connection") {
    await vscode.commands.executeCommand("vng.newConnection");
  }
  return undefined;
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
