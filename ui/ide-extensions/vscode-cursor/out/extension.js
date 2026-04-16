"use strict";
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
exports.activate = activate;
exports.deactivate = deactivate;
exports.getServices = getServices;
const vscode = __importStar(require("vscode"));
const fs_1 = require("fs");
const config_1 = require("./config");
const client_1 = require("./client");
const activityView_1 = require("./activityView");
const models_1 = require("./models");
const services_1 = require("./services");
const providers_1 = require("./providers");
const commands_1 = require("./commands");
const sql_1 = require("./sql");
const ConnectionManagerWebview_1 = require("./ui/ConnectionManagerWebview");
const models_2 = require("./models");
const QueryResultsWebview_1 = require("./ui/QueryResultsWebview");
const TableEditorWebview_1 = require("./ui/TableEditorWebview");
// Global service instances
let connectionManager;
let httpClient;
let queryExecutionService;
let schemaManager;
let tableEditorService;
let databaseExplorerProvider;
let queryHistoryProvider;
async function activate(context) {
    const output = vscode.window.createOutputChannel("VoltNueronGrid");
    const connectionStatusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    connectionStatusBar.command = "vng.quickSwitchConnection";
    const updateConnectionStatusBar = () => {
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
    connectionManager = (0, services_1.createConnectionManager)(context);
    await connectionManager.initialize();
    httpClient = (0, services_1.createHttpClient)();
    queryExecutionService = (0, services_1.createQueryExecutionService)(httpClient, context);
    await queryExecutionService.initialize();
    schemaManager = (0, services_1.createSchemaManager)(httpClient);
    tableEditorService = (0, services_1.createTableEditorService)(httpClient, schemaManager);
    // Initialize database explorer
    databaseExplorerProvider = (0, providers_1.createDatabaseExplorerProvider)(schemaManager);
    queryHistoryProvider = (0, providers_1.createQueryHistoryProvider)(queryExecutionService);
    output.appendLine("[VoltNueronGrid] Extension activated (v0.2.0)");
    let latestQueryResult;
    let latestQueryResultState;
    let queryResultsPanel;
    let tableEditorState;
    let tableEditorPanel;
    const buildDefaultQueryResultState = () => ({
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
    const exportLatestQueryResult = async (format) => {
        if (!latestQueryResult) {
            vscode.window.showWarningMessage("No query result available to export.");
            return;
        }
        const defaultExtension = format === "csv" ? "csv" : "json";
        const uri = await vscode.window.showSaveDialog({
            saveLabel: `Export ${format.toUpperCase()}`,
            defaultUri: vscode.Uri.file(`vng-query-result-${Date.now()}.${defaultExtension}`),
            filters: format === "csv"
                ? { CSV: ["csv"], "All Files": ["*"] }
                : { JSON: ["json"], "All Files": ["*"] },
        });
        if (!uri) {
            return;
        }
        const payload = format === "csv" ? (0, models_2.exportAsCSV)(latestQueryResult) : (0, models_2.exportAsJSON)(latestQueryResult);
        await fs_1.promises.writeFile(uri.fsPath, payload, "utf8");
        vscode.window.showInformationMessage(`Exported query result to ${uri.fsPath}.`);
    };
    const ensureQueryResultsPanel = () => {
        if (queryResultsPanel) {
            return queryResultsPanel;
        }
        const initialState = latestQueryResultState ?? buildDefaultQueryResultState();
        queryResultsPanel = (0, QueryResultsWebview_1.createQueryResultsPanel)(context, initialState, async (message) => {
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
    const publishQueryResult = async (result, operation, connectionName) => {
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
    const syncTableEditorPanel = async () => {
        if (tableEditorPanel && tableEditorState) {
            await tableEditorPanel.updateState(tableEditorState);
        }
    };
    const resolveTableEditorConnection = () => connectionManager.getConnection(tableEditorState?.connectionId ?? "") ?? undefined;
    const handleTableEditorMessage = async (message) => {
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
                        session: await tableEditorService.openSession(connection, tableEditorState.session.target, tableEditorState.session.page, tableEditorState.session.pageSize, "Rows refreshed."),
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
        }
        catch (error) {
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
    const openTableEditorForTarget = async (activeConnection, target) => {
        tableEditorState = {
            connectionId: activeConnection.id,
            connectionName: activeConnection.settings.name,
            session: await tableEditorService.openSession(activeConnection, target),
        };
        const panel = ensureTableEditorPanel();
        await panel.updateState(tableEditorState);
        panel.reveal();
    };
    const ensureTableEditorPanel = () => {
        if (tableEditorPanel && tableEditorState) {
            return tableEditorPanel;
        }
        const initialState = tableEditorState ?? {
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
        tableEditorPanel = (0, TableEditorWebview_1.createTableEditorPanel)(context, initialState, handleTableEditorMessage);
        tableEditorPanel.panel.onDidDispose(() => {
            tableEditorPanel = undefined;
        });
        context.subscriptions.push(tableEditorPanel.panel);
        return tableEditorPanel;
    };
    // Create tree views
    const actionsProvider = new activityView_1.VngActionsProvider();
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
    }
    else {
        // Backward-compatible migration from legacy single-connection settings.
        const legacyConnection = await (0, config_1.readConnection)(context);
        if (legacyConnection) {
            const migrated = await upsertManagedConnection(legacyConnection);
            databaseExplorerProvider.setConnection(migrated);
            queryHistoryProvider.setActiveConnection(migrated.id);
        }
    }
    updateConnectionStatusBar();
    const connect = vscode.commands.registerCommand("vng.connectWizard", async () => {
        const connection = await (0, config_1.runConnectionWizard)(context);
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
        const pick = await vscode.window.showQuickPick(connections.map((connection) => ({
            label: connection.settings.name,
            description: `${connection.settings.mode}${connection.isActive ? " • active" : ""}`,
            detail: `${connection.settings.baseUrl}${connection.isConnected ? " • connected" : " • not verified"}`,
            connectionId: connection.id,
        })), {
            title: "Switch VoltNueronGrid Connection",
            placeHolder: "Select an active connection",
        });
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
        let editor;
        const defaultDraft = () => ({
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
        const draftFromConnection = (connection) => ({
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
        const buildWebviewState = () => ({
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
        const panel = (0, ConnectionManagerWebview_1.createConnectionManagerPanel)(context, buildWebviewState(), async (message) => {
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
                let parsedUrl;
                try {
                    parsedUrl = new URL(draft.baseUrl);
                    if (parsedUrl.protocol !== "http:" && parsedUrl.protocol !== "https:") {
                        throw new Error("Only http/https URLs are supported.");
                    }
                }
                catch {
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
                const connectionPatch = {
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
                const validationError = (0, models_1.validateConnectionSettings)(connectionPatch);
                if (validationError) {
                    vscode.window.showWarningMessage(validationError);
                    await panel.updateState(buildWebviewState());
                    return;
                }
                if (editor?.mode === "edit" && draft.id) {
                    await connectionManager.updateConnection(draft.id, connectionPatch);
                    vscode.window.showInformationMessage("Connection updated.");
                }
                else {
                    const created = await connectionManager.addConnection((0, models_1.createDefaultConnection)({
                        id: `conn-${Date.now()}`,
                        ...connectionPatch,
                        serverType: "voltnuerongrid",
                    }));
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
                const confirmation = await vscode.window.showWarningMessage(`Delete connection '${selected.settings.name}'?`, { modal: true }, "Delete");
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
                vscode.window.showInformationMessage(result.isHealthy ? `Connection test succeeded for '${selected.settings.name}'.` : `Connection test failed: ${result.message}`);
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
        await vscode.window.withProgress({
            location: vscode.ProgressLocation.Notification,
            title: "VoltNueronGrid: Testing connectivity",
        }, async () => {
            try {
                const checks = await (0, client_1.runConnectivityChecks)(connection);
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
            }
            catch (error) {
                const message = error instanceof Error ? error.message : "Unknown connectivity error";
                vscode.window.showErrorMessage(`Connectivity test failed: ${message}`);
            }
        });
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
        const confirmation = await vscode.window.showWarningMessage("Clear query history for the active connection?", { modal: true }, "Clear");
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
        const pick = await vscode.window.showQuickPick(matches.slice(0, 50).map((entry) => ({
            label: entry.query.replace(/\s+/g, " ").trim().slice(0, 120),
            description: `${entry.status} • ${entry.executionTime ?? 0} ms`,
            detail: new Date(entry.timestamp).toLocaleString(),
            entryId: entry.id,
        })), {
            title: "Query History Matches",
            placeHolder: "Select an entry to re-run",
        });
        if (!pick) {
            return;
        }
        await vscode.commands.executeCommand("vng.reRunHistoryQuery", pick.entryId);
    });
    const reRunHistoryQuery = vscode.commands.registerCommand("vng.reRunHistoryQuery", async (entryId) => {
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
        const response = await (0, client_1.analyzeSql)(connection, sql);
        await presentResponse("Query Diagnostics", response.status, response.bodyText, connection, output);
    });
    const schema = vscode.commands.registerCommand("vng.showSchemaRegistry", async () => {
        const connection = await ensureConnection(context);
        if (!connection) {
            return;
        }
        const response = await (0, client_1.getSchemaRegistry)(connection);
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
            await vscode.window.withProgress({
                location: vscode.ProgressLocation.Window,
                title: "Refreshing database schema...",
            }, async () => {
                schemaManager.invalidateCache(activeConnection.id);
                databaseExplorerProvider.refresh();
            });
            vscode.window.showInformationMessage("Schema refreshed successfully");
        }
        catch (error) {
            const message = error instanceof Error ? error.message : "Unknown error";
            vscode.window.showErrorMessage(`Failed to refresh schema: ${message}`);
        }
    });
    const copyName = vscode.commands.registerCommand("vng.copyName", async (element) => {
        await (0, commands_1.handleCopyName)(element);
    });
    const showTableDDL = vscode.commands.registerCommand("vng.showTableDDL", async (element) => {
        await (0, commands_1.handleShowDDL)(element);
    });
    const showSQLTemplate = vscode.commands.registerCommand("vng.showSQLTemplate", async (element) => {
        await (0, commands_1.handleSQLTemplate)(element);
    });
    const generateMockData = vscode.commands.registerCommand("vng.generateMockData", async (element) => {
        await (0, commands_1.handleGenerateMockData)(element);
    });
    const dumpTableStruct = vscode.commands.registerCommand("vng.dumpTableStruct", async (element) => {
        await (0, commands_1.handleDumpStruct)(element);
    });
    const dropTable = vscode.commands.registerCommand("vng.dropTable", async (element) => {
        await (0, commands_1.handleDropTable)(element);
    });
    const openTableEditor = vscode.commands.registerCommand("vng.openTableEditor", async (element) => {
        if (element.type !== "table") {
            vscode.window.showWarningMessage("Select a table to open the inline editor.");
            return;
        }
        const activeConnection = connectionManager.getActiveConnection();
        if (!activeConnection) {
            vscode.window.showWarningMessage("Activate a managed connection before opening the table editor.");
            return;
        }
        const payload = element.data;
        const target = {
            database: payload.database,
            schema: payload.schema,
            tableName: payload.table.name,
        };
        try {
            await openTableEditorForTarget(activeConnection, target);
        }
        catch (error) {
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
        const tablePicks = registry.databases.flatMap((database) => database.schemas.flatMap((schema) => schema.tables
            .filter((table) => !table.isSystem)
            .map((table) => ({
            label: table.name,
            description: schema.name,
            detail: database.name,
            target: {
                database: database.name,
                schema: schema.name,
                tableName: table.name,
            },
        }))));
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
    const createTableWizard = vscode.commands.registerCommand("vng.createTableWizard", async (element) => {
        const activeConnection = connectionManager.getActiveConnection();
        if (!activeConnection) {
            vscode.window.showWarningMessage("Activate a managed connection before using schema wizard.");
            return;
        }
        await (0, commands_1.runCreateTableWizard)(activeConnection, schemaManager, async (sql, operation) => {
            await executeManagedSqlWithProgress(activeConnection, sql, operation, { stopOnError: true });
            schemaManager.invalidateCache(activeConnection.id);
            databaseExplorerProvider.refresh();
        }, element);
    });
    const alterTableWizard = vscode.commands.registerCommand("vng.alterTableWizard", async (element) => {
        const activeConnection = connectionManager.getActiveConnection();
        if (!activeConnection) {
            vscode.window.showWarningMessage("Activate a managed connection before using schema wizard.");
            return;
        }
        await (0, commands_1.runAlterTableWizard)(activeConnection, schemaManager, async (sql, operation) => {
            await executeManagedSqlWithProgress(activeConnection, sql, operation, { stopOnError: true });
            schemaManager.invalidateCache(activeConnection.id);
            databaseExplorerProvider.refresh();
        }, element);
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
    (0, commands_1.registerSettingsCommands)(context, context.extensionUri);
    const sqlDisposables = (0, sql_1.registerSqlEditorFeatures)({
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
    const executeManagedSqlWithProgress = async (connection, sql, operation, streamOptions) => {
        const statements = queryExecutionService.parseStatements(sql);
        const executionId = `exec-${Date.now()}`;
        const timeoutMs = connection.settings.advanced.connectionTimeout ?? 30000;
        await vscode.window.withProgress({
            location: vscode.ProgressLocation.Notification,
            title: `VoltNueronGrid: ${operation}`,
            cancellable: true,
        }, async (_progress, cancellationToken) => {
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
        });
    };
    context.subscriptions.push(connect, manageConnections, quickSwitchConnection, test, queryRunner, showQueryResults, refreshQueryHistory, clearQueryHistory, searchQueryHistory, reRunHistoryQuery, cancelActiveQuery, diagnostics, schema, focusPanel, refreshSchema, copyName, showTableDDL, showSQLTemplate, generateMockData, dumpTableStruct, dropTable, openTableEditor, openTableEditorPicker, createTableWizard, alterTableWizard, tableEditorSave, tableEditorAddRow, tableEditorDiscard, tableEditorRefresh, ...sqlDisposables, actionsView, databaseView, queryHistoryView, connectionStatusBar, output);
}
function deactivate() {
    // Clean up service resources if needed
    schemaManager.clearCache();
    schemaManager.dispose();
}
async function ensureConnection(context) {
    const activeManagedConnection = connectionManager.getActiveConnection();
    if (activeManagedConnection) {
        return toRuntimeConnection(activeManagedConnection);
    }
    const current = await (0, config_1.readConnection)(context);
    if (current) {
        await upsertManagedConnection(current);
        return current;
    }
    const configured = await (0, config_1.runConnectionWizard)(context);
    if (configured) {
        await upsertManagedConnection(configured);
    }
    return configured;
}
async function presentResponse(operation, status, bodyText, connection, output) {
    output.appendLine(`[${operation}] HTTP ${status}`);
    output.appendLine(bodyText || "(empty response)");
    output.appendLine("---");
    output.show(true);
    if (status === 200) {
        vscode.window.showInformationMessage(`${operation} completed successfully.`);
        return;
    }
    const message = (0, client_1.toPermissionMessage)(status, connection.settings.mode);
    if (message) {
        vscode.window.showWarningMessage(`${operation}: ${message}`);
        return;
    }
    vscode.window.showErrorMessage(`${operation} failed with HTTP ${status}.`);
}
/**
 * Get global service instances (for use by other modules)
 */
function getServices() {
    return {
        connectionManager,
        httpClient,
        queryExecutionService,
        schemaManager,
    };
}
async function upsertManagedConnection(runtimeConnection) {
    const managedSettings = toManagedSettings(runtimeConnection);
    const existing = connectionManager
        .listConnections()
        .find((candidate) => candidate.settings.baseUrl === managedSettings.baseUrl &&
        candidate.settings.mode === managedSettings.mode &&
        candidate.settings.operatorId === managedSettings.operatorId &&
        candidate.settings.tenantId === managedSettings.tenantId &&
        candidate.settings.userId === managedSettings.userId);
    if (existing) {
        const updated = await connectionManager.updateConnection(existing.id, managedSettings);
        const active = await connectionManager.setActiveConnection(existing.id);
        return active ?? updated ?? existing;
    }
    const created = await connectionManager.addConnection(managedSettings);
    const active = await connectionManager.setActiveConnection(created.id);
    return active ?? created;
}
function toManagedSettings(runtimeConnection) {
    const url = new URL(runtimeConnection.settings.baseUrl);
    const host = url.hostname;
    const port = url.port ? Number(url.port) : url.protocol === "https:" ? 443 : 80;
    const baseName = `${runtimeConnection.settings.mode}-${host}:${port}`;
    return (0, models_1.createDefaultConnection)({
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
function toRuntimeConnection(connection) {
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
//# sourceMappingURL=extension.js.map