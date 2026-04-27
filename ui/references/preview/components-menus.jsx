/* global React */
const { useState: useStateW } = React;

/* ───────────────── Context-menu builders (mirror src/components/ContextMenu/menus.ts) ───────────────── */

window.buildConnectionMenu = function(conn, dispatch) {
  const isActive = conn.id && window.__APP_STATE && window.__APP_STATE.activeId === conn.id;
  return {
    title: conn.name,
    items: [
      { id: "connect", label: isActive ? "Reconnect" : "Connect", icon: "⚡",
        onSelect: () => dispatch({ type: "setActive", id: conn.id }) },
      { id: "disconnect", label: "Disconnect", icon: "⏻", disabled: !isActive,
        onSelect: () => dispatch({ type: "setActive", id: null }) },
      { separator: true },
      { id: "refresh", label: "Refresh Schema", icon: "↻", shortcut: "F5", disabled: !isActive,
        onSelect: () => dispatch({ type: "toast", msg: "Schema refreshed" }) },
      { id: "test", label: "Test Connection", icon: "✓",
        onSelect: () => dispatch({ type: "toast", msg: "Connection OK · 12ms" }) },
      { separator: true },
      { id: "edit", label: "Edit Connection…", icon: "✎",
        onSelect: () => dispatch({ type: "openConnPanel", id: conn.id }) },
      { id: "duplicate", label: "Duplicate", icon: "⎘",
        onSelect: () => dispatch({ type: "toast", msg: "Connection duplicated" }) },
      { separator: true },
      { id: "newdb", label: "New Database…", icon: "＋", disabled: !isActive,
        onSelect: () => dispatch({ type: "openModal", kind: "create-database" }) },
      { id: "newuser", label: "New User…", icon: "👤", disabled: !isActive,
        onSelect: () => dispatch({ type: "openModal", kind: "create-user" }) },
      { separator: true },
      { id: "remove", label: "Remove Connection", icon: "🗑", danger: true,
        onSelect: () => dispatch({ type: "toast", msg: "(Demo) Connection would be removed" }) },
    ],
  };
};

window.buildDatabaseMenu = function(dbName, dispatch) {
  return {
    title: dbName,
    items: [
      { id: "use", label: "Set as Active Database", icon: "★",
        onSelect: () => dispatch({ type: "toast", msg: `Active DB: ${dbName}` }) },
      { separator: true },
      { id: "newschema", label: "New Schema…", icon: "＋",
        onSelect: () => dispatch({ type: "openModal", kind: "create-schema", target: dbName }) },
      { id: "newtable", label: "New Table…", icon: "📋",
        onSelect: () => dispatch({ type: "openModal", kind: "create-table", target: dbName }) },
      { separator: true },
      { id: "ddl", label: "View DDL", icon: "{ }",
        onSelect: () => dispatch({ type: "openModal", kind: "view-ddl", target: dbName }) },
      { id: "rename", label: "Rename…", icon: "✎",
        onSelect: () => dispatch({ type: "openModal", kind: "rename-table", target: dbName }) },
      { separator: true },
      { id: "drop", label: "Drop Database…", icon: "🗑", danger: true,
        onSelect: () => dispatch({ type: "openModal", kind: "drop-database", target: dbName }) },
    ],
  };
};

window.buildSchemaMenu = function(dbName, schemaName, dispatch) {
  const target = `${dbName}.${schemaName}`;
  return {
    title: target,
    items: [
      { id: "newtable", label: "New Table…", icon: "＋",
        onSelect: () => dispatch({ type: "openModal", kind: "create-table", target }) },
      { id: "ddl", label: "View DDL", icon: "{ }",
        onSelect: () => dispatch({ type: "openModal", kind: "view-ddl", target }) },
      { separator: true },
      { id: "drop", label: "Drop Schema…", icon: "🗑", danger: true,
        onSelect: () => dispatch({ type: "openModal", kind: "drop-schema", target }) },
    ],
  };
};

window.buildTableMenu = function(dbName, schemaName, table, dispatch) {
  const target = `${dbName}.${schemaName}.${table.name}`;
  return {
    title: table.name,
    items: [
      { id: "open", label: "Open Table", icon: "👁", shortcut: "↵",
        onSelect: () => dispatch({ type: "openTableTab", schema: schemaName, table: table.name }) },
      { id: "select100", label: "SELECT * LIMIT 100", icon: "⌕",
        onSelect: () => dispatch({ type: "openTableTab", schema: schemaName, table: table.name }) },
      { id: "selectcount", label: "SELECT COUNT(*)", icon: "Σ",
        onSelect: () => dispatch({ type: "openSqlTab", sql: `SELECT COUNT(*) FROM ${schemaName}.${table.name};`, name: `count_${table.name}.sql` }) },
      { separator: true },
      { id: "insert", label: "Generate INSERT…", icon: "＋", submenu: [
        { id: "i1", label: "Single row template", icon: "·",
          onSelect: () => dispatch({ type: "openSqlTab", sql: `INSERT INTO ${schemaName}.${table.name} (${table.columns.map((c) => c.name).join(", ")})\nVALUES (${table.columns.map(() => "?").join(", ")});`, name: `insert_${table.name}.sql` }) },
        { id: "i2", label: "UPDATE template", icon: "·",
          onSelect: () => dispatch({ type: "openSqlTab", sql: `UPDATE ${schemaName}.${table.name}\nSET ...\nWHERE ...;`, name: `update_${table.name}.sql` }) },
        { id: "i3", label: "DELETE template", icon: "·",
          onSelect: () => dispatch({ type: "openSqlTab", sql: `DELETE FROM ${schemaName}.${table.name}\nWHERE ...;`, name: `delete_${table.name}.sql` }) },
      ]},
      { separator: true },
      { id: "details", label: "Show Details", icon: "ℹ",
        onSelect: () => dispatch({ type: "openRightPanel", target: `${schemaName}.${table.name}` }) },
      { id: "ddl", label: "View DDL", icon: "{ }",
        onSelect: () => dispatch({ type: "openModal", kind: "view-ddl", target, payload: { kind: "table", table } }) },
      { id: "analyze", label: "Analyze Table", icon: "📊",
        onSelect: () => dispatch({ type: "openSqlTab", sql: `ANALYZE TABLE ${schemaName}.${table.name};`, name: `analyze_${table.name}.sql` }) },
      { separator: true },
      { id: "rename", label: "Rename…", icon: "✎",
        onSelect: () => dispatch({ type: "openModal", kind: "rename-table", target }) },
      { id: "truncate", label: "Truncate Table…", icon: "⌫", danger: true,
        onSelect: () => dispatch({ type: "openModal", kind: "truncate-table", target }) },
      { id: "drop", label: "Drop Table…", icon: "🗑", danger: true,
        onSelect: () => dispatch({ type: "openModal", kind: "drop-table", target }) },
    ],
  };
};

