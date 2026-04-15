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
  editor?: {
    mode: "create" | "edit";
    draft: WebviewConnectionDraft;
  };
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
  | { type: "cancelEdit" }
  | { type: "save"; draft: WebviewConnectionDraft }
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
    .layout {
      display: grid;
      grid-template-columns: minmax(0, 2fr) minmax(320px, 1fr);
      gap: 14px;
      align-items: start;
    }
    .card {
      border: 1px solid var(--vscode-panel-border);
      border-radius: 8px;
      padding: 10px;
    }
    .editor {
      display: none;
    }
    .editor.visible {
      display: block;
    }
    .editor h3 {
      margin: 0 0 10px 0;
      font-size: 14px;
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 8px;
    }
    .field {
      display: flex;
      flex-direction: column;
      gap: 4px;
    }
    .field.full {
      grid-column: 1 / -1;
    }
    .field input,
    .field select {
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
      border: 1px solid var(--vscode-input-border, var(--vscode-panel-border));
      border-radius: 4px;
      padding: 6px 8px;
      font: inherit;
    }
    .editor-actions {
      display: flex;
      gap: 8px;
      margin-top: 10px;
    }
    .section {
      border-top: 1px solid var(--vscode-panel-border);
      margin-top: 12px;
      padding-top: 12px;
    }
    .section h4 {
      margin: 0 0 8px 0;
      font-size: 13px;
    }
    .checkbox {
      display: flex;
      align-items: center;
      gap: 8px;
      min-height: 32px;
    }
    .checkbox input {
      width: auto;
      margin: 0;
    }
    .hint {
      opacity: 0.8;
      font-size: 12px;
    }
    @media (max-width: 960px) {
      .layout {
        grid-template-columns: 1fr;
      }
      .grid {
        grid-template-columns: 1fr;
      }
    }
  </style>
