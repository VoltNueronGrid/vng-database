import * as vscode from "vscode";
import { TableEditorSession } from "../models/TableEditor";

export interface TableEditorState {
  connectionId: string;
  connectionName: string;
  session: TableEditorSession;
}

export type TableEditorMessage =
  | { type: "ready" }
  | { type: "refresh" }
  | { type: "save" }
  | { type: "discard" }
  | { type: "copyPendingSql" }
  | { type: "addRow" }
  | { type: "changePage"; direction: "next" | "previous" }
  | { type: "updateCell"; rowId: string; columnName: string; value: string }
  | { type: "toggleDeleteRow"; rowId: string };

export interface TableEditorPanelHandle {
  panel: vscode.WebviewPanel;
  updateState: (state: TableEditorState) => Promise<void>;
  reveal: () => void;
}

export function createTableEditorPanel(
  context: vscode.ExtensionContext,
  initialState: TableEditorState,
  onMessage: (message: TableEditorMessage) => Promise<void>
): TableEditorPanelHandle {
  const panel = vscode.window.createWebviewPanel("vngTableEditor", "VoltNueronGrid Table Editor", vscode.ViewColumn.Beside, {
    enableScripts: true,
    retainContextWhenHidden: true,
  });

  panel.webview.html = getTableEditorHtml(initialState);
  panel.webview.onDidReceiveMessage(
    async (message: TableEditorMessage) => {
      await onMessage(message);
    },
    undefined,
    context.subscriptions
  );

  return {
    panel,
    reveal: () => panel.reveal(vscode.ViewColumn.Beside, true),
    updateState: async (state: TableEditorState) => {
      await panel.webview.postMessage({ type: "state", state });
    },
  };
}

