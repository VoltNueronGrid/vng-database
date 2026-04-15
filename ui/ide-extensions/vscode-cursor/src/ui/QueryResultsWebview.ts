import * as vscode from "vscode";
import { QueryResult } from "../models";

export interface QueryResultsState {
  operation: string;
  connectionName: string;
  result: QueryResult;
}

export type QueryResultsMessage =
  | { type: "ready" }
  | { type: "requestExport"; format: "csv" | "json" };

export interface QueryResultsPanelHandle {
  panel: vscode.WebviewPanel;
  updateState: (state: QueryResultsState) => Promise<void>;
  reveal: () => void;
}

export function createQueryResultsPanel(
  context: vscode.ExtensionContext,
  initialState: QueryResultsState,
  onMessage: (message: QueryResultsMessage) => Promise<void>
): QueryResultsPanelHandle {
  const panel = vscode.window.createWebviewPanel("vngQueryResults", "VoltNueronGrid Query Results", vscode.ViewColumn.Beside, {
    enableScripts: true,
    retainContextWhenHidden: true,
  });

  panel.webview.html = getQueryResultsHtml(initialState);

  panel.webview.onDidReceiveMessage(
    async (message: QueryResultsMessage) => {
      await onMessage(message);
    },
    undefined,
    context.subscriptions
  );

  return {
    panel,
    reveal: () => panel.reveal(vscode.ViewColumn.Beside, true),
    updateState: async (state: QueryResultsState) => {
      await panel.webview.postMessage({ type: "state", state });
    },
  };
}

