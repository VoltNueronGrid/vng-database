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
exports.createConnectionEditorPanel = createConnectionEditorPanel;
const vscode = __importStar(require("vscode"));
function createConnectionEditorPanel(context, initialState, onMessage) {
    const panel = vscode.window.createWebviewPanel("vngConnectionEditor", initialState.mode === "create" ? "Create VoltNueronGrid Connection" : "Edit VoltNueronGrid Connection", vscode.ViewColumn.Beside, {
        enableScripts: true,
        retainContextWhenHidden: true,
    });
    panel.webview.html = getConnectionEditorHtml(initialState);
    panel.webview.onDidReceiveMessage(async (message) => {
        await onMessage(message);
    }, undefined, context.subscriptions);
    return {
        panel,
        async updateState(state) {
            panel.title = state.mode === "create" ? "Create VoltNueronGrid Connection" : "Edit VoltNueronGrid Connection";
            await panel.webview.postMessage({ type: "state", state });
        },
    };
}
function getConnectionEditorHtml(initialState) {
    const stateJson = JSON.stringify(initialState).replace(/</g, "\\u003c");
    const html = `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>VoltNueronGrid Connection Editor</title>
  <style>
    :root {
      color-scheme: light dark;
    }
    body {
      font-family: "Segoe UI", Tahoma, Geneva, Verdana, sans-serif;
      margin: 0;
      padding: 18px;
    }
    h2 {
      margin: 0 0 8px 0;
      font-size: 18px;
    }
    p.lede {
      margin: 0 0 14px 0;
      opacity: 0.8;
      font-size: 12px;
    }
    .card {
      border: 1px solid var(--vscode-panel-border);
      border-radius: 8px;
      padding: 14px;
      background: color-mix(in srgb, var(--vscode-editor-background) 88%, var(--vscode-sideBar-background));
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
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
      padding: 7px 9px;
      font: inherit;
    }
    .section {
      border-top: 1px solid var(--vscode-panel-border);
      margin-top: 14px;
      padding-top: 14px;
    }
    .section h3 {
      margin: 0 0 10px 0;
      font-size: 13px;
    }
    .checkbox {
      display: flex;
      align-items: center;
      gap: 8px;
      min-height: 32px;
    }
    .hint {
      font-size: 12px;
      opacity: 0.8;
      margin-top: 10px;
    }
    .actions {
      display: flex;
      gap: 8px;
      margin-top: 14px;
    }
    button {
      border: 1px solid var(--vscode-button-border, transparent);
      background: var(--vscode-button-background);
      color: var(--vscode-button-foreground);
      padding: 7px 12px;
      border-radius: 4px;
      cursor: pointer;
    }
    button.secondary {
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
    }
    @media (max-width: 880px) {
      .grid {
        grid-template-columns: 1fr;
      }
    }
  </style>
</head>
<body>
  <div class="card">
    <h2 id="title"></h2>
    <p class="lede">Use the same rich form for create and edit flows. Save persists the profile and keeps secrets in VS Code secret storage.</p>
    <div id="editor"></div>
  </div>
  <script>
    const vscode = acquireVsCodeApi();
    let state = __INITIAL_STATE__;

    function postMessage(message) {
      vscode.postMessage(message);
    }

    function readNumberInput(id, fallbackValue) {
      const value = document.getElementById(id).value.trim();
      if (!value) {
        return fallbackValue;
      }
      const parsed = Number(value);
      return Number.isFinite(parsed) ? parsed : fallbackValue;
    }

    function escapeHtml(value) {
      return String(value)
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
    }

    function readDraftFromDom() {
      const mode = document.getElementById("draft-mode").value;
      const sslEnabled = document.getElementById("draft-ssl-enabled").checked;
      return {
        id: state.draft ? state.draft.id : undefined,
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
      state.draft = readDraftFromDom();
    }

    function render() {
      const title = document.getElementById("title");
      const editor = document.getElementById("editor");
      const draft = state.draft;
      const isOperator = draft.mode === "operator";
      const isTenant = draft.mode === "tenant";
      const sslEnabled = !!(draft.ssl && draft.ssl.enabled);
      title.textContent = state.mode === "create" ? "Create Connection" : "Edit Connection";

      editor.innerHTML =
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
          '<h3>SSL / TLS</h3>' +
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
        '</div>' +
        '<div class="section">' +
          '<h3>Advanced Options</h3>' +
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
        '<div class="actions">' +
          '<button data-action="save">' + (state.mode === "create" ? 'Create Connection' : 'Save Changes') + '</button>' +
          '<button class="secondary" data-action="cancel">Cancel</button>' +
        '</div>' +
        '<div class="hint">Operator mode requires Admin Key plus Operator ID. Tenant mode requires Tenant ID and User ID.</div>';
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
      postMessage({ type: action });
    });

    document.addEventListener("change", (event) => {
      const target = event.target;
      if (target instanceof HTMLSelectElement && target.id === "draft-mode") {
        syncDraftFromDom();
        state.draft.mode = target.value;
        render();
        return;
      }
      if (target instanceof HTMLInputElement && target.id === "draft-ssl-enabled") {
        syncDraftFromDom();
        state.draft.ssl.enabled = target.checked;
        render();
      }
    });

    render();
  </script>
</body>
</html>`;
    return html.replace("__INITIAL_STATE__", stateJson);
}
//# sourceMappingURL=ConnectionEditorWebview.js.map