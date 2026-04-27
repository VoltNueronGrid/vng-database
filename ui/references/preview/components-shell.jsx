/* global React */
const { useState, useEffect, useRef, useMemo } = React;

/* ───────────────── Helpers ───────────────── */
function cx(...a) { return a.filter(Boolean).join(" "); }

function colTypeClass(type) {
  const t = (type || "").toUpperCase();
  if (t.includes("INT") || t.includes("FLOAT") || t.includes("DECIMAL") || t.includes("DOUBLE") || t.includes("NUM")) return "int";
  if (t.includes("BOOL")) return "bool";
  if (t.includes("DATE") || t.includes("TIME")) return "date";
  if (t.includes("JSON")) return "json";
  if (t.includes("UUID")) return "str";
  return "str";
}

function fmtRows(n) {
  if (n == null) return "—";
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(0) + "K";
  return String(n);
}

window.cx = cx;
window.colTypeClass = colTypeClass;
window.fmtRows = fmtRows;

/* ───────────────── Title Bar ───────────────── */
function TitleBar({ state, dispatch }) {
  const [themeMenu, setThemeMenu] = useState(false);
  const themeAnchor = useRef(null);

  useEffect(() => {
    if (!themeMenu) return;
    const onDown = (e) => {
      if (themeAnchor.current && !themeAnchor.current.contains(e.target)) setThemeMenu(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [themeMenu]);

  const active = state.connections.find((c) => c.id === state.activeId);
  const dot = active ? (active.health === "ok" ? "ok" : active.health === "error" ? "error" : "none") : "none";
  const themeIcons = { light: "☀", dark: "☾", system: "◐" };

  return (
    <div className="titlebar">
      <div className="titlebar-traffic">
        <button className="traffic traffic-close" />
        <button className="traffic traffic-min" />
        <button className="traffic traffic-max" />
      </div>
      <div className="titlebar-logo">
        <div className="logo-icon">V</div>
        <span className="titlebar-name"><span>Volt</span>NueronGrid Studio</span>
      </div>
      <div className="titlebar-spacer" />
      <button
        className="titlebar-conn-badge"
        onClick={() => dispatch({ type: "openConnPanel", id: state.activeId })}
      >
        <span className={`conn-badge-dot ${dot}`} />
        <span>{active ? `${active.name} · ${active.host}:${active.port}` : "No connection"}</span>
        <span style={{ color: "var(--text-3)", fontSize: 10 }}>▾</span>
      </button>
      <div className="titlebar-spacer" />
      <div className="titlebar-actions">
        {active && (
          <button className="titlebar-btn" title="Refresh schema" onClick={() => dispatch({ type: "toast", msg: "Schema refreshed" })}>↻</button>
        )}
        {active && (
          <button className="titlebar-btn" title="Disconnect" onClick={() => dispatch({ type: "setActive", id: null })}>⏻</button>
        )}
        <button className="titlebar-btn" title="Dashboard" onClick={() => dispatch({ type: "setScreen", screen: "dashboard" })}>📊</button>
        <button className="titlebar-btn" title="New Connection" onClick={() => dispatch({ type: "openConnPanel", id: null })}>＋</button>
        <div className="theme-menu-anchor" ref={themeAnchor}>
          <button
            className={cx("titlebar-btn", themeMenu && "active")}
            title={`Theme: ${state.themeMode}`}
            onClick={() => setThemeMenu((o) => !o)}
          >{themeIcons[state.themeMode]}</button>
          {themeMenu && (
            <div className="theme-menu">
              {["light", "dark", "system"].map((m) => (
                <button
                  key={m}
                  className={state.themeMode === m ? "active" : ""}
                  onClick={() => { dispatch({ type: "setTheme", mode: m }); setThemeMenu(false); }}
                >
                  <span>{themeIcons[m]}</span>
                  <span style={{ textTransform: "capitalize" }}>{m}</span>
                  <span className="check">✓</span>
                </button>
              ))}
            </div>
          )}
        </div>
        <button className="titlebar-btn" title="Settings">⚙</button>
      </div>
    </div>
  );
}

/* ───────────────── Sidebar - Connection List ───────────────── */
function ConnectionList({ state, dispatch }) {
  return (
    <div>
      <div className="conn-section-header">
        <span className="label-xs">Connections</span>
        <button className="conn-add-btn" title="Add Connection" onClick={() => dispatch({ type: "openConnPanel", id: null })}>＋</button>
      </div>
      {state.connections.map((c) => {
        const dot = c.health === "ok" ? "ok" : c.health === "error" ? "error" : "none";
        const isVng = c.serverType === "voltnuerongrid";
        return (
          <div
            key={c.id}
            className={cx("conn-item", c.id === state.activeId && "active")}
            onClick={() => dispatch({ type: "setActive", id: c.id })}
            onDoubleClick={() => dispatch({ type: "openConnPanel", id: c.id })}
            onContextMenu={(e) => dispatch({
              type: "ctxMenu",
              x: e.clientX, y: e.clientY, _e: e,
              menu: window.buildConnectionMenu(c, dispatch),
            })}
          >
            <span className={`conn-dot ${dot}`} />
            <span className="conn-item-name">{c.name}</span>
            <span className={cx("conn-type-badge", !isVng && "pg")}>
              {isVng ? "VNG" : c.serverType.toUpperCase().slice(0, 2)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

/* ───────────────── Sidebar - Schema Tree ───────────────── */
function TableNode({ table, schemaName, dbName, state, dispatch }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <div
        className="tree-node"
        onClick={() => setOpen((o) => !o)}
        onDoubleClick={() => dispatch({ type: "openTableTab", schema: schemaName, table: table.name })}
        onContextMenu={(e) => dispatch({
          type: "ctxMenu", x: e.clientX, y: e.clientY, _e: e,
          menu: window.buildTableMenu(dbName, schemaName, table, dispatch),
        })}
      >
        <span className="tree-indent" /><span className="tree-indent" /><span className="tree-indent" />
        <span className={cx("tree-chevron", open && "open")}>▶</span>
        <span className="tree-icon">📋</span>
        <span className="tree-label">{table.name}</span>
        <span className="tree-count">{fmtRows(table.row_count)}</span>
      </div>
      {open && table.columns.map((col) => (
        <div
          key={col.name}
          className="tree-node"
          onClick={() => dispatch({ type: "openRightPanel", target: `${schemaName}.${table.name}` })}
          onContextMenu={(e) => dispatch({
            type: "ctxMenu", x: e.clientX, y: e.clientY, _e: e,
            menu: window.buildColumnMenu(dbName, schemaName, table.name, col, dispatch),
          })}
        >
          <span className="tree-indent" /><span className="tree-indent" /><span className="tree-indent" /><span className="tree-indent" />
          <span className="tree-chevron" style={{ visibility: "hidden" }}>▶</span>
          {col.primary_key
            ? <span className="pk-marker" title="PK">🔑</span>
            : <span style={{ width: 14 }} />}
          <span className="tree-label mono" style={{ fontSize: 11 }}>{col.name}</span>
          <span className={`col-chip ${colTypeClass(col.data_type)}`}>{col.data_type}</span>
        </div>
      ))}
    </>
  );
}

function SchemaNode({ ns, dbName, state, dispatch }) {
  const [open, setOpen] = useState(true);
  return (
    <>
      <div
        className="tree-node"
        onClick={() => setOpen((o) => !o)}
        onContextMenu={(e) => dispatch({
          type: "ctxMenu", x: e.clientX, y: e.clientY, _e: e,
          menu: window.buildSchemaMenu(dbName, ns.name, dispatch),
        })}
      >
        <span className="tree-indent" /><span className="tree-indent" />
        <span className={cx("tree-chevron", open && "open")}>▶</span>
        <span className="tree-icon">📁</span>
        <span className="tree-label">{ns.name}</span>
        <span className="tree-count">{ns.tables.length}</span>
      </div>
      {open && ns.tables.map((t) => (
        <TableNode key={t.name} table={t} schemaName={ns.name} dbName={dbName} state={state} dispatch={dispatch} />
      ))}
    </>
  );
}

function DatabaseNode({ db, state, dispatch }) {
  const [open, setOpen] = useState(true);
  return (
    <>
      <div
        className="tree-node"
        onClick={() => setOpen((o) => !o)}
        onContextMenu={(e) => dispatch({
          type: "ctxMenu", x: e.clientX, y: e.clientY, _e: e,
          menu: window.buildDatabaseMenu(db.name, dispatch),
        })}
      >
        <span className="tree-indent" />
        <span className={cx("tree-chevron", open && "open")}>▶</span>
        <span className="tree-icon">🗄</span>
        <span className="tree-label">{db.name}</span>
        <span className="tree-badge">{db.schemas.length} schemas</span>
      </div>
      {open && db.schemas.map((ns) => (
        <SchemaNode key={ns.name} ns={ns} dbName={db.name} state={state} dispatch={dispatch} />
      ))}
    </>
  );
}

function SchemaTree({ state, dispatch }) {
  if (!state.activeId) {
    return <div style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>Connect to a server to browse schema.</div>;
  }
  return (
    <div>
      {window.SAMPLE_SCHEMA.databases.map((db) => (
        <DatabaseNode key={db.name} db={db} state={state} dispatch={dispatch} />
      ))}
    </div>
  );
}

/* ───────────────── Sidebar - Users Panel ───────────────── */
function UsersPanel({ state, dispatch }) {
  const roleStyles = {
    dba:       { bg: "#ef444411", fg: "var(--red)",        bd: "#ef444433" },
    operator:  { bg: "#9333ea11", fg: "#c084fc",           bd: "#9333ea33" },
    readwrite: { bg: "#3b82f611", fg: "var(--blue)",       bd: "#3b82f633" },
    readonly:  { bg: "#22c55e11", fg: "var(--green)",      bd: "#22c55e33" },
  };
  return (
    <div>
      <div className="conn-section-header">
        <span className="label-xs">Users</span>
        <button className="conn-add-btn" onClick={() => dispatch({ type: "openModal", kind: "create-user" })}>＋</button>
      </div>
      {window.SAMPLE_USERS.map((u) => {
        const s = roleStyles[u.role] || roleStyles.readonly;
        return (
          <div
            key={u.id}
            className="conn-item"
            onContextMenu={(e) => dispatch({
              type: "ctxMenu", x: e.clientX, y: e.clientY, _e: e,
              menu: window.buildUserMenu(u.username, dispatch),
            })}
          >
            <span className={`conn-dot ${u.active ? "ok" : "none"}`} />
            <span className="conn-item-name">{u.username}</span>
            <span className="conn-type-badge" style={{ background: s.bg, color: s.fg, borderColor: s.bd }}>{u.role}</span>
          </div>
        );
      })}
      <div className="conn-section-header" style={{ marginTop: 14 }}>
        <span className="label-xs">Roles</span>
        <button className="conn-add-btn" onClick={() => dispatch({ type: "openModal", kind: "create-role" })}>＋</button>
      </div>
      {["dba", "operator", "readwrite", "readonly"].map((r) => (
        <div key={r} className="conn-item" style={{ cursor: "default" }}>
          <span className="tree-icon">🛡</span>
          <span className="conn-item-name">{r}</span>
          <span className="tree-count">{window.SAMPLE_USERS.filter((u) => u.role === r).length}</span>
        </div>
      ))}
      <div style={{ padding: 12, fontSize: 10.5, color: "var(--text-3)", lineHeight: 1.5 }}>
        Right-click a user to manage. User management is local-only until server admin endpoints are wired.
      </div>
    </div>
  );
}

/* ───────────────── Sidebar Shell ───────────────── */
function Sidebar({ state, dispatch }) {
  return (
    <div className="sidebar">
      <div className="sidebar-activity">
        {[
          ["connections", "Schema"],
          ["users", "Users"],
          ["history", "History"],
          ["saved", "Saved"],
        ].map(([id, label]) => (
          <button
            key={id}
            className={cx("activity-btn", state.sidebarTab === id && "active")}
            onClick={() => dispatch({ type: "setSidebarTab", tab: id })}
          >{label}</button>
        ))}
      </div>
      <div className="sidebar-scroll">
        {state.sidebarTab === "connections" && (<>
          <ConnectionList state={state} dispatch={dispatch} />
          <SchemaTree state={state} dispatch={dispatch} />
        </>)}
        {state.sidebarTab === "users" && <UsersPanel state={state} dispatch={dispatch} />}
        {state.sidebarTab === "history" && <div style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>Query history — coming soon.</div>}
        {state.sidebarTab === "saved" && <div style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>Saved queries — coming soon.</div>}
      </div>
    </div>
  );
}

window.TitleBar = TitleBar;
window.Sidebar = Sidebar;
