import * as vscode from "vscode";
import { SettingsPanel } from "../ui/SettingsWebview";

/**
 * Opens the VoltNueronGrid Settings panel.
 * Reuses existing panel if already open.
 */
export async function openSettingsPanel(extensionUri: vscode.Uri) {
  SettingsPanel.createOrShow(extensionUri);
}

/**
 * Command handler to bind to the extension.
 */
export function registerSettingsCommands(
  context: vscode.ExtensionContext,
  extensionUri: vscode.Uri
) {
  context.subscriptions.push(
    vscode.commands.registerCommand("vng.openSettings", async () => {
      openSettingsPanel(extensionUri);
    })
  );
}