function getTableEditorHtml(initialState: TableEditorState): string {
  const stateJson = JSON.stringify(initialState).replace(/</g, "\\u003c");

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>VoltNueronGrid Table Editor</title>
  <style>
    body {
      margin: 0;
      padding: 14px;
      font-family: "Segoe UI", Tahoma, Geneva, Verdana, sans-serif;
      color: var(--vscode-foreground);
      background: var(--vscode-editor-background);
    }
    .headline {
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 12px;
      margin-bottom: 12px;
    }
    .headline h1 {
      font-size: 16px;
      margin: 0 0 4px 0;
    }
    .subtitle {
      font-size: 12px;
      opacity: 0.85;
      line-height: 1.4;
    }
    .toolbar {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      align-items: center;
      margin-bottom: 12px;
    }
    .shortcut-hint {
      margin-left: auto;
      font-size: 11px;
      opacity: 0.75;
    }
    button {
      background: var(--vscode-button-background);
      color: var(--vscode-button-foreground);
      border: 1px solid var(--vscode-button-border, transparent);
      border-radius: 4px;
      padding: 6px 10px;
      font: inherit;
      cursor: pointer;
    }
    button.secondary {
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
      border-color: var(--vscode-input-border, var(--vscode-panel-border));
    }
    button:disabled {
      opacity: 0.55;
      cursor: not-allowed;
    }
    .notice {
      font-size: 12px;
      margin-bottom: 10px;
      padding: 8px 10px;
      border-radius: 6px;
      border: 1px solid var(--vscode-panel-border);
      background: color-mix(in srgb, var(--vscode-editorInfo-background, rgba(127,127,127,0.12)) 50%, transparent);
      white-space: pre-wrap;
    }
    .notice.error {
      border-color: var(--vscode-inputValidation-errorBorder, #b73a3a);
      background: color-mix(in srgb, var(--vscode-inputValidation-errorBackground, #5a1d1d) 55%, transparent);
    }
    .table-wrap {
      border: 1px solid var(--vscode-panel-border);
      border-radius: 6px;
      overflow: auto;
      max-height: 62vh;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      min-width: 900px;
    }
    th,
    td {
      border-bottom: 1px solid var(--vscode-panel-border);
      text-align: left;
      padding: 6px 8px;
      vertical-align: top;
      font-size: 12px;
    }
    th {
      position: sticky;
      top: 0;
      background: var(--vscode-editor-background);
      z-index: 1;
    }
    td input {
      width: 100%;
      min-width: 120px;
      box-sizing: border-box;
      padding: 4px 6px;
      border-radius: 4px;
      border: 1px solid var(--vscode-input-border, var(--vscode-panel-border));
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
      font: inherit;
    }
    td input.invalid {
      border-color: var(--vscode-inputValidation-errorBorder, #b73a3a);
      box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--vscode-inputValidation-errorBorder, #b73a3a) 45%, transparent);
    }
    tr.deleted {
      opacity: 0.6;
    }
    tr.deleted td {
      text-decoration: line-through;
    }
    .key-badge,
    .draft-badge,
    .read-only-badge {
      display: inline-block;
      margin-left: 6px;
      padding: 1px 5px;
      border-radius: 999px;
      font-size: 10px;
      letter-spacing: 0.04em;
      border: 1px solid var(--vscode-panel-border);
      opacity: 0.9;
    }
    .pager {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-top: 10px;
      font-size: 12px;
      opacity: 0.9;
    }
    .empty {
      padding: 14px;
      opacity: 0.8;
    }
  </style>
</head>
<body>
  <div class="headline">
    <div>
      <h1 id="title" aria-live="polite"></h1>
      <div class="subtitle" id="subtitle"></div>
    </div>
    <div class="subtitle" id="meta"></div>
  </div>

  <div class="toolbar" role="toolbar" aria-label="Table editor actions">
    <button id="addRow" aria-label="Add a draft row">Add Row</button>
    <button id="save" aria-label="Save table edits">Save Changes</button>
    <button id="discard" class="secondary" aria-label="Discard unsaved table edits">Discard</button>
    <button id="refresh" class="secondary" aria-label="Refresh table rows">Refresh</button>
    <span class="shortcut-hint">Table Editor shortcuts: Ctrl+Shift+F open, Ctrl+S save, Ctrl+Shift+N add row</span>
  </div>

  <div id="noticeBox" aria-live="polite"></div>
  <div id="tableContainer" class="table-wrap" role="region" aria-label="Table rows editor"></div>

  <div class="pager" role="navigation" aria-label="Table editor pagination">
    <div id="pagerText"></div>
    <div>
      <button class="secondary" id="prevPage" aria-label="Previous page of table rows">Previous</button>
      <button class="secondary" id="nextPage" aria-label="Next page of table rows">Next</button>
    </div>
  </div>

  <script>
    const vscode = acquireVsCodeApi();
    let state = __INITIAL_STATE__;

    function render() {
      const session = state.session;
      const title = document.getElementById("title");
      const subtitle = document.getElementById("subtitle");
      const meta = document.getElementById("meta");
      const noticeBox = document.getElementById("noticeBox");
      const tableContainer = document.getElementById("tableContainer");
      const pagerText = document.getElementById("pagerText");
      const addRowButton = document.getElementById("addRow");
      const saveButton = document.getElementById("save");
      const discardButton = document.getElementById("discard");
      const refreshButton = document.getElementById("refresh");
      const prevPageButton = document.getElementById("prevPage");
      const nextPageButton = document.getElementById("nextPage");

      title.textContent = session.target.schema + "." + session.target.tableName;
      subtitle.textContent = "Connection: " + state.connectionName;
      meta.textContent = "Page " + session.page + " | " + session.rows.length + " row(s) loaded";

      addRowButton.disabled = !session.capabilities.canInsert;
      saveButton.disabled = !session.dirty;
      discardButton.disabled = !session.dirty;
      refreshButton.disabled = session.dirty;
      prevPageButton.disabled = session.page <= 1 || session.dirty;
      nextPageButton.disabled = !session.hasNextPage || session.dirty;

      let notices = "";
      if (session.errorMessage) {
        notices += '<div class="notice error" role="alert">' + escapeHtml(session.errorMessage) + "</div>";
      }
      if (session.infoMessage) {
        notices += '<div class="notice" role="status">' + escapeHtml(session.infoMessage) + "</div>";
      }
      if (session.capabilities.readOnlyReason) {
        notices += '<div class="notice" role="status">' + escapeHtml(session.capabilities.readOnlyReason) + "</div>";
      }
      if (session.dirty) {
        notices += '<div class="notice" role="status">Unsaved changes are present. Save or discard before navigating pages or refreshing.</div>';
      }
      if (session.partialSave && session.pendingSaveSql && session.pendingSaveSql.length > 0) {
        notices +=
          '<div class="notice error" role="alert">Partial save detected. Applied ' +
          escapeHtml(String(session.partialSave.applied)) +
          ' of ' +
          escapeHtml(String(session.partialSave.total)) +
          ' changes. <button id="copyPendingSql" class="secondary" aria-label="Open pending SQL statements in a new editor">Copy Pending SQL</button></div>'; 
      }
      noticeBox.innerHTML = notices;

      const copyPendingSqlButton = document.getElementById("copyPendingSql");
      if (copyPendingSqlButton) {
        copyPendingSqlButton.addEventListener("click", () => {
          vscode.postMessage({ type: "copyPendingSql" });
        });
      }

      if (!session.columns.length) {
        tableContainer.innerHTML = '<div class="empty" role="status">No columns were returned for this table.</div>';
        pagerText.textContent = "0 rows";
        return;
      }

      const header = ["<th scope=\"col\">Actions</th>"]
        .concat(
          session.columns.map((column) => {
            const isKey = session.capabilities.keyColumns.includes(column.name);
            const isReadOnly = column.type === "BYTEA";
            const badges = [
              isKey ? '<span class="key-badge">KEY</span>' : "",
              isReadOnly ? '<span class="read-only-badge">READ ONLY</span>' : "",
            ].join("");
            return "<th scope=\"col\">" + escapeHtml(column.name) + badges + "</th>";
          })
        )
        .join("");

      const body = session.rows
        .map((row) => {
          const actionLabel = row.kind === "draft" ? "Remove" : row.isDeleted ? "Undo Delete" : "Delete";
          const actionButton =
            '<button class="secondary row-action" data-row-id="' +
            escapeHtml(row.rowId) +
            '" aria-label="' + escapeHtml(actionLabel + ' row ' + row.rowId) + '">' +
            escapeHtml(actionLabel) +
            "</button>";

          const cells = session.columns
            .map((column) => {
              const isBinary = column.type === "BYTEA";
              const isKey = session.capabilities.keyColumns.includes(column.name);
              const readOnly = row.kind === "existing" && (isBinary || isKey || !session.capabilities.canUpdate || row.isDeleted);
              const draftReadOnly = row.kind === "draft" && (!session.capabilities.canInsert || isBinary || row.isDeleted);
              const disabled = readOnly || draftReadOnly ? "disabled" : "";
              const value = row.values[column.name] || "";
              const cellError = session.cellErrors && session.cellErrors[row.rowId] ? session.cellErrors[row.rowId][column.name] : "";
              const invalidClass = cellError ? "invalid" : "";
              const title = cellError ? ' title="' + escapeHtml(cellError) + '"' : "";
              return (
                '<td><input data-row-id="' +
                escapeHtml(row.rowId) +
                '" data-column-name="' +
                escapeHtml(column.name) +
                '" aria-label="' + escapeHtml(column.name + ' for row ' + row.rowId) + '" aria-invalid="' + (cellError ? "true" : "false") + '" value="' +
                escapeHtml(value) +
                '" class="' +
                invalidClass +
                '"' +
                title +
                " " +
                disabled +
                " /></td>"
              );
            })
            .join("");

          const badges = row.kind === "draft" ? '<span class="draft-badge">DRAFT</span>' : "";
          return '<tr class="' + (row.isDeleted ? "deleted" : "") + '"><td>' + actionButton + badges + "</td>" + cells + "</tr>";
        })
        .join("");

      tableContainer.innerHTML = "<table aria-label=\"Editable table rows\"><thead><tr>" + header + "</tr></thead><tbody>" + body + "</tbody></table>";
      pagerText.textContent = "Loaded " + session.rows.length + " row(s)";

      tableContainer.querySelectorAll("input[data-row-id]").forEach((input) => {
        input.addEventListener("change", (event) => {
          const target = event.target;
          vscode.postMessage({
            type: "updateCell",
            rowId: target.getAttribute("data-row-id"),
            columnName: target.getAttribute("data-column-name"),
            value: target.value,
          });
        });
      });

      tableContainer.querySelectorAll("button.row-action").forEach((button) => {
        button.addEventListener("click", () => {
          vscode.postMessage({
            type: "toggleDeleteRow",
            rowId: button.getAttribute("data-row-id"),
          });
        });
      });
    }

    function escapeHtml(value) {
      return String(value)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/\"/g, "&quot;")
        .replace(/'/g, "&#39;");
    }

    document.getElementById("addRow").addEventListener("click", () => vscode.postMessage({ type: "addRow" }));
    document.getElementById("save").addEventListener("click", () => vscode.postMessage({ type: "save" }));
    document.getElementById("discard").addEventListener("click", () => vscode.postMessage({ type: "discard" }));
    document.getElementById("refresh").addEventListener("click", () => vscode.postMessage({ type: "refresh" }));
    document.getElementById("prevPage").addEventListener("click", () => vscode.postMessage({ type: "changePage", direction: "previous" }));
    document.getElementById("nextPage").addEventListener("click", () => vscode.postMessage({ type: "changePage", direction: "next" }));

    window.addEventListener("message", (event) => {
      const message = event.data;
      if (message && message.type === "state" && message.state) {
        state = message.state;
        render();
      }
    });

    vscode.postMessage({ type: "ready" });
    render();
  </script>
</body>
</html>`.replace("__INITIAL_STATE__", stateJson);
}