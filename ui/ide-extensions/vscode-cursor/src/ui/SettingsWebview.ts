import * as vscode from "vscode";

export interface SettingsData {
  editor: {
    fontSize: number;
    fontFamily: string;
    wordWrap: boolean;
  };
  sql: {
    runOnSave: "never" | "prompt" | "always";
    runOnOpen: "never" | "prompt" | "always";
    timeout: number;
    lineLength: number;
  };
  results: {
    rowsPerPage: number;
    autoRefresh: boolean;
    autoRefreshIntervalSeconds: number;
    exportFormat: "csv" | "json";
  };
  connection: {
    cacheSchemaEnabled: boolean;
    cacheTTLSeconds: number;
    retryAttempts: number;
    retryDelayMs: number;
  };
}

export class SettingsPanel {
  public static currentPanel: SettingsPanel | undefined;
  private readonly panel: vscode.WebviewPanel;
  private disposables: vscode.Disposable[] = [];

  public static createOrShow(extensionUri: vscode.Uri) {
    if (SettingsPanel.currentPanel) {
      SettingsPanel.currentPanel.panel.reveal(vscode.ViewColumn.Beside);
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      "vngSettings",
      "VoltNueronGrid Settings",
      vscode.ViewColumn.Beside,
      {
        enableScripts: true,
        retainContextWhenHidden: false,
      }
    );

    SettingsPanel.currentPanel = new SettingsPanel(panel, extensionUri);
  }

  private constructor(
    panel: vscode.WebviewPanel,
    extensionUri: vscode.Uri
  ) {
    this.panel = panel;

    this.panel.onDidDispose(() => this.dispose(), null, this.disposables);
    this.panel.webview.onDidReceiveMessage(
      (message) => this.handleWebviewMessage(message),
      null,
      this.disposables
    );

    this.updateWebview();
  }

  public updateWebview() {
    const config = vscode.workspace.getConfiguration("voltnuerongrid");
    const settings = this.readSettings(config);
    this.panel.webview.html = this.getWebviewContent(settings);
  }

  private readSettings(config: vscode.WorkspaceConfiguration): SettingsData {
    return {
      editor: {
        fontSize:
          vscode.workspace
            .getConfiguration("editor")
            .get<number>("fontSize") || 14,
        fontFamily:
          vscode.workspace
            .getConfiguration("editor")
            .get<string>("fontFamily") || "monospace",
        wordWrap:
          vscode.workspace
            .getConfiguration("editor")
            .get<boolean>("wordWrap") || false,
      },
      sql: {
        runOnSave:
          config.get<"never" | "prompt" | "always">("sql.runOnSave") ||
          "prompt",
        runOnOpen:
          config.get<"never" | "prompt" | "always">("sql.runOnOpen") ||
          "never",
        timeout: config.get<number>("sql.timeout") || 30,
        lineLength: config.get<number>("sql.lineLength") || 80,
      },
      results: {
        rowsPerPage: config.get<number>("results.rowsPerPage") || 25,
        autoRefresh: config.get<boolean>("results.autoRefresh") || false,
        autoRefreshIntervalSeconds:
          config.get<number>("results.autoRefreshIntervalSeconds") || 5,
        exportFormat:
          config.get<"csv" | "json">("results.exportFormat") || "csv",
      },
      connection: {
        cacheSchemaEnabled:
          config.get<boolean>("schema.cache.enabled") !== false,
        cacheTTLSeconds: config.get<number>("schema.cache.ttlSeconds") || 300,
        retryAttempts: config.get<number>("connection.retry.attempts") || 3,
        retryDelayMs: config.get<number>("connection.retry.delayMs") || 500,
      },
    };
  }