function getQueryResultsHtml(initialState: QueryResultsState): string {
  const stateJson = JSON.stringify(initialState).replace(/</g, "\\u003c");
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>VoltNueronGrid Query Results</title>
  <style>
    body {
      margin: 0;
      padding: 14px;
      font-family: "Segoe UI", Tahoma, Geneva, Verdana, sans-serif;
      color: var(--vscode-foreground);
      background: var(--vscode-editor-background);
    }
    .toolbar {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      align-items: center;
      margin-bottom: 12px;
    }
    .toolbar input,
    .toolbar select,
    .toolbar button {
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
      border: 1px solid var(--vscode-input-border, var(--vscode-panel-border));
      border-radius: 4px;
      padding: 6px 8px;
      font: inherit;
    }
    .toolbar button {
      background: var(--vscode-button-background);
      color: var(--vscode-button-foreground);
      border-color: var(--vscode-button-border, transparent);
      cursor: pointer;
    }
    .toolbar button.secondary {
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
    }
    .summary {
      margin-bottom: 10px;
      opacity: 0.9;
      line-height: 1.4;
      font-size: 12px;
    }
    .error {
      margin: 8px 0 10px 0;
      border: 1px solid var(--vscode-inputValidation-errorBorder, #b73a3a);
      background: color-mix(in srgb, var(--vscode-inputValidation-errorBackground, #5a1d1d) 55%, transparent);
      padding: 8px;
      border-radius: 4px;
      white-space: pre-wrap;
    }
    .table-wrap {
      border: 1px solid var(--vscode-panel-border);
      border-radius: 6px;
      overflow: auto;
      max-height: 58vh;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      min-width: 720px;
    }
    th,
    td {
      border-bottom: 1px solid var(--vscode-panel-border);
      text-align: left;
      padding: 7px 8px;
      vertical-align: top;
      font-size: 12px;
    }
    th button {
      all: unset;
      color: inherit;
      cursor: pointer;
      font-weight: 600;
    }
    .pager {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-top: 10px;
      font-size: 12px;
      opacity: 0.9;
    }
    .pager .controls {
      display: flex;
      gap: 8px;
    }
    .empty {
      padding: 12px;
      opacity: 0.8;
    }
    .query {
      margin-top: 10px;
      padding: 8px;
      border-radius: 6px;
      border: 1px solid var(--vscode-panel-border);
      background: var(--vscode-textCodeBlock-background, rgba(127, 127, 127, 0.12));
      white-space: pre-wrap;
      font-family: Consolas, "Courier New", monospace;
      font-size: 12px;
    }
  </style>
</head>
<body>
  <div class="summary" id="summary"></div>
  <div class="toolbar">
    <input id="filter" type="text" placeholder="Filter rows" />
    <label>
      Page size
      <select id="pageSize">
        <option value="25">25</option>
        <option value="50" selected>50</option>
        <option value="100">100</option>
        <option value="250">250</option>
      </select>
    </label>
    <button class="secondary" id="exportCsv">Export CSV</button>
    <button class="secondary" id="exportJson">Export JSON</button>
  </div>
  <div id="errorBox"></div>
  <div id="tableContainer" class="table-wrap"></div>
  <div class="pager">
    <div id="pagerText"></div>
    <div class="controls">
      <button class="secondary" id="prevPage">Previous</button>
      <button class="secondary" id="nextPage">Next</button>
    </div>
  </div>
  <div id="query" class="query"></div>

  <script>
    const vscode = acquireVsCodeApi();
    let state = __INITIAL_STATE__;
    let filterText = "";
    let sortColumn = "";
    let sortDirection = "asc";
    let currentPage = 1;

    const filterInput = document.getElementById("filter");
    const pageSizeSelect = document.getElementById("pageSize");

    function getPageSize() {
      const value = Number(pageSizeSelect.value);
      return Number.isFinite(value) && value > 0 ? value : 50;
    }

    function normalizeValue(value) {
      if (value === null || value === undefined) {
        return "";
      }
      if (typeof value === "object") {
        try {
          return JSON.stringify(value);
        } catch {
          return String(value);
        }
      }
      return String(value);
    }

    function applyFilter(rows, columns) {
      if (!filterText.trim()) {
        return rows;
      }
      const needle = filterText.toLowerCase();
      return rows.filter((row) =>
        columns.some((column) => normalizeValue(row[column.name]).toLowerCase().includes(needle))
      );
    }

    function applySort(rows) {
      if (!sortColumn) {
        return rows;
      }
      const factor = sortDirection === "asc" ? 1 : -1;
      return [...rows].sort((left, right) => {
        const leftValue = normalizeValue(left[sortColumn]).toLowerCase();
        const rightValue = normalizeValue(right[sortColumn]).toLowerCase();
        if (leftValue === rightValue) {
          return 0;
        }
        return leftValue > rightValue ? factor : -factor;
      });
    }

    function render() {
      const result = state.result;
      const summary = document.getElementById("summary");
      const errorBox = document.getElementById("errorBox");
      const tableContainer = document.getElementById("tableContainer");
      const pagerText = document.getElementById("pagerText");
      const query = document.getElementById("query");
      const columns = Array.isArray(result.columns) ? result.columns : [];
      const rows = Array.isArray(result.rows) ? result.rows : [];

      summary.textContent =
        state.operation +
        " on " +
        state.connectionName +
        " | status=" +
        result.status +
        " | rows=" +
        (result.rowCount || 0) +
        " | execution=" +
        (result.executionTime || 0) +
        "ms";

      query.textContent = result.query || "";

      if (result.status === "error") {
        const message = (result.error && result.error.message) || "Execution failed.";
        const detail = result.error && result.error.detail ? "\n" + result.error.detail : "";
        errorBox.innerHTML = '<div class="error">' + escapeHtml(message + detail) + "</div>";
      } else {
        errorBox.innerHTML = "";
      }

      if (!columns.length) {
        tableContainer.innerHTML = '<div class="empty">No tabular result columns were returned.</div>';
        pagerText.textContent = "0 rows";
        return;
      }

      const filteredRows = applyFilter(rows, columns);
      const sortedRows = applySort(filteredRows);
      const pageSize = getPageSize();
      const totalPages = Math.max(1, Math.ceil(sortedRows.length / pageSize));
      currentPage = Math.min(currentPage, totalPages);
      currentPage = Math.max(1, currentPage);
      const start = (currentPage - 1) * pageSize;
      const pageRows = sortedRows.slice(start, start + pageSize);

      const header = columns
        .map((column) => {
          const isSorted = sortColumn === column.name;
          const marker = isSorted ? (sortDirection === "asc" ? " ▲" : " ▼") : "";
          return '<th><button data-col="' + escapeHtml(column.name) + '">' + escapeHtml(column.name + marker) + "</button></th>";
        })
        .join("");

      const body = pageRows
        .map((row) => {
          const cells = columns
            .map((column) => '<td>' + escapeHtml(normalizeValue(row[column.name])) + "</td>")
            .join("");
          return "<tr>" + cells + "</tr>";
        })
        .join("");

      tableContainer.innerHTML = "<table><thead><tr>" + header + "</tr></thead><tbody>" + body + "</tbody></table>";
      pagerText.textContent = "Rows " + (sortedRows.length === 0 ? 0 : start + 1) + "-" + Math.min(start + pageRows.length, sortedRows.length) + " of " + sortedRows.length + " | Page " + currentPage + " of " + totalPages;

      tableContainer.querySelectorAll("th button").forEach((button) => {
        button.addEventListener("click", () => {
          const column = button.getAttribute("data-col") || "";
          if (!column) {
            return;
          }
          if (sortColumn === column) {
            sortDirection = sortDirection === "asc" ? "desc" : "asc";
          } else {
            sortColumn = column;
            sortDirection = "asc";
          }
          render();
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

    document.getElementById("prevPage").addEventListener("click", () => {
      currentPage = Math.max(1, currentPage - 1);
      render();
    });

    document.getElementById("nextPage").addEventListener("click", () => {
      currentPage = currentPage + 1;
      render();
    });

    filterInput.addEventListener("input", (event) => {
      filterText = event.target.value || "";
      currentPage = 1;
      render();
    });

    pageSizeSelect.addEventListener("change", () => {
      currentPage = 1;
      render();
    });

    document.getElementById("exportCsv").addEventListener("click", () => {
      vscode.postMessage({ type: "requestExport", format: "csv" });
    });

    document.getElementById("exportJson").addEventListener("click", () => {
      vscode.postMessage({ type: "requestExport", format: "json" });
    });

    window.addEventListener("message", (event) => {
      const message = event.data;
      if (message && message.type === "state" && message.state) {
        state = message.state;
        currentPage = 1;
        render();
      }
    });

    vscode.postMessage({ type: "ready" });
    render();
  </script>
</body>
</html>`
    .replace("__INITIAL_STATE__", stateJson);
}