/* global React */
const { useState: useStateM, useMemo: useMemoM, useEffect: useEffectM } = React;

/* ───────────────── Workspace: tabs + toolbar + editor + results ───────────────── */
function Toolbar({ tab, dispatch }) {
  return (
    <div className="toolbar">
      <button className="btn primary" onClick={() => dispatch({ type: "runQuery" })}>▶ Run</button>
      <button className="btn">⏸ Stop</button>
      <div className="toolbar-sep" />
      <select className="toolbar-select" defaultValue="auto">
        <option value="auto">Auto-route</option>
        <option value="oltp">OLTP</option>
        <option value="olap">OLAP</option>
      </select>
      <span className={`route-badge route-${tab.lastRoute || "unknown"}`}>{(tab.lastRoute || "AUTO").toUpperCase()}</span>
      <div className="toolbar-sep" />
      <button className="btn">⌘ Format</button>
      <button className="btn">★ Save</button>
      <button className="btn">⤓ Export</button>
      <div className="toolbar-spacer" />
      <button className="btn btn-sm">⌘K Command Palette</button>
    </div>
  );
}

function ResultsPane({ tab }) {
  if (tab.kind === "table" || tab.kind === "sql") {
    const r = window.SAMPLE_RESULT;
    return (
      <div className="results-pane">
        <div className="results-toolbar">
          <button className="results-tab-btn active">Results</button>
          <button className="results-tab-btn">Plan</button>
          <button className="results-tab-btn">Messages</button>
          <button className="results-tab-btn">History</button>
          <div className="results-meta">
            <span><span className="v">{r.rowCount}</span> rows</span>
            <div className="results-sep" />
            <span><span className="v">{r.elapsedMs}</span> ms</span>
            <div className="results-sep" />
            <span className="status-cyan">{r.routePath.toUpperCase()}</span>
          </div>
        </div>
        <div className="data-table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th style={{ width: 40 }}>#</th>
                {r.columns.map((c) => (
                  <th key={c.name}>
                    {c.name}
                    <span className="th-type">{c.type}</span>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {r.rows.map((row, i) => (
                <tr key={i}>
                  <td className="row-num">{i + 1}</td>
                  <td className="cell-num">{row.id}</td>
                  <td className="cell-num">{row.user_id}</td>
                  <td>{row.kind}</td>
                  <td className={row.payload == null ? "cell-null" : ""}>{row.payload ?? "NULL"}</td>
                  <td className="cell-date">{row.created_at}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    );
  }
  return (
    <div className="results-pane">
      <div className="results-empty"><div className="re-icon">▶</div><div>Run a query to see results</div></div>
    </div>
  );
}

function EditorPane({ tab, dispatch }) {
  return (
    <textarea
      className="mono"
      value={tab.sql}
      onChange={(e) => dispatch({ type: "updateSql", id: tab.id, sql: e.target.value })}
      style={{
        flex: 1.4, padding: 14, fontSize: 13, lineHeight: 1.55,
        background: "var(--bg-base)", color: "var(--text-1)",
        border: "none", outline: "none", resize: "none",
        fontFamily: "'SF Mono', 'Fira Code', 'Cascadia Code', monospace",
      }}
      spellCheck={false}
    />
  );
}

function Workspace({ state, dispatch }) {
  const tab = state.tabs.find((t) => t.id === state.activeTabId);
  return (
    <div className="workspace">
      <div className="tabbar">
        {state.tabs.map((t) => (
          <div
            key={t.id}
            className={window.cx("tab", t.id === state.activeTabId && "active")}
            onClick={() => dispatch({ type: "setActiveTab", id: t.id })}
          >
            <span className="tab-icon">{t.kind === "table" ? "📋" : "‹›"}</span>
            <span className="tab-label">{t.name}</span>
            {t.dirty && <span className="tab-dirty">●</span>}
            <button className="tab-close" onClick={(e) => { e.stopPropagation(); dispatch({ type: "closeTab", id: t.id }); }}>✕</button>
          </div>
        ))}
        <button className="tab-new-btn" onClick={() => dispatch({ type: "openSqlTab", sql: "-- New Query\nSELECT 1;\n", name: "untitled.sql" })}>＋</button>
      </div>
      {tab ? (
        <>
          <Toolbar tab={tab} dispatch={dispatch} />
          <div className="editor-area">
            <EditorPane tab={tab} dispatch={dispatch} />
            <div className="resize-handle" />
            <ResultsPane tab={tab} />
          </div>
        </>
      ) : (
        <div className="results-empty" style={{ flex: 1 }}>
          <div className="re-icon">📋</div>
          <div>Select a table from the sidebar to begin</div>
        </div>
      )}
    </div>
  );
}

/* ───────────────── Right Panel ───────────────── */
function RightPanel({ state, dispatch }) {
  const target = state.rightPanelTarget; // "schema.table"
  if (!target) return null;
  const [schemaName, tableName] = target.split(".");
  let table = null;
  for (const db of window.SAMPLE_SCHEMA.databases) {
    for (const ns of db.schemas) {
      if (ns.name === schemaName) {
        const t = ns.tables.find((t) => t.name === tableName);
        if (t) { table = t; break; }
      }
    }
    if (table) break;
  }

  return (
    <div className="right-panel">
      <div className="panel-header">
        <span className="tree-icon">📋</span>
        {tableName ?? "Table"}
        <button className="panel-close" onClick={() => dispatch({ type: "closeRightPanel" })}>✕</button>
      </div>
      <div className="panel-body">
        {table ? (<>
          <div className="detail-section">
            <div className="detail-title">Stats</div>
            <div className="detail-stat-grid">
              <div className="detail-stat">
                <div className="ds-val cyan">{table.row_count?.toLocaleString() ?? "—"}</div>
                <div className="ds-label">Rows</div>
              </div>
              <div className="detail-stat">
                <div className="ds-val">{table.columns.length}</div>
                <div className="ds-label">Columns</div>
              </div>
            </div>
          </div>
          <div className="detail-section">
            <div className="detail-title">Columns</div>
            <div className="col-list">
              {table.columns.map((c) => (
                <div key={c.name} className="col-row">
                  {c.primary_key ? <span className="pk-marker">🔑</span> : <span style={{ width: 14 }} />}
                  <span className="col-row-name mono">{c.name}</span>
                  <span className={`col-chip ${window.colTypeClass(c.data_type)}`}>{c.data_type}</span>
                </div>
              ))}
            </div>
          </div>
          <div className="detail-section">
            <div className="detail-title">Quick Actions</div>
            <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
              <button className="btn" style={{ justifyContent: "center", fontSize: 11.5 }}
                onClick={() => dispatch({ type: "openTableTab", schema: schemaName, table: tableName })}
              >SELECT * LIMIT 100</button>
              <button className="btn" style={{ justifyContent: "center", fontSize: 11.5 }}
                onClick={() => dispatch({ type: "openModal", kind: "view-ddl", target: `${schemaName}.${tableName}`, payload: { kind: "table", table } })}
              >View DDL</button>
              <button className="btn" style={{ justifyContent: "center", fontSize: 11.5 }}>Analyze Table</button>
              <button className="btn danger" style={{ justifyContent: "center", fontSize: 11.5 }}
                onClick={() => dispatch({ type: "openModal", kind: "truncate-table", target: `analytics.${schemaName}.${tableName}` })}
              >TRUNCATE TABLE</button>
            </div>
          </div>
        </>) : <div className="results-empty"><div className="re-icon">📋</div><div className="text-muted">Table not found</div></div>}
      </div>
    </div>
  );
}

/* ───────────────── Status Bar ───────────────── */
function StatusBar({ state, dispatch }) {
  const conn = state.connections.find((c) => c.id === state.activeId);
  const tab = state.tabs.find((t) => t.id === state.activeTabId);
  const r = tab && (tab.kind === "table" || tab.kind === "sql") ? window.SAMPLE_RESULT : null;
  const themeIcons = { light: "☀", dark: "☾", system: "◐" };
  return (
    <div className="statusbar">
      {conn ? <>
        <div className="status-item"><span className="status-dot ok" />{conn.name}</div>
        <div className="status-sep" />
        <div className="status-item">{conn.host}:{conn.port}</div>
        <div className="status-sep" />
        <div className="status-item">{conn.mode}</div>
      </> : <div className="status-item">No connection</div>}
      <div className="status-sep" />
      {r && <>
        <div className="status-item"><span className="status-cyan">{r.routePath.toUpperCase()}</span></div>
        <div className="status-sep" />
        <div className="status-item">{r.elapsedMs} ms</div>
        <div className="status-sep" />
        <div className="status-item">{r.rowCount} rows</div>
      </>}
      <div className="status-spacer" />
      <button className="status-item" onClick={() => dispatch({ type: "cycleTheme" })}>
        <span>{themeIcons[state.themeMode]}</span>
        <span style={{ textTransform: "capitalize" }}>{state.themeMode}</span>
      </button>
      <div className="status-sep" />
      <div className="status-item">UTF-8</div>
      <div className="status-sep" />
      <div className="status-item">v0.1.0</div>
    </div>
  );
}

/* ───────────────── Welcome ───────────────── */
function Welcome({ state, dispatch }) {
  return (
    <div className="welcome">
      <div className="welcome-logo">V</div>
      <div>
        <div className="welcome-title">Welcome to <span>VoltNueronGrid</span> Studio</div>
        <div className="welcome-sub">A unified console for VoltNueronGrid clusters and Postgres replicas. Connect to a server to browse schema, run hybrid OLTP/OLAP queries, and manage users.</div>
      </div>
      <div className="welcome-cards">
        <div className="welcome-card" onClick={() => dispatch({ type: "openConnPanel", id: null })}>
          <div className="wc-icon">＋</div>
          <div className="wc-title">New Connection</div>
          <div className="wc-desc">Connect to a VNG cluster or Postgres host</div>
        </div>
        <div className="welcome-card" onClick={() => { dispatch({ type: "setActive", id: "c1" }); }}>
          <div className="wc-icon">⚡</div>
          <div className="wc-title">Quick Connect</div>
          <div className="wc-desc">Open Production VNG</div>
        </div>
        <div className="welcome-card" onClick={() => dispatch({ type: "setScreen", screen: "dashboard" })}>
          <div className="wc-icon">📊</div>
          <div className="wc-title">Cluster Dashboard</div>
          <div className="wc-desc">Live KPIs &amp; node health</div>
        </div>
      </div>
      <div className="recent-list">
        <div className="label-xs" style={{ marginBottom: 4 }}>Recent</div>
        {state.connections.map((c) => (
          <div key={c.id} className="recent-item" onClick={() => dispatch({ type: "setActive", id: c.id })}>
            <span className={`conn-dot ${c.health === "ok" ? "ok" : c.health === "error" ? "error" : "none"}`} />
            <span style={{ fontWeight: 600 }}>{c.name}</span>
            <span style={{ color: "var(--text-3)", fontSize: 11 }}>· {c.host}</span>
            <span className="recent-time">{c.serverType === "voltnuerongrid" ? "VNG" : "PG"}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ───────────────── Dashboard ───────────────── */
function Dashboard({ state }) {
  const conn = state.connections.find((c) => c.id === state.activeId);
  return (
    <div className="dashboard">
      <div className="dash-title-row">
        <div>
          <div className="dash-title">Cluster Overview</div>
          <div className="dash-sub">{conn ? `${conn.name} · ${conn.host}:${conn.port}` : "—"}</div>
        </div>
        <div className="status-cyan" style={{ fontSize: 11 }}>● Live</div>
      </div>

      <div>
        <div className="section-label">KPIs</div>
        <div className="kpi-grid">
          {[
            { v: "12.4 K", l: "Queries / min", t: "▲ 4.2%", cls: "trend-up" },
            { v: "184 ms", l: "P95 latency",   t: "▼ 12 ms", cls: "trend-up" },
            { v: "3 / 3",  l: "Nodes healthy", t: "stable",  cls: "trend-flat" },
            { v: "62%",    l: "Cache hit",     t: "▲ 1.1%",  cls: "trend-up" },
          ].map((k, i) => (
            <div key={i} className="kpi-card">
              <div className="kpi-val">{k.v}</div>
              <div className="kpi-label">{k.l}</div>
              <div className={`kpi-trend ${k.cls}`}>{k.t}</div>
            </div>
          ))}
        </div>
      </div>

      <div>
        <div className="section-label">Nodes</div>
        <div className="node-grid">
          {[
            { name: "vng-01", role: "leader",   status: "active",  cpu: 38, ram: 51 },
            { name: "vng-02", role: "follower", status: "active",  cpu: 42, ram: 48 },
            { name: "vng-03", role: "follower", status: "active",  cpu: 36, ram: 50 },
            { name: "pg-replica-01", role: "follower", status: "passive", cpu: 14, ram: 22 },
          ].map((n) => (
            <div key={n.name} className="node-card">
              <span className={`node-status-dot ${n.status}`} />
              <div className="node-info">
                <div className="node-name">{n.name}</div>
                <div className={`node-role ${n.role}`}>{n.role}</div>
                <div className="node-stats">
                  <div className="node-stat">CPU <span className="nv">{n.cpu}%</span></div>
                  <div className="node-stat">RAM <span className="nv">{n.ram}%</span></div>
                </div>
                <div className="bar-track"><div className="bar-fill bar-cpu" style={{ width: `${n.cpu}%` }} /></div>
                <div className="bar-track"><div className="bar-fill bar-ram" style={{ width: `${n.ram}%` }} /></div>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div>
        <div className="section-label">Recent Audit</div>
        <div className="audit-list">
          {[
            { kind: "sql",   actor: "analyst", action: "SELECT * FROM analytics.public.events LIMIT 100", time: "2m ago", outcome: "ok" },
            { kind: "ddl",   actor: "admin",   action: "CREATE TABLE billing.public.invoices …",          time: "11m ago",outcome: "ok" },
            { kind: "auth",  actor: "etl_bot", action: "Login from 10.0.4.12",                            time: "18m ago",outcome: "ok" },
            { kind: "admin", actor: "admin",   action: "GRANT readwrite TO analyst",                      time: "1h ago", outcome: "ok" },
            { kind: "sql",   actor: "ops",     action: "TRUNCATE TABLE analytics.public.tmp_staging",     time: "2h ago", outcome: "fail" },
          ].map((a, i) => (
            <div key={i} className="audit-row">
              <span className={`audit-kind ak-${a.kind}`}>{a.kind.toUpperCase()}</span>
              <span className="audit-actor">{a.actor}</span>
              <span className="audit-action mono">{a.action}</span>
              <span className="audit-time">{a.time}</span>
              <span className={`audit-outcome ${a.outcome === "ok" ? "ao-ok" : "ao-fail"}`}>
                {a.outcome === "ok" ? "✓" : "✕"}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

window.Workspace = Workspace;
window.RightPanel = RightPanel;
window.StatusBar = StatusBar;
window.Welcome = Welcome;
window.Dashboard = Dashboard;
