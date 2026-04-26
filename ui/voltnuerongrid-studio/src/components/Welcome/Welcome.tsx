import { useUiStore } from "@/store/ui";
import { useConnectionStore } from "@/store/connection";
import { useEditorStore } from "@/store/editor";

export function Welcome() {
  const setScreen = useUiStore((s) => s.setScreen);
  const openConnectionPanel = useUiStore((s) => s.openConnectionPanel);
  const connections = useConnectionStore((s) => s.connections);
  const setActive = useConnectionStore((s) => s.setActive);
  const openSqlTab = useEditorStore((s) => s.openSqlTab);

  function connect(id: string) {
    setActive(id);
    setScreen("main");
  }

  function newQuery() {
    openSqlTab();
    setScreen("main");
  }

  const sorted = [...connections].sort(
    (a, b) => (b.lastUsed ?? b.createdAt) - (a.lastUsed ?? a.createdAt)
  );

  return (
    <div className="welcome" style={{ flex: 1 }}>
      <div className="welcome-logo">V</div>

      <div style={{ textAlign: "center" }}>
        <div className="welcome-title">
          <span>VoltNueronGrid</span> Studio
        </div>
        <div className="welcome-sub" style={{ marginTop: 10 }}>
          A modern, high-performance database studio for HTAP workloads.
          Connect to VoltNueronGrid, PostgreSQL, or MySQL.
        </div>
      </div>

      <div className="welcome-cards">
        <div className="welcome-card" onClick={() => openConnectionPanel(null)}>
          <div className="wc-icon">⚡</div>
          <div className="wc-title">New Connection</div>
          <div className="wc-desc">Connect to a database</div>
        </div>
        <div className="welcome-card" onClick={newQuery}>
          <div className="wc-icon">📝</div>
          <div className="wc-title">New Query</div>
          <div className="wc-desc">Open blank SQL editor</div>
        </div>
        <div className="welcome-card" onClick={() => setScreen("dashboard")}>
          <div className="wc-icon">📊</div>
          <div className="wc-title">Dashboard</div>
          <div className="wc-desc">Monitor cluster health</div>
        </div>
      </div>

      {sorted.length > 0 && (
        <div style={{ width: "100%", maxWidth: 480 }}>
          <div className="section-label">Recent Connections</div>
          <div className="recent-list">
            {sorted.slice(0, 5).map((c) => (
              <div
                key={c.id}
                className="recent-item"
                onClick={() => connect(c.id)}
              >
                <span
                  className={`conn-dot ${c.id === sorted[0].id ? "ok" : "none"}`}
                />
                <span style={{ flex: 1, fontSize: 12.5 }}>
                  {c.name}
                  <span className="text-muted" style={{ marginLeft: 6 }}>
                    — {c.host}:{c.port}
                  </span>
                </span>
                <span className={`conn-type-badge ${c.serverType === "postgresql" ? "pg" : ""}`}>
                  {c.serverType === "voltnuerongrid" ? "VNG" : c.serverType.toUpperCase()}
                </span>
                <span className="recent-time">
                  {c.lastUsed
                    ? new Date(c.lastUsed).toLocaleDateString()
                    : "Never used"}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