</head>
<body>
  <div class="toolbar">
    <button data-action="openCreate">Add Connection</button>
    <button class="secondary" data-action="refresh">Refresh</button>
  </div>

  <div class="layout">
    <div class="card">
      <div id="content"></div>
    </div>

    <div id="editor" class="card editor"></div>
  </div>

  <script>
    const vscode = acquireVsCodeApi();
    let state = __INITIAL_STATE__;

    function postMessage(message) {
      vscode.postMessage(message);
    }

    function render() {
      const content = document.getElementById("content");
      const editor = document.getElementById("editor");
      if (!state.connections || state.connections.length === 0) {
        content.innerHTML = '<div class="empty">No connections configured yet.</div>';
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

      renderEditor(editor);
    }

    function renderEditor(editor) {
      if (!state.editor) {
        editor.classList.remove("visible");
        editor.innerHTML = '<h3>Connection Editor</h3><div class="hint">Click Add Connection or Edit to configure a profile.</div>';
        return;
      }

      const draft = state.editor.draft;
      const mode = state.editor.mode;
      const isOperator = draft.mode === "operator";
      const isTenant = draft.mode === "tenant";
      const sslEnabled = !!(draft.ssl && draft.ssl.enabled);
      const title = mode === "create" ? "Create Connection" : "Edit Connection";
      const saveLabel = mode === "create" ? "Create" : "Save";

      editor.classList.add("visible");
      editor.innerHTML =
        '<h3>' + title + '</h3>' +
        '<div class="grid">' +
          '<label class="field full">' +
            '<span>Name</span>' +
            '<input id="draft-name" type="text" value="' + escapeHtml(draft.name || "") + '" />' +
          '</label>' +
          '<label class="field full">' +
            '<span>Base URL</span>' +
            '<input id="draft-baseUrl" type="text" value="' + escapeHtml(draft.baseUrl || "") + '" />' +
          '</label>' +
          '<label class="field">' +
            '<span>Mode</span>' +
            '<select id="draft-mode">' +
              '<option value="admin"' + (draft.mode === "admin" ? ' selected' : '') + '>admin</option>' +
              '<option value="operator"' + (draft.mode === "operator" ? ' selected' : '') + '>operator</option>' +
              '<option value="tenant"' + (draft.mode === "tenant" ? ' selected' : '') + '>tenant</option>' +
            '</select>' +
          '</label>' +
          '<label class="field">' +
            '<span>Runtime Target</span>' +
            '<select id="draft-runtimeTarget">' +
              '<option value="local"' + (draft.runtimeTarget === "local" ? ' selected' : '') + '>local</option>' +
              '<option value="docker"' + (draft.runtimeTarget === "docker" ? ' selected' : '') + '>docker</option>' +
              '<option value="cloud"' + (draft.runtimeTarget === "cloud" ? ' selected' : '') + '>cloud</option>' +
              '<option value="custom"' + (draft.runtimeTarget === "custom" ? ' selected' : '') + '>custom</option>' +
            '</select>' +
          '</label>' +
          '<label class="field full">' +
            '<span>Admin Key</span>' +
            '<input id="draft-adminKey" type="password" value="' + escapeHtml(draft.adminKey || "") + '" placeholder="Leave blank to keep existing key on edit" />' +
          '</label>' +
          '<label class="field' + (isOperator ? '' : ' full') + '">' +
            '<span>Operator ID</span>' +
            '<input id="draft-operatorId" type="text" value="' + escapeHtml(draft.operatorId || "") + '" ' + (isOperator ? '' : 'disabled') + ' />' +
          '</label>' +
          '<label class="field' + (isTenant ? '' : ' full') + '">' +
            '<span>Tenant ID</span>' +
            '<input id="draft-tenantId" type="text" value="' + escapeHtml(draft.tenantId || "") + '" ' + (isTenant ? '' : 'disabled') + ' />' +
          '</label>' +
          '<label class="field' + (isTenant ? '' : ' full') + '">' +
            '<span>User ID</span>' +
            '<input id="draft-userId" type="text" value="' + escapeHtml(draft.userId || "") + '" ' + (isTenant ? '' : 'disabled') + ' />' +
          '</label>' +
        '</div>' +
        '<div class="section">' +
          '<h4>SSL / TLS</h4>' +
          '<label class="checkbox">' +
            '<input id="draft-ssl-enabled" type="checkbox" ' + (sslEnabled ? 'checked' : '') + ' />' +
            '<span>Enable SSL / TLS metadata</span>' +
          '</label>' +
          '<div class="grid">' +
            '<label class="field full">' +
              '<span>CA Path</span>' +
              '<input id="draft-ssl-caPath" type="text" value="' + escapeHtml(draft.ssl?.caPath || "") + '" ' + (sslEnabled ? '' : 'disabled') + ' />' +
            '</label>' +
            '<label class="field full">' +
              '<span>Certificate Path</span>' +
              '<input id="draft-ssl-certPath" type="text" value="' + escapeHtml(draft.ssl?.certPath || "") + '" ' + (sslEnabled ? '' : 'disabled') + ' />' +
            '</label>' +
            '<label class="field full">' +
              '<span>Key Path</span>' +
              '<input id="draft-ssl-keyPath" type="text" value="' + escapeHtml(draft.ssl?.keyPath || "") + '" ' + (sslEnabled ? '' : 'disabled') + ' />' +
            '</label>' +
            '<label class="checkbox field full">' +
              '<input id="draft-ssl-rejectUnauthorized" type="checkbox" ' + ((draft.ssl?.rejectUnauthorized ?? true) ? 'checked' : '') + ' ' + (sslEnabled ? '' : 'disabled') + ' />' +
              '<span>Reject unauthorized certificates</span>' +
            '</label>' +
          '</div>' +
          '<div class="hint">VS Code webviews use browser networking, so HTTPS validation is handled by the platform. These TLS fields are stored with the profile for future runtime integrations.</div>' +
        '</div>' +
        '<div class="section">' +
          '<h4>Advanced Options</h4>' +
          '<div class="grid">' +
            '<label class="field">' +
              '<span>Connection Timeout (ms)</span>' +
              '<input id="draft-advanced-connectionTimeout" type="number" min="1" value="' + escapeHtml(String(draft.advanced?.connectionTimeout ?? 5000)) + '" />' +
            '</label>' +
            '<label class="field">' +
              '<span>Idle Timeout (ms)</span>' +
              '<input id="draft-advanced-idleTimeout" type="number" min="1" value="' + escapeHtml(String(draft.advanced?.idleTimeout ?? 300000)) + '" />' +
            '</label>' +
            '<label class="field">' +
              '<span>Max Connections</span>' +
              '<input id="draft-advanced-maxConnections" type="number" min="1" value="' + escapeHtml(String(draft.advanced?.maxConnections ?? 10)) + '" />' +
            '</label>' +
            '<label class="checkbox field">' +
              '<input id="draft-advanced-keepAlive" type="checkbox" ' + ((draft.advanced?.keepAlive ?? true) ? 'checked' : '') + ' />' +
              '<span>Keep alive</span>' +
            '</label>' +
          '</div>' +
        '</div>' +
        '<div class="editor-actions">' +
          '<button data-action="save">' + saveLabel + '</button>' +
          '<button class="secondary" data-action="cancelEdit">Cancel</button>' +
        '</div>' +
        '<div class="hint">Mode requirements: operator needs Operator ID and Admin Key. Tenant needs Tenant ID and User ID.</div>';
    }

    function readNumberInput(id, fallbackValue) {
      const value = document.getElementById(id).value.trim();
      if (!value) {
        return fallbackValue;
      }
      const parsed = Number(value);
      return Number.isFinite(parsed) ? parsed : fallbackValue;
    }

    function readDraftFromDom() {
      const mode = document.getElementById("draft-mode").value;
      const sslEnabled = document.getElementById("draft-ssl-enabled").checked;
      return {
        id: state.editor && state.editor.draft ? state.editor.draft.id : undefined,
        name: document.getElementById("draft-name").value.trim(),
        baseUrl: document.getElementById("draft-baseUrl").value.trim(),
        mode,
        runtimeTarget: document.getElementById("draft-runtimeTarget").value,
        adminKey: document.getElementById("draft-adminKey").value,
        operatorId: document.getElementById("draft-operatorId").value.trim(),
        tenantId: document.getElementById("draft-tenantId").value.trim(),
        userId: document.getElementById("draft-userId").value.trim(),
        ssl: {
          enabled: sslEnabled,
          caPath: document.getElementById("draft-ssl-caPath").value.trim(),
          certPath: document.getElementById("draft-ssl-certPath").value.trim(),
          keyPath: document.getElementById("draft-ssl-keyPath").value.trim(),
          rejectUnauthorized: document.getElementById("draft-ssl-rejectUnauthorized").checked,
        },
        advanced: {
          connectionTimeout: readNumberInput("draft-advanced-connectionTimeout", 5000),
          idleTimeout: readNumberInput("draft-advanced-idleTimeout", 300000),
          keepAlive: document.getElementById("draft-advanced-keepAlive").checked,
          maxConnections: readNumberInput("draft-advanced-maxConnections", 10),
        },
      };
    }

    function syncDraftFromDom() {
      if (!state.editor) {
        return;
      }
      state.editor.draft = readDraftFromDom();
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

    document.addEventListener("change", (event) => {
      const target = event.target;
      if (!state.editor) {
        return;
      }
      if (target instanceof HTMLSelectElement && target.id === "draft-mode") {
        syncDraftFromDom();
        state.editor.draft.mode = target.value;
        render();
        return;
      }
      if (target instanceof HTMLInputElement && target.id === "draft-ssl-enabled") {
        syncDraftFromDom();
        state.editor.draft.ssl.enabled = target.checked;
        render();
      }
    });

    render();
  </script>
</body>
</html>`;

  return html.replace("__INITIAL_STATE__", stateJson);
}