  private async handleWebviewMessage(message: any) {
    if (message.command === "saveSetting") {
      const { key, value } = message;
      const config = vscode.workspace.getConfiguration("voltnuerongrid");

      try {
        await config.update(key, value, vscode.ConfigurationTarget.Global);
        this.panel.webview.postMessage({
          command: "settingsSaved",
          success: true,
          message: `Setting "${key}" saved successfully.`,
        });
      } catch (error) {
        this.panel.webview.postMessage({
          command: "settingsSaved",
          success: false,
          message: `Failed to save setting: ${error}`,
        });
      }
    }

    if (message.command === "resetToDefaults") {
      const config = vscode.workspace.getConfiguration("voltnuerongrid");
      const defaultSettings = [
        "sql.runOnSave",
        "sql.runOnOpen",
        "schema.cache.enabled",
        "schema.cache.ttlSeconds",
      ];

      try {
        for (const setting of defaultSettings) {
          await config.update(setting, undefined, vscode.ConfigurationTarget.Global);
        }
        this.updateWebview();
        this.panel.webview.postMessage({
          command: "settingsSaved",
          success: true,
          message: "Settings reset to defaults.",
        });
      } catch (error) {
        this.panel.webview.postMessage({
          command: "settingsSaved",
          success: false,
          message: `Failed to reset settings: ${error}`,
        });
      }
    }
  }

  private getWebviewContent(settings: SettingsData): string {
    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>VoltNueronGrid Settings</title>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
      background-color: var(--vscode-editor-background);
      color: var(--vscode-editor-foreground);
      padding: 20px;
      line-height: 1.6;
    }
    h1 {
      font-size: 24px;
      margin-bottom: 20px;
      color: var(--vscode-title-bar-activeForeground);
    }
    h2 {
      font-size: 18px;
      margin-top: 24px;
      margin-bottom: 12px;
      border-bottom: 1px solid var(--vscode-widget-border);
      padding-bottom: 8px;
      color: var(--vscode-sideBar-foreground);
    }
    .section {
      margin-bottom: 24px;
    }
    .setting-group {
      margin-bottom: 16px;
      display: flex;
      flex-direction: column;
      gap: 6px;
    }
    label {
      font-weight: 500;
      font-size: 13px;
      color: var(--vscode-settings-textInputForeground);
    }
    input[type="text"],
    input[type="number"],
    select {
      padding: 8px 12px;
      background-color: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
      border: 1px solid var(--vscode-input-border);
      border-radius: 4px;
      font-family: monospace;
      font-size: 13px;
    }
    input[type="text"]:focus,
    input[type="number"]:focus,
    select:focus {
      outline: none;
      border-color: var(--vscode-focusBorder);
      box-shadow: 0 0 0 1px var(--vscode-focusBorder);
    }
    input[type="checkbox"] {
      width: 16px;
      height: 16px;
      cursor: pointer;
      accent-color: var(--vscode-button-background);
    }
    .checkbox-label {
      display: flex;
      align-items: center;
      gap: 8px;
      cursor: pointer;
    }
    .description {
      font-size: 12px;
      color: var(--vscode-descriptionForeground);
      margin-top: 4px;
    }
    .button-group {
      display: flex;
      gap: 8px;
      margin-top: 24px;
    }
    button {
      padding: 10px 16px;
      background-color: var(--vscode-button-background);
      color: var(--vscode-button-foreground);
      border: none;
      border-radius: 4px;
      font-size: 13px;
      font-weight: 500;
      cursor: pointer;
      transition: background-color 0.2s;
    }
    button:hover {
      background-color: var(--vscode-button-hoverBackground);
    }
    button:active {
      transform: scale(0.98);
    }
    .secondary {
      background-color: var(--vscode-button-secondaryBackground);
      color: var(--vscode-button-secondaryForeground);
    }
    .secondary:hover {
      background-color: var(--vscode-button-secondaryHoverBackground);
    }
    .info-box {
      background-color: var(--vscode-panel-background);
      border-left: 3px solid var(--vscode-notebookStatusSuccessIcon-foreground);
      padding: 12px;
      border-radius: 4px;
      margin-bottom: 16px;
      font-size: 12px;
    }
    .message {
      margin-top: 12px;
      padding: 8px;
      border-radius: 4px;
      font-size: 12px;
      display: none;
    }
    .message.success {
      background-color: var(--vscode-notebookStatusSuccessIcon-foreground);
      color: var(--vscode-editor-background);
      display: block;
    }
    .message.error {
      background-color: var(--vscode-notebookStatusErrorIcon-foreground);
      color: var(--vscode-editor-background);
      display: block;
    }
  </style>
</head>
<body>
  <h1>⚙️ VoltNueronGrid Settings</h1>

