import { useConnectionStore } from "@/store/connection";
import { useEditorStore } from "@/store/editor";
import { useQueryStore } from "@/store/query";
import { useThemeStore } from "@/store/theme";

export function StatusBar() {
  const activeId = useConnectionStore((s) => s.activeId);
  const connections = useConnectionStore((s) => s.connections);
  const health = useConnectionStore((s) => s.health);
  const activeTabId = useEditorStore((s) => s.activeTabId);
  const result = useQueryStore((s) => activeTabId ? s.results[activeTabId] ?? null : null);
  const executing = useQueryStore((s) => activeTabId ? s.executing.has(activeTabId) : false);
  const themeMode = useThemeStore((s) => s.mode);
  const cycleTheme = useThemeStore((s) => s.cycleMode);

  const conn = connections.find((c) => c.id === activeId);
  const h = activeId ? health[activeId] : null;
  const dotClass = h?.state === "ok" ? "ok" : h?.state === "error" ? "err" : "ok";

  const themeIcon = themeMode === "light" ? "☀" : themeMode === "dark" ? "☾" : "◐";

  return (
    <div className="statusbar">
      {conn ? (
        <>
          <div className="status-item">
            <span className={`status-dot ${dotClass}`} />
            {conn.name}
          </div>
          <div className="status-sep" />
          <div className="status-item">{conn.host}:{conn.port}</div>
          <div className="status-sep" />
          <div className="status-item">{conn.mode}</div>
        </>
      ) : (
        <div className="status-item">No connection</div>
      )}

      <div className="status-sep" />

      {executing && (
        <div className="status-item" style={{ color: "var(--yellow)" }}>⟳ Running…</div>
      )}
      {result && !executing && (
        <>
          <div className="status-item">
            <span className="status-cyan">{result.routePath.toUpperCase()}</span>
          </div>
          <div className="status-sep" />
          <div className="status-item">{result.elapsedMs} ms</div>
          <div className="status-sep" />
          <div className="status-item">{result.rowCount} rows</div>
        </>
      )}

      <div className="status-spacer" />

      <button
        className="status-item"
        title={`Theme: ${themeMode} — click to cycle`}
        onClick={cycleTheme}
      >
        <span>{themeIcon}</span>
        <span style={{ textTransform: "capitalize" }}>{themeMode}</span>
      </button>
      <div className="status-sep" />
      <div className="status-item">UTF-8</div>
      <div className="status-sep" />
      <div className="status-item">v0.1.0</div>
    </div>
  );
}