window.buildColumnMenu = function(dbName, schemaName, tableName, col, dispatch) {
  const target = `${dbName}.${schemaName}.${tableName}.${col.name}`;
  return {
    title: `${col.name} : ${col.data_type}`,
    items: [
      { id: "filter", label: `Filter by ${col.name}`, icon: "⌕",
        onSelect: () => dispatch({ type: "openSqlTab", sql: `SELECT *\nFROM ${schemaName}.${tableName}\nWHERE ${col.name} = ?;`, name: `where_${col.name}.sql` }) },
      { id: "groupby", label: `GROUP BY ${col.name}`, icon: "⌗",
        onSelect: () => dispatch({ type: "openSqlTab", sql: `SELECT ${col.name}, COUNT(*)\nFROM ${schemaName}.${tableName}\nGROUP BY ${col.name};`, name: `groupby_${col.name}.sql` }) },
      { separator: true },
      { id: "edit", label: "Edit Column…", icon: "✎",
        onSelect: () => dispatch({ type: "openModal", kind: "edit-column", target, payload: { col } }) },
      { id: "drop", label: "Drop Column…", icon: "🗑", danger: true,
        onSelect: () => dispatch({ type: "openModal", kind: "drop-column", target, payload: { col } }) },
    ],
  };
};

window.buildUserMenu = function(username, dispatch) {
  return {
    title: username,
    items: [
      { id: "edit", label: "Edit User…", icon: "✎",
        onSelect: () => dispatch({ type: "openModal", kind: "create-user", target: username, payload: { edit: true } }) },
      { id: "grant", label: "Grant Role…", icon: "🛡",
        onSelect: () => dispatch({ type: "openModal", kind: "grant-role", target: username }) },
      { id: "resetpw", label: "Reset Password…", icon: "🔑",
        onSelect: () => dispatch({ type: "openSqlTab", sql: `ALTER USER ${username} WITH PASSWORD '<new-password>';`, name: `reset_${username}.sql` }) },
      { separator: true },
      { id: "drop", label: "Drop User…", icon: "🗑", danger: true,
        onSelect: () => dispatch({ type: "openModal", kind: "drop-user", target: username }) },
    ],
  };
};

/* ───────────────── Context Menu component ───────────────── */
function ContextMenu({ menu, dispatch }) {
  const ref = React.useRef(null);
  const [openSub, setOpenSub] = useStateW(null);
  React.useEffect(() => {
    const close = () => dispatch({ type: "closeCtxMenu" });
    const onDown = (e) => { if (ref.current && !ref.current.contains(e.target)) close(); };
    const onKey = (e) => { if (e.key === "Escape") close(); };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", close, { passive: true });
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("wheel", close);
    };
  }, [dispatch]);

  if (!menu) return null;
  const x = Math.min(menu.x, window.innerWidth - 250);
  const y = Math.min(menu.y, window.innerHeight - 100);

  function select(item) {
    if (item.disabled || item.separator || item.submenu) return;
    item.onSelect && item.onSelect();
    dispatch({ type: "closeCtxMenu" });
  }

  return (
    <div ref={ref} className="ctx-menu" style={{ left: x, top: y }}>
      {menu.title && <div className="ctx-menu-title">{menu.title}</div>}
      {menu.items.map((it, i) => it.separator
        ? <div key={`s${i}`} className="ctx-menu-sep" />
        : (
          <div
            key={it.id}
            className={window.cx("ctx-menu-item", it.disabled && "disabled", it.danger && "danger")}
            onMouseEnter={() => setOpenSub(it.submenu ? it.id : null)}
            onClick={() => select(it)}
          >
            <span className="ctx-menu-icon">{it.icon || ""}</span>
            <span className="ctx-menu-label">{it.label}</span>
            {it.shortcut && <span className="ctx-menu-shortcut">{it.shortcut}</span>}
            {it.submenu && <span className="ctx-menu-arrow">▸</span>}
            {it.submenu && openSub === it.id && (
              <div className="ctx-menu submenu" style={{ left: "100%", top: -4 }}>
                {it.submenu.map((sub) => (
                  <div
                    key={sub.id}
                    className={window.cx("ctx-menu-item", sub.disabled && "disabled", sub.danger && "danger")}
                    onClick={(e) => { e.stopPropagation(); sub.onSelect && sub.onSelect(); dispatch({ type: "closeCtxMenu" }); }}
                  >
                    <span className="ctx-menu-icon">{sub.icon || ""}</span>
                    <span className="ctx-menu-label">{sub.label}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )
      )}
    </div>
  );
}

window.ContextMenu = ContextMenu;