  <div id="message" class="message"></div>

  <!-- Editor Settings -->
  <div class="section">
    <h2>Editor</h2>
    <div class="setting-group">
      <label for="fontSize">Font Size</label>
      <input type="number" id="fontSize" min="8" max="32" value="${settings.editor.fontSize}">
      <div class="description">Font size for SQL editor and result tables</div>
    </div>
    <div class="setting-group">
      <label for="fontFamily">Font Family</label>
      <input type="text" id="fontFamily" value="${settings.editor.fontFamily}">
      <div class="description">Monospace font for code (e.g., 'Courier New', 'Consolas')</div>
    </div>
    <div class="setting-group">
      <label class="checkbox-label">
        <input type="checkbox" id="wordWrap" ${settings.editor.wordWrap ? "checked" : ""}>
        Enable Word Wrap
      </label>
      <div class="description">Wrap long lines in editor and results</div>
    </div>
  </div>

  <!-- SQL Execution Settings -->
  <div class="section">
    <h2>SQL</h2>
    <div class="setting-group">
      <label for="runOnSave">Run on Save</label>
      <select id="runOnSave">
        <option value="never" ${settings.sql.runOnSave === "never" ? "selected" : ""}>Never</option>
        <option value="prompt" ${settings.sql.runOnSave === "prompt" ? "selected" : ""}>Prompt</option>
        <option value="always" ${settings.sql.runOnSave === "always" ? "selected" : ""}>Always</option>
      </select>
      <div class="description">Execute SQL automatically when .sql file is saved</div>
    </div>
    <div class="setting-group">
      <label for="runOnOpen">Run on Open</label>
      <select id="runOnOpen">
        <option value="never" ${settings.sql.runOnOpen === "never" ? "selected" : ""}>Never</option>
        <option value="prompt" ${settings.sql.runOnOpen === "prompt" ? "selected" : ""}>Prompt</option>
        <option value="always" ${settings.sql.runOnOpen === "always" ? "selected" : ""}>Always</option>
      </select>
      <div class="description">Execute SQL automatically when .sql file is opened</div>
    </div>
    <div class="setting-group">
      <label for="timeout">Timeout (seconds)</label>
      <input type="number" id="timeout" min="5" max="300" value="${settings.sql.timeout}">
      <div class="description">Query execution timeout</div>
    </div>
    <div class="setting-group">
      <label for="lineLength">Line Length</label>
      <input type="number" id="lineLength" min="40" max="200" value="${settings.sql.lineLength}">
      <div class="description">Preferred SQL formatting width</div>
    </div>
  </div>

  <!-- Results Display Settings -->
  <div class="section">
    <h2>Results</h2>
    <div class="setting-group">
      <label for="rowsPerPage">Rows Per Page</label>
      <input type="number" id="rowsPerPage" min="5" max="100" value="${settings.results.rowsPerPage}">
      <div class="description">Pagination size for result sets</div>
    </div>
    <div class="setting-group">
      <label class="checkbox-label">
        <input type="checkbox" id="autoRefresh" ${settings.results.autoRefresh ? "checked" : ""}>
        Auto-Refresh Results
      </label>
      <div class="description">Periodically refresh query results</div>
    </div>
    <div class="setting-group">
      <label for="autoRefreshInterval">Auto-Refresh Interval (seconds)</label>
      <input type="number" id="autoRefreshInterval" min="1" max="60" value="${settings.results.autoRefreshIntervalSeconds}">
      <div class="description">Interval between result refreshes</div>
    </div>
    <div class="setting-group">
      <label for="exportFormat">Default Export Format</label>
      <select id="exportFormat">
        <option value="csv" ${settings.results.exportFormat === "csv" ? "selected" : ""}>CSV</option>
        <option value="json" ${settings.results.exportFormat === "json" ? "selected" : ""}>JSON</option>
      </select>
      <div class="description">Format for exporting query results</div>
    </div>
  </div>

