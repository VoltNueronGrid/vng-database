import { useState } from "react";
import { useEditorStore } from "@/store/editor";
import { useQueryStore } from "@/store/query";
import { DataTable } from "./DataTable";

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
            <button className="btn btn-sm">Export ↓</button>
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
