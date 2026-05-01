import { useEditorStore } from "@/store/editor";
import { useQueryStore } from "@/store/query";
import { useQuery } from "@/hooks/useQuery";
import { useConnectionStore } from "@/store/connection";

/** Format elapsed ms into an appropriate unit string. */
function formatElapsed(ms: number): string {
  if (ms <= 0) return "0 ms";
  if (ms < 0.001) return "< 1 µs";
  if (ms < 1) return `${Math.round(ms * 1000)} µs`;
  if (ms < 1000) return `${ms % 1 === 0 ? ms : ms.toFixed(1)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

export function Toolbar() {
  const activeTabId = useEditorStore((s) => s.activeTabId);
  const getActiveTab = useEditorStore((s) => s.getActiveTab);
  const getSelectedSql = useEditorStore((s) => s.getSelectedSql);
  const getDatabases = useConnectionStore((s) => s.getDatabases);

  const isExecuting = useQueryStore((s) =>
    activeTabId ? s.executing.has(activeTabId) : false
  );
  const result = useQueryStore((s) =>
    activeTabId ? s.results[activeTabId] ?? null : null
  );

  const { execute } = useQuery(activeTabId ?? "");

  function run() {
    const tab = getActiveTab();
    if (!tab) return;
    // If user has text selected in the editor, run only that selection
    const selected = getSelectedSql();
    const sql = selected ?? tab.sql;
    if (!sql.trim()) return;
    execute(sql);
  }

  const databases = getDatabases();
  const routePath = result?.routePath ?? null;

  return (
    <div className="toolbar">
      <button
        className="btn primary"
        onClick={run}
        disabled={isExecuting || !activeTabId}
        title="Run query or selection (⌘Enter)"
      >
        <span>{isExecuting ? "⟳" : "▶"}</span>
        {isExecuting ? "Running…" : "Run"}
      </button>

      <div className="toolbar-sep" />

      <button className="btn" title="Format SQL">
        Format
      </button>
      <button className="btn" title="Explain query plan">
        Explain
      </button>

      <div className="toolbar-sep" />

      {databases.length > 0 && (
        <select className="toolbar-select" title="Database">
          {databases.map((db) => (
            <option key={db.name}>{db.name}</option>
          ))}
        </select>
      )}

      <div className="toolbar-spacer" />

      {routePath && (
        <span className={`route-badge route-${routePath}`}>
          {routePath.toUpperCase()}
        </span>
      )}
      {result && (
        <span style={{ fontSize: 11, color: "var(--text-3)", marginLeft: 8 }}>
          {formatElapsed(result.elapsedMs)}
          {result.rowCount > 0 && ` · ${result.rowCount} rows`}
        </span>
      )}
    </div>
  );
}
