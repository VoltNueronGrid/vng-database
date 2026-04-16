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
exports.createConnectionManagerPanel = createConnectionManagerPanel;
const vscode = __importStar(require("vscode"));
function createConnectionManagerPanel(context, initialState, onMessage) {
    const panel = vscode.window.createWebviewPanel("vngConnectionManager", "VoltNueronGrid Connections", vscode.ViewColumn.One, {
        enableScripts: true,
        retainContextWhenHidden: true,
    });
    panel.webview.html = getConnectionManagerHtml(panel.webview, initialState);
    panel.webview.onDidReceiveMessage(async (message) => {
        await onMessage(message);
    }, undefined, context.subscriptions);
    return {
        panel,
        async updateState(state) {
            await panel.webview.postMessage({ type: "state", state });
        },
    };
}
function getConnectionManagerHtml(webview, initialState) {
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
  <div class="toolbar">
    <button data-action="openCreate">Create New Connection</button>
    <button class="secondary" data-action="refresh">Refresh</button>
  </div>

  <div class="card">
    <div class="hint" style="margin-bottom: 12px;">Manage saved connection profiles here. Create and edit now open a dedicated panel beside the explorer to match the connection-centric UX.</div>
    <div id="content"></div>
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
        content.innerHTML = '<div class="empty">No connections configured yet. Use Create New Connection to add a profile.</div>';
      } else {
        const rows = state.connections
          .map((connection) => {
            const statusClass = connection.active ? "active" : "inactive";
            const statusLabel = connection.active ? "Active" : "Inactive";
            return '<tr>' +
              '<td><strong>' + escapeHtml(connection.name) + '</strong></td>' +
              '<td>' + escapeHtml(connection.mode) + '</td>' +
              '<td>' + escapeHtml(connection.baseUrl) + '</td>' +
              '<td><span class="badge ' + statusClass + '">' + statusLabel + '</span></td>' +
              '<td>' + (connection.connected ? 'Connected' : 'Unknown') + '</td>' +
              '<td>' +
                '<div class="actions">' +
                  '<button class="secondary" data-action="activate" data-id="' + escapeHtml(connection.id) + '">Switch</button>' +
                  '<button class="secondary" data-action="test" data-id="' + escapeHtml(connection.id) + '">Test</button>' +
                  '<button class="secondary" data-action="openEdit" data-id="' + escapeHtml(connection.id) + '">Edit</button>' +
                  '<button class="secondary" data-action="delete" data-id="' + escapeHtml(connection.id) + '">Delete</button>' +
                '</div>' +
              '</td>' +
            '</tr>';
          })
          .join("");

        content.innerHTML =
          '<table>' +
            '<thead>' +
              '<tr>' +
                '<th>Name</th>' +
                '<th>Mode</th>' +
                '<th>Base URL</th>' +
                '<th>Status</th>' +
                '<th>Health</th>' +
                '<th>Actions</th>' +
              '</tr>' +
            '</thead>' +
            '<tbody>' + rows + '</tbody>' +
          '</table>';
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
//# sourceMappingURL=ConnectionManagerWebview.js.map