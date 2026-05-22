import { useState } from "react";
import { useEditorStore } from "@/store/editor";
import { useQueryStore } from "@/store/query";
import { DataTable } from "./DataTable";
import type { QueryResult } from "@/store/query";

// ── L-1: Export helpers ────────────────────────────────────────────────────────

type ExportFormat = "csv" | "json";

/** Build a CSV string from query result columns + rows. */
function toCSV(result: QueryResult): string {
  const headers = result.columns.map((c) => c.name);
  const escape = (v: unknown): string => {
    const s = v == null ? "" : String(v);
    // Wrap in double-quotes if the value contains commas, quotes, or newlines.
    return s.includes(",") || s.includes('"') || s.includes("\n")
      ? `"${s.replace(/"/g, '""')}"`
      : s;
  };
  const rows = result.rows.map((row) =>
    headers.map((h) => escape(row[h])).join(",")
  );
  return [headers.join(","), ...rows].join("\n");
}

/** Trigger a browser file download using a Blob + object URL. */
function downloadBlob(content: string, filename: string, mimeType: string): void {
  const blob = new Blob([content], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  // Release the object URL shortly after — the browser needs a moment to start
  // the download before we revoke the reference.
  setTimeout(() => URL.revokeObjectURL(url), 5_000);
}

/** Export query results to the requested format. */
function exportResults(result: QueryResult, fmt: ExportFormat): void {
  const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  if (fmt === "csv") {
    downloadBlob(toCSV(result), `vng-export-${ts}.csv`, "text/csv;charset=utf-8;");
  } else {
    downloadBlob(
      JSON.stringify({ columns: result.columns, rows: result.rows }, null, 2),
      `vng-export-${ts}.json`,
      "application/json",
    );
  }
}

type ResultTab = "results" | "messages" | "explain";

/**
 * Format a duration given in milliseconds into a human-readable string that
 * uses the most appropriate unit:
 *   < 0.001 ms  → "< 1 µs"
 *   < 1 ms      → e.g. "412 µs"
 *   < 1 000 ms  → e.g. "47 ms"
 *   ≥ 1 000 ms  → e.g. "3.2 s"
 */
function formatElapsed(ms: number): string {
  if (ms <= 0) return "0 ms";
  if (ms < 0.001) return "< 1 µs";
  if (ms < 1) return `${Math.round(ms * 1000)} µs`;
  if (ms < 1000) return `${ms % 1 === 0 ? ms : ms.toFixed(1)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

export function ResultsPane() {
  const [activeTab, setActiveTab] = useState<ResultTab>("results");
  const [exportFmt, setExportFmt] = useState<ExportFormat>("csv");
  const activeTabId = useEditorStore((s) => s.activeTabId);
  const result = useQueryStore((s) =>
    activeTabId ? s.results[activeTabId] ?? null : null
  );
  const isExecuting = useQueryStore((s) =>
    activeTabId ? s.executing.has(activeTabId) : false
  );

  return (
    <div className="results-pane">
      <div className="results-toolbar">
        {(["results", "messages", "explain"] as ResultTab[]).map((t) => (
          <button
            key={t}
            className={`results-tab-btn ${activeTab === t ? "active" : ""}`}
            onClick={() => setActiveTab(t)}
          >
            {t.charAt(0).toUpperCase() + t.slice(1)}
          </button>
        ))}

        {result && (
          <div className="results-meta">
            <span>Rows</span>
            <span className="v">{result.rowCount.toLocaleString()}</span>
            <div className="results-sep" />
            <span>Time</span>
            <span className="v">{formatElapsed(result.elapsedMs)}</span>
            <div className="results-sep" />
            <span className={`route-badge route-${result.routePath}`}>
              {result.routePath.toUpperCase()}
            </span>
            <div className="results-sep" />
            {/* L-1: Export — download result set as CSV or JSON. */}
            <select
              className="btn btn-sm"
              style={{ padding: "0 4px", cursor: "pointer" }}
              value={exportFmt}
              onChange={(e) => setExportFmt(e.target.value as ExportFormat)}
              title="Choose export format"
            >
              <option value="csv">CSV</option>
              <option value="json">JSON</option>
            </select>
            <button
              className="btn btn-sm"
              onClick={() => exportResults(result, exportFmt)}
              title={`Download results as ${exportFmt.toUpperCase()}`}
            >
              Export ↓
            </button>
          </div>
        )}
      </div>

      {/* Content */}
      {isExecuting && (
        <div className="results-empty">
          <div style={{ color: "var(--yellow)", fontSize: 20 }}>⟳</div>
          <div className="text-muted">Executing…</div>
        </div>
      )}

      {!isExecuting && activeTab === "results" && (
        <>
          {!result && (
            <div className="results-empty">
              <div className="re-icon">📋</div>
              <div className="text-muted">
                Run a query to see results here.
              </div>
              <div style={{ fontSize: 11, color: "var(--text-3)" }}>
                Press ⌘Enter or click Run
              </div>
            </div>
          )}
          {result?.error && (
            <div className="results-empty">
              <div style={{ fontSize: 22 }}>⚠</div>
              <div className="results-error">{result.error}</div>
            </div>
          )}
          {result && !result.error && result.columns.length > 0 && (
            <DataTable columns={result.columns} rows={result.rows} />
          )}
          {result && !result.error && result.columns.length === 0 && (
            <div className="results-empty">
              <div className="re-icon">✓</div>
              <div className="text-muted">
                Query executed successfully.
              </div>
              <div style={{ fontSize: 11, color: "var(--text-3)" }}>
                {result.rejectedCount > 0
                  ? `${result.rejectedCount} statements rejected`
                  : "No rows returned."}
              </div>
            </div>
          )}
        </>
      )}

      {!isExecuting && activeTab === "messages" && (
        <div className="panel-body" style={{ fontFamily: "monospace", fontSize: 12 }}>
          {result ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <div>
                <span style={{ color: "var(--text-3)" }}>status: </span>
                <span style={{ color: result.error ? "var(--red)" : "var(--green)" }}>
                  {result.status}
                </span>
              </div>
              <div>
                <span style={{ color: "var(--text-3)" }}>route: </span>
                <span className={`route-badge route-${result.routePath}`}>
                  {result.routePath}
                </span>
              </div>
              {result.transactionId && (
                <div>
                  <span style={{ color: "var(--text-3)" }}>transaction_id: </span>
                  <span>{result.transactionId}</span>
                </div>
              )}
              {result.rejectedCount > 0 && (
                <div style={{ color: "var(--red)" }}>
                  ⚠ {result.rejectedCount} statement(s) rejected
                </div>
              )}
              {result.error && (
                <div style={{ color: "var(--red)", marginTop: 8 }}>
                  {result.error}
                </div>
              )}
            </div>
          ) : (
            <div className="text-muted">No messages.</div>
          )}
        </div>
      )}

      {!isExecuting && activeTab === "explain" && (
        <div className="results-empty">
          <div className="re-icon">🔍</div>
          <div className="text-muted">
            Query explain plan — coming soon.
          </div>
        </div>
      )}
    </div>
  );
}
