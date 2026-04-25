import * as vscode from "vscode";
import { WebviewConnectionDraft } from "./ConnectionManagerWebview";

export interface ConnectionEditorState {
  mode: "create" | "edit";
  draft: WebviewConnectionDraft;
}

export type ConnectionEditorMessage =
  | { type: "save"; draft: WebviewConnectionDraft }
  | { type: "test"; draft: WebviewConnectionDraft }
  | { type: "cancel" };

export function createConnectionEditorPanel(
  context: vscode.ExtensionContext,
  initialState: ConnectionEditorState,
  onMessage: (message: ConnectionEditorMessage) => Promise<void>
): {
  panel: vscode.WebviewPanel;
  updateState: (state: ConnectionEditorState) => Promise<void>;
} {
  const panel = vscode.window.createWebviewPanel(
    "vngConnectionEditor",
    initialState.mode === "create" ? "Connect to server" : "Edit connection",
    vscode.ViewColumn.Beside,
    {
      enableScripts: true,
      retainContextWhenHidden: true,
    }
  );

  panel.webview.html = getConnectionEditorHtml(initialState);

  panel.webview.onDidReceiveMessage(
    async (message: ConnectionEditorMessage) => {
      await onMessage(message);
    },
    undefined,
    context.subscriptions
  );

  return {
    panel,
    async updateState(state: ConnectionEditorState): Promise<void> {
      panel.title = state.mode === "create" ? "Connect to server" : "Edit connection";
      await panel.webview.postMessage({ type: "state", state });
    },
  };
}

function getConnectionEditorHtml(initialState: ConnectionEditorState): string {
  const stateJson = JSON.stringify(initialState).replace(/</g, "\\u003c");

  const html = `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Connect to server</title>
  <style>
    :root {
      color-scheme: light dark;
    }
    body {
      font-family: "Segoe UI", sans-serif;
      margin: 0;
      padding: 16px;
      background: var(--vscode-editor-background);
    }
    h1 {
      margin: 0 0 8px 0;
      font-size: 22px;
    }
    p {
      margin: 0 0 14px 0;
      opacity: 0.8;
      font-size: 12px;
    }
    .surface {
      border: 1px solid var(--vscode-panel-border);
      border-radius: 10px;
      padding: 16px;
      background: color-mix(in srgb, var(--vscode-editor-background) 92%, var(--vscode-sideBar-background));
    }
    .section {
      border-top: 1px solid var(--vscode-panel-border);
      margin-top: 14px;
      padding-top: 14px;
    }
    .section h2 {
      margin: 0 0 10px 0;
      font-size: 14px;
      font-weight: 600;
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
    .checkbox {
      display: flex;
      align-items: center;
      gap: 8px;
      min-height: 32px;
    }
    .hint, .inline-help {
      font-size: 12px;
      opacity: 0.8;
      margin-top: 10px;
    }
    .row {
      display: flex;
      gap: 8px;
      align-items: center;
      margin-top: 8px;
    }
    details {
      border: 1px solid var(--vscode-panel-border);
      border-radius: 8px;
      padding: 8px;
      margin-top: 8px;
    }
    summary {
      cursor: pointer;
      font-weight: 600;
      user-select: none;
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
    .test-result {
      margin-top: 8px;
      padding: 6px 10px;
      border-radius: 4px;
      font-size: 12px;
      display: none;
    }
    .test-result.ok {
      background: color-mix(in srgb, var(--vscode-testing-iconPassed, #73c991) 20%, transparent);
      border: 1px solid var(--vscode-testing-iconPassed, #73c991);
    }
    .test-result.fail {
      background: color-mix(in srgb, var(--vscode-testing-iconFailed, #f48771) 20%, transparent);
      border: 1px solid var(--vscode-testing-iconFailed, #f48771);
    }
    .modal-backdrop {
      display: none;
      position: fixed;
      inset: 0;
      background: rgba(0, 0, 0, 0.4);
      align-items: center;
      justify-content: center;
      z-index: 1000;
    }
    .modal {
      width: min(720px, calc(100vw - 32px));
      border: 1px solid var(--vscode-panel-border);
      border-radius: 10px;
      background: var(--vscode-editor-background);
      padding: 16px;
    }
    .modal h3 {
      margin: 0 0 10px 0;
    }
    .mode-tag {
      display: inline-block;
      margin-left: 6px;
      padding: 1px 8px;
      border-radius: 999px;
      border: 1px solid var(--vscode-panel-border);
      font-size: 11px;
      opacity: 0.9;
    }
    @media (max-width: 880px) {
      .grid {
        grid-template-columns: 1fr;
      }
    }
  </style>
</head>
<body>
  <div class="surface" role="main" aria-label="Connection editor">
    <h1 id="title" aria-live="polite"></h1>
    <p>Use a single form for create and edit. Credentials are stored securely in VS Code Secret Storage.</p>
    <div id="editor" role="form" aria-label="Connection details form"></div>
  </div>
  <div id="sslModalBackdrop" class="modal-backdrop" role="dialog" aria-modal="true" aria-label="SSL configuration dialog">
    <div class="modal">
      <h3>SSL / TLS paths</h3>
      <div class="grid">
        <label class="field full">
          <span>CA Path</span>
          <input id="modal-ssl-caPath" type="text" aria-label="SSL certificate authority path" />
        </label>
        <label class="field full">
          <span>Certificate Path</span>
          <input id="modal-ssl-certPath" type="text" aria-label="SSL certificate path" />
        </label>
        <label class="field full">
          <span>Key Path</span>
          <input id="modal-ssl-keyPath" type="text" aria-label="SSL private key path" />
        </label>
        <label class="checkbox field full">
          <input id="modal-ssl-rejectUnauthorized" type="checkbox" aria-label="Reject unauthorized certificates" />
          <span>Reject unauthorized certificates</span>
        </label>
      </div>
      <div class="actions">
        <button data-action="saveSslModal">Apply SSL Paths</button>
        <button class="secondary" data-action="cancelSslModal">Cancel</button>
      </div>
      <p class="hint">Only shown when SSL is enabled. Keep these paths empty if your runtime uses default trust.</p>
    </div>
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

    function byId(id) {
      return document.getElementById(id);
    }

    function readTextInput(id) {
      const element = byId(id);
      if (!(element instanceof HTMLInputElement)) {
        return "";
      }
      return element.value.trim();
    }

    function readOptionalTextInput(id) {
      const value = readTextInput(id);
      return value.length > 0 ? value : "";
    }

    function readCheckboxInput(id, fallback = false) {
      const element = byId(id);
      if (!(element instanceof HTMLInputElement)) {
        return fallback;
      }
      return element.checked;
    }

    function readSelectValue(id, fallback) {
      const element = byId(id);
      if (!(element instanceof HTMLSelectElement)) {
        return fallback;
      }
      return element.value;
    }

    function readDraftFromDom() {
      const mode = readSelectValue("draft-mode", "admin");
      const driverMode = readSelectValue("draft-driverMode", "http");
      const sslEnabled = readCheckboxInput("draft-ssl-enabled", false);
      return {
        id: state.draft ? state.draft.id : undefined,
        name: readTextInput("draft-name"),
        driverMode,
        baseUrl: readTextInput("draft-baseUrl"),
        mode,
        runtimeTarget: readSelectValue("draft-runtimeTarget", "custom"),
        adminKey: readOptionalTextInput("draft-adminKey"),
        operatorId: readOptionalTextInput("draft-operatorId"),
        tenantId: readOptionalTextInput("draft-tenantId"),
        userId: readOptionalTextInput("draft-userId"),
        ssl: {
          enabled: sslEnabled,
          caPath: (state.draft.ssl && state.draft.ssl.caPath) || "",
          certPath: (state.draft.ssl && state.draft.ssl.certPath) || "",
          keyPath: (state.draft.ssl && state.draft.ssl.keyPath) || "",
          rejectUnauthorized: (state.draft.ssl && state.draft.ssl.rejectUnauthorized) ?? true,
        },
        advanced: {
          connectionTimeout: readNumberInput("draft-advanced-connectionTimeout", 5000),
          idleTimeout: readNumberInput("draft-advanced-idleTimeout", 300000),
          keepAlive: readCheckboxInput("draft-advanced-keepAlive", true),
          maxConnections: readNumberInput("draft-advanced-maxConnections", 10),
        },
      };
    }

    function syncDraftFromDom() {
      state.draft = readDraftFromDom();
    }

    function openSslModal() {
      const backdrop = byId("sslModalBackdrop");
      if (!backdrop) {
        return;
      }
      byId("modal-ssl-caPath").value = state.draft.ssl?.caPath || "";
      byId("modal-ssl-certPath").value = state.draft.ssl?.certPath || "";
      byId("modal-ssl-keyPath").value = state.draft.ssl?.keyPath || "";
      byId("modal-ssl-rejectUnauthorized").checked = state.draft.ssl?.rejectUnauthorized ?? true;
      backdrop.style.display = "flex";
    }

    function closeSslModal() {
      const backdrop = byId("sslModalBackdrop");
      if (!backdrop) {
        return;
      }
      backdrop.style.display = "none";
    }

    function saveSslModal() {
      state.draft.ssl = {
        enabled: true,
        caPath: readOptionalTextInput("modal-ssl-caPath"),
        certPath: readOptionalTextInput("modal-ssl-certPath"),
        keyPath: readOptionalTextInput("modal-ssl-keyPath"),
        rejectUnauthorized: readCheckboxInput("modal-ssl-rejectUnauthorized", true),
      };
      closeSslModal();
      render();
    }

    function renderModeFields(draft) {
      if (draft.mode === "admin") {
        return '' +
          '<label class="field full">' +
            '<span>Admin Key</span>' +
            '<input id="draft-adminKey" type="password" aria-label="Admin API key" value="' + escapeHtml(draft.adminKey || "") + '" placeholder="' + (state.mode === "edit" ? "Leave blank to keep existing key on edit" : "Required for admin mode") + '" />' +
          '</label>';
      }

      if (draft.mode === "operator") {
        return '' +
          '<label class="field full">' +
            '<span>Admin Key</span>' +
            '<input id="draft-adminKey" type="password" aria-label="Admin API key" value="' + escapeHtml(draft.adminKey || "") + '" placeholder="' + (state.mode === "edit" ? "Leave blank to keep existing key on edit" : "Required for operator mode") + '" />' +
          '</label>' +
          '<label class="field full">' +
            '<span>Operator ID</span>' +
            '<input id="draft-operatorId" type="text" aria-label="Operator ID" value="' + escapeHtml(draft.operatorId || "") + '" placeholder="Required for operator mode" />' +
          '</label>';
      }

      return '' +
        '<label class="field full">' +
          '<span>Tenant ID</span>' +
          '<input id="draft-tenantId" type="text" aria-label="Tenant ID" value="' + escapeHtml(draft.tenantId || "") + '" placeholder="Required for tenant mode" />' +
        '</label>' +
        '<label class="field full">' +
          '<span>User ID (optional)</span>' +
          '<input id="draft-userId" type="text" aria-label="User ID" value="' + escapeHtml(draft.userId || "") + '" placeholder="Optional - use when tenant requests user-level audit headers" />' +
        '</label>' +
        '<div class="field full inline-help">User ID is optional. Leave it empty unless your tenant-level authorization policy requires x-vng-user-id.</div>';
    }

    function render() {
      const title = document.getElementById("title");
      const editor = document.getElementById("editor");
      const draft = state.draft;
      const sslEnabled = Boolean(draft.ssl && draft.ssl.enabled);
      title.textContent = state.mode === "create" ? "Connect to server" : "Edit connection";
      const modeLabel = draft.mode.charAt(0).toUpperCase() + draft.mode.slice(1);

      editor.innerHTML =
        '<div class="row"><strong>Connection mode</strong><span class="mode-tag">' + escapeHtml(modeLabel) + '</span></div>' +
        '<div class="grid">' +
          '<label class="field full">' +
            '<span>Name</span>' +
            '<input id="draft-name" type="text" aria-label="Connection name" value="' + escapeHtml(draft.name || "") + '" />' +
          '</label>' +
          '<label class="field full">' +
            '<span>Driver Mode</span>' +
            '<select id="draft-driverMode" aria-label="Driver mode">' +
              '<option value="http"' + ((draft.driverMode || "http") === "http" ? ' selected' : '') + '>http (REST)</option>' +
              '<option value="native"' + (draft.driverMode === "native" ? ' selected' : '') + '>native (vng:// socket)</option>' +
            '</select>' +
          '</label>' +
          '<label class="field full">' +
            '<span>Base URL</span>' +
            '<input id="draft-baseUrl" type="text" aria-label="Connection base URL" value="' + escapeHtml(draft.baseUrl || "") + '" placeholder="' + ((draft.driverMode || "http") === "native" ? "vng://127.0.0.1:7542" : "http://127.0.0.1:8080") + '" />' +
          '</label>' +
          '<label class="field">' +
            '<span>Mode</span>' +
            '<select id="draft-mode" aria-label="Connection mode">' +
              '<option value="admin"' + (draft.mode === "admin" ? ' selected' : '') + '>admin</option>' +
              '<option value="operator"' + (draft.mode === "operator" ? ' selected' : '') + '>operator</option>' +
              '<option value="tenant"' + (draft.mode === "tenant" ? ' selected' : '') + '>tenant</option>' +
            '</select>' +
          '</label>' +
          '<label class="field">' +
            '<span>Runtime Target</span>' +
            '<select id="draft-runtimeTarget" aria-label="Runtime target">' +
              '<option value="local"' + (draft.runtimeTarget === "local" ? ' selected' : '') + '>local</option>' +
              '<option value="docker"' + (draft.runtimeTarget === "docker" ? ' selected' : '') + '>docker</option>' +
              '<option value="cloud"' + (draft.runtimeTarget === "cloud" ? ' selected' : '') + '>cloud</option>' +
              '<option value="custom"' + (draft.runtimeTarget === "custom" ? ' selected' : '') + '>custom</option>' +
            '</select>' +
          '</label>' +
          renderModeFields(draft) +
        '</div>' +
        '<div class="section">' +
          '<h2>SSL / TLS</h2>' +
          '<label class="checkbox">' +
            '<input id="draft-ssl-enabled" type="checkbox" aria-label="Enable SSL or TLS metadata" ' + (sslEnabled ? 'checked' : '') + ' />' +
            '<span>Enable SSL / TLS metadata</span>' +
          '</label>' +
          '<div class="row">' +
            '<button class="secondary" data-action="openSslModal" ' + (sslEnabled ? '' : 'disabled') + '>Configure SSL paths</button>' +
            '<span class="inline-help">' + (sslEnabled ? 'CA/Certificate/Key paths are configured in the popup.' : 'Enable SSL first to configure certificate paths.') + '</span>' +
          '</div>' +
        '</div>' +
        '<div class="section">' +
          '<details>' +
            '<summary>Advanced Options</summary>' +
            '<div class="grid">' +
              '<label class="field">' +
                '<span>Connection Timeout (ms)</span>' +
                '<input id="draft-advanced-connectionTimeout" type="number" min="1" aria-label="Connection timeout milliseconds" value="' + escapeHtml(String(draft.advanced?.connectionTimeout ?? 5000)) + '" />' +
              '</label>' +
              '<label class="field">' +
                '<span>Idle Timeout (ms)</span>' +
                '<input id="draft-advanced-idleTimeout" type="number" min="1" aria-label="Idle timeout milliseconds" value="' + escapeHtml(String(draft.advanced?.idleTimeout ?? 300000)) + '" />' +
              '</label>' +
              '<label class="field">' +
                '<span>Max Connections</span>' +
                '<input id="draft-advanced-maxConnections" type="number" min="1" aria-label="Maximum connections" value="' + escapeHtml(String(draft.advanced?.maxConnections ?? 10)) + '" />' +
              '</label>' +
              '<label class="checkbox field">' +
                '<input id="draft-advanced-keepAlive" type="checkbox" aria-label="Enable keep alive" ' + ((draft.advanced?.keepAlive ?? true) ? 'checked' : '') + ' />' +
                '<span>Keep alive</span>' +
              '</label>' +
            '</div>' +
          '</details>' +
        '</div>' +
        '<div class="actions">' +
          '<button data-action="save" aria-label="Save connection profile">' + (state.mode === "create" ? 'Create Connection' : 'Save Changes') + '</button>' +
          '<button class="secondary" data-action="test" aria-label="Test connection">Test Connection</button>' +
          '<button class="secondary" data-action="cancel" aria-label="Cancel connection editing">Cancel</button>' +
        '</div>' +
        '<div id="test-result" class="test-result" role="status" aria-live="polite"></div>' +
        '<div class="hint">Admin mode: Admin Key required. Operator mode: Admin Key + Operator ID required. Tenant mode: Tenant ID required, User ID optional.</div>';
    }

    window.addEventListener("message", (event) => {
      if (event.data && event.data.type === "state") {
        state = event.data.state;
        render();
        return;
      }
      if (event.data && event.data.type === "testResult") {
        const resultEl = byId("test-result");
        if (resultEl) {
          resultEl.textContent = event.data.message;
          resultEl.className = "test-result " + (event.data.ok ? "ok" : "fail");
          resultEl.style.display = "block";
        }
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
      if (action === "openSslModal") {
        openSslModal();
        return;
      }
      if (action === "saveSslModal") {
        saveSslModal();
        return;
      }
      if (action === "cancelSslModal") {
        closeSslModal();
        return;
      }
      if (action === "save") {
        postMessage({ type: "save", draft: readDraftFromDom() });
        return;
      }
      if (action === "test") {
        const resultEl = byId("test-result");
        if (resultEl) {
          resultEl.textContent = "Testing connection…";
          resultEl.className = "test-result";
          resultEl.style.display = "block";
        }
        postMessage({ type: "test", draft: readDraftFromDom() });
        return;
      }
      postMessage({ type: action });
    });

    // Default credentials populated when the user picks a Mode.
    const MODE_DEFAULTS = {
      admin:    { adminKey: "local-dev-test", operatorId: "",        tenantId: "",        userId: "" },
      operator: { adminKey: "local-dev-test", operatorId: "operator-dev", tenantId: "",   userId: "" },
      tenant:   { adminKey: "",               operatorId: "",        tenantId: "tenant-dev", userId: "" },
    };
    const DRIVER_DEFAULTS = {
      http:   "http://127.0.0.1:8080",
      native: "vng://127.0.0.1:7542",
    };

    document.addEventListener("change", (event) => {
      const target = event.target;
      if (target instanceof HTMLSelectElement && target.id === "draft-mode") {
        syncDraftFromDom();
        state.draft.mode = target.value;
        // Auto-populate the auth field for the new mode if it's empty.
        const defaults = MODE_DEFAULTS[target.value] || MODE_DEFAULTS.admin;
        if (!state.draft.adminKey)   { state.draft.adminKey   = defaults.adminKey; }
        if (!state.draft.operatorId) { state.draft.operatorId = defaults.operatorId; }
        if (!state.draft.tenantId)   { state.draft.tenantId   = defaults.tenantId; }
        if (!state.draft.userId)     { state.draft.userId     = defaults.userId; }
        render();
        return;
      }
      if (target instanceof HTMLSelectElement && target.id === "draft-driverMode") {
        syncDraftFromDom();
        state.draft.driverMode = target.value;
        // Replace baseUrl with the driver-specific default when switching
        // unless the user has already typed a non-default custom URL.
        const oppositeDefault = DRIVER_DEFAULTS[target.value === "http" ? "native" : "http"];
        if (!state.draft.baseUrl || state.draft.baseUrl === oppositeDefault) {
          state.draft.baseUrl = DRIVER_DEFAULTS[target.value] || "";
        }
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