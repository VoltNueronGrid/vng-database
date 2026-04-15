import * as vscode from "vscode";
import { readConnection, runConnectionWizard } from "./config";
import { analyzeSql, executeSql, getSchemaRegistry, runConnectivityChecks, toPermissionMessage } from "./client";
import { RuntimeConnection } from "./config";
import { VngActionsProvider } from "./activityView";

export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel("VoltNueronGrid");
  const actionsProvider = new VngActionsProvider();
  const actionsView = vscode.window.createTreeView("vngActions", {
    treeDataProvider: actionsProvider,
    showCollapseAll: false,
  });

  const connect = vscode.commands.registerCommand("vng.connectWizard", async () => {
    const connection = await runConnectionWizard(context);
    if (!connection) {
      vscode.window.showInformationMessage("VoltNueronGrid connection wizard canceled.");
      return;
    }

    vscode.window.showInformationMessage(`Saved VoltNueronGrid connection for ${connection.settings.mode} mode.`);
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

    const response = await executeSql(connection, sql);
    await presentResponse("Query Runner", response.status, response.bodyText, connection, output);
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

  context.subscriptions.push(connect, test, queryRunner, diagnostics, schema, focusPanel, actionsView, output);
}

export function deactivate(): void {
  // No long-running resources to dispose.
}

async function ensureConnection(context: vscode.ExtensionContext): Promise<RuntimeConnection | undefined> {
  const current = await readConnection(context);
  if (current) {
    return current;
  }
  return runConnectionWizard(context);
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
