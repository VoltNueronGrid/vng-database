import * as vscode from "vscode";
import { AdvancedOptions, SSLConfig } from "../models";

export interface WebviewConnectionItem {
  id: string;
  name: string;
  mode: string;
  baseUrl: string;
  active: boolean;
  connected: boolean;
}

export interface ConnectionManagerState {
  connections: WebviewConnectionItem[];
}

export interface WebviewConnectionDraft {
  id?: string;
  name: string;
  baseUrl: string;
  mode: "admin" | "operator" | "tenant";
  runtimeTarget: "local" | "docker" | "cloud" | "custom";
  adminKey?: string;
  operatorId?: string;
  tenantId?: string;
  userId?: string;
  ssl: SSLConfig;
  advanced: AdvancedOptions;
}

export type ConnectionManagerMessage =
  | { type: "refresh" }
  | { type: "openCreate" }
  | { type: "openEdit"; id: string }
  | { type: "delete"; id: string }
  | { type: "test"; id: string }
  | { type: "activate"; id: string };

export function createConnectionManagerPanel(
  context: vscode.ExtensionContext,
  initialState: ConnectionManagerState,
  onMessage: (message: ConnectionManagerMessage) => Promise<void>
): {
  panel: vscode.WebviewPanel;
  updateState: (state: ConnectionManagerState) => Promise<void>;
} {
  const panel = vscode.window.createWebviewPanel(
    "vngConnectionManager",
    "VoltNueronGrid Connections",
    vscode.ViewColumn.One,
    {
      enableScripts: true,
      retainContextWhenHidden: true,
    }
  );

  panel.webview.html = getConnectionManagerHtml(panel.webview, initialState);

  panel.webview.onDidReceiveMessage(
    async (message: ConnectionManagerMessage) => {
      await onMessage(message);
    },
    undefined,
    context.subscriptions
  );

  return {
    panel,
    async updateState(state: ConnectionManagerState): Promise<void> {
      await panel.webview.postMessage({ type: "state", state });
    },
  };
}

function getConnectionManagerHtml(webview: vscode.Webview, initialState: ConnectionManagerState): string {
  const stateJson = JSON.stringify(initialState).replace(/</g, "\\u003c");

  const html = `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>VoltNueronGrid Connections</title>
  <style>
    :root {
      color-scheme: light dark;
    }
    body {
      font-family: "Segoe UI", Tahoma, Geneva, Verdana, sans-serif;
      margin: 0;
      padding: 16px;
    }
    .toolbar {
      display: flex;
      gap: 8px;
      margin-bottom: 12px;
    }
    button {
      border: 1px solid var(--vscode-button-border, transparent);
      background: var(--vscode-button-background);
      color: var(--vscode-button-foreground);
      padding: 6px 10px;
      border-radius: 4px;
      cursor: pointer;
    }
    button.secondary {
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
    }
    table {
      width: 100%;
      border-collapse: collapse;
    }
    th,
    td {
      border-bottom: 1px solid var(--vscode-panel-border);
      text-align: left;
      padding: 8px 6px;
      vertical-align: middle;
    }
    .badge {
      display: inline-block;
      padding: 2px 6px;
      border-radius: 999px;
      font-size: 11px;
    }
    .badge.active {
      background: #1b8f3a;
      color: #fff;
    }
    .badge.inactive {
      background: #6c6c6c;
      color: #fff;
    }
    .actions {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
    }
    .empty {
      padding: 16px;
      opacity: 0.8;
    }
    .card {
      border: 1px solid var(--vscode-panel-border);
      border-radius: 8px;
      padding: 10px;
    }
    .hint {
      opacity: 0.8;
      font-size: 12px;
    }
  </style>
</head>
<body>
  <div class="toolbar" role="toolbar" aria-label="Connection manager actions">
    <button data-action="openCreate" aria-label="Create a new connection profile">Create New Connection</button>
    <button class="secondary" data-action="refresh" aria-label="Refresh connection profile list">Refresh</button>
  </div>

  <div class="card" role="main" aria-label="VoltNueronGrid connection profiles">
    <div class="hint" style="margin-bottom: 12px;">Manage saved connection profiles here. Create and edit now open a dedicated panel beside the explorer to match the connection-centric UX.</div>
    <div id="content" role="region" aria-live="polite" aria-label="Connection profile list"></div>
  </div>

  <script>
    const vscode = acquireVsCodeApi();
    let state = __INITIAL_STATE__;

    function postMessage(message) {
      vscode.postMessage(message);
    }

    function render() {
      const content = document.getElementById("content");
      if (!state.connections || state.connections.length === 0) {
        content.innerHTML = '<div class="empty" role="status">No connections configured yet. Use Create New Connection to add a profile.</div>';
      } else {
        const rows = state.connections
          .map((connection) => {
            const statusClass = connection.active ? "active" : "inactive";
            const statusLabel = connection.active ? "Active" : "Inactive";
            return '<tr>' +
              '<td><strong>' + escapeHtml(connection.name) + '</strong></td>' +
              '<td>' + escapeHtml(connection.mode) + '</td>' +
              '<td>' + escapeHtml(connection.baseUrl) + '</td>' +
              '<td><span class="badge ' + statusClass + '" aria-label="Connection status ' + statusLabel + '">' + statusLabel + '</span></td>' +
              '<td>' + (connection.connected ? 'Connected' : 'Unknown') + '</td>' +
              '<td>' +
                '<div class="actions">' +
                  '<button class="secondary" data-action="activate" data-id="' + escapeHtml(connection.id) + '" aria-label="Switch active connection to ' + escapeHtml(connection.name) + '">Switch</button>' +
                  '<button class="secondary" data-action="test" data-id="' + escapeHtml(connection.id) + '" aria-label="Test connection ' + escapeHtml(connection.name) + '">Test</button>' +
                  '<button class="secondary" data-action="openEdit" data-id="' + escapeHtml(connection.id) + '" aria-label="Edit connection ' + escapeHtml(connection.name) + '">Edit</button>' +
                  '<button class="secondary" data-action="delete" data-id="' + escapeHtml(connection.id) + '" aria-label="Delete connection ' + escapeHtml(connection.name) + '">Delete</button>' +
                '</div>' +
              '</td>' +
            '</tr>';
          })
          .join("");

        content.innerHTML =
          '<table aria-label="Saved VoltNueronGrid connections">' +
            '<thead>' +
              '<tr>' +
                '<th scope="col">Name</th>' +
                '<th scope="col">Mode</th>' +
                '<th scope="col">Base URL</th>' +
                '<th scope="col">Status</th>' +
                '<th scope="col">Health</th>' +
                '<th scope="col">Actions</th>' +
              '</tr>' +
            '</thead>' +
            '<tbody>' + rows + '</tbody>' +
          '</table>';
      }
    }

    function escapeHtml(value) {
      return String(value)
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
    }

    window.addEventListener("message", (event) => {
      if (event.data && event.data.type === "state") {
        state = event.data.state;
        render();
      }
    });

    document.addEventListener("click", (event) => {
      const target = event.target;
      if (!(target instanceof HTMLButtonElement)) {
        return;
      }
      const action = target.dataset.action;
      if (!action) {
        return;
      }
      if (action === "save") {
        postMessage({ type: "save", draft: readDraftFromDom() });
        return;
      }
      const id = target.dataset.id;
      if (id) {
        postMessage({ type: action, id });
      } else {
        postMessage({ type: action });
      }
    });

    render();
  </script>
</body>
</html>`;

  return html.replace("__INITIAL_STATE__", stateJson);
}