  <!-- Connection Settings -->
  <div class="section">
    <h2>Connection</h2>
    <div class="setting-group">
      <label class="checkbox-label">
        <input type="checkbox" id="cacheEnabled" ${settings.connection.cacheSchemaEnabled ? "checked" : ""}>
        Enable Schema Cache
      </label>
      <div class="description">Cache database schema for faster tree loading</div>
    </div>
    <div class="setting-group">
      <label for="cacheTTL">Cache TTL (seconds)</label>
      <input type="number" id="cacheTTL" min="5" max="3600" value="${settings.connection.cacheTTLSeconds}">
      <div class="description">Schema cache expires after this duration</div>
    </div>
    <div class="setting-group">
      <label for="retryAttempts">Connection Retry Attempts</label>
      <input type="number" id="retryAttempts" min="1" max="10" value="${settings.connection.retryAttempts}">
      <div class="description">Number of retry attempts on connection failure</div>
    </div>
    <div class="setting-group">
      <label for="retryDelay">Retry Delay (milliseconds)</label>
      <input type="number" id="retryDelay" min="100" max="5000" step="100" value="${settings.connection.retryDelayMs}">
      <div class="description">Delay between connection retry attempts</div>
    </div>
  </div>

  <div class="button-group">
    <button onclick="saveAllSettings()">💾 Save All Settings</button>
    <button class="secondary" onclick="resetToDefaults()">🔄 Reset to Defaults</button>
  </div>

  <script>
    const vscode = acquireVsCodeApi();

    function saveAllSettings() {
      const settings = {
        'editor.fontSize': parseInt(document.getElementById('fontSize').value),
        'editor.fontFamily': document.getElementById('fontFamily').value,
        'editor.wordWrap': document.getElementById('wordWrap').checked,
        'sql.runOnSave': document.getElementById('runOnSave').value,
        'sql.runOnOpen': document.getElementById('runOnOpen').value,
        'sql.timeout': parseInt(document.getElementById('timeout').value),
        'sql.lineLength': parseInt(document.getElementById('lineLength').value),
        'results.rowsPerPage': parseInt(document.getElementById('rowsPerPage').value),
        'results.autoRefresh': document.getElementById('autoRefresh').checked,
        'results.autoRefreshIntervalSeconds': parseInt(document.getElementById('autoRefreshInterval').value),
        'results.exportFormat': document.getElementById('exportFormat').value,
        'schema.cache.enabled': document.getElementById('cacheEnabled').checked,
        'schema.cache.ttlSeconds': parseInt(document.getElementById('cacheTTL').value),
        'connection.retry.attempts': parseInt(document.getElementById('retryAttempts').value),
        'connection.retry.delayMs': parseInt(document.getElementById('retryDelay').value),
      };

      Object.entries(settings).forEach(([key, value]) => {
        vscode.postMessage({
          command: 'saveSetting',
          key: key,
          value: value
        });
      });
    }

    function resetToDefaults() {
      if (confirm('Reset all settings to defaults?')) {
        vscode.postMessage({
          command: 'resetToDefaults'
        });
      }
    }

    window.addEventListener('message', event => {
      const message = event.data;
      if (message.command === 'settingsSaved') {
        const msgDiv = document.getElementById('message');
        msgDiv.textContent = message.message;
        msgDiv.className = 'message ' + (message.success ? 'success' : 'error');
        setTimeout(() => {
          msgDiv.className = 'message';
        }, 3000);
      }
    });
  </script>
</body>
</html>`;
  }

  public dispose() {
    SettingsPanel.currentPanel = undefined;
    this.panel.dispose();
    while (this.disposables.length) {
      const disposable = this.disposables.pop();
      if (disposable) {
        disposable.dispose();
      }
    }
  }
}
