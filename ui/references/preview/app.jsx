/* global React, ReactDOM */
const { useReducer, useEffect } = React;

const initial = {
  themeMode: "dark",
  resolved: "dark",

  screen: "welcome",
  sidebarTab: "connections",

  connections: window.SAMPLE_CONNECTIONS,
  activeId: null,

  connPanelOpen: false,
  editingConnId: null,

  rightPanelOpen: false,
  rightPanelTarget: null,

  tabs: [],
  activeTabId: null,
  nextTabId: 1,

  modal: null,         // { kind, target, payload }
  ctxMenu: null,       // { x, y, items, title }
  toast: null,         // string
};

function detectSystemTheme() {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function reducer(s, a) {
  switch (a.type) {
    case "setTheme": {
      const resolved = a.mode === "system" ? detectSystemTheme() : a.mode;
      return { ...s, themeMode: a.mode, resolved };
    }
    case "cycleTheme": {
      const order = ["light", "dark", "system"];
      const next = order[(order.indexOf(s.themeMode) + 1) % order.length];
      const resolved = next === "system" ? detectSystemTheme() : next;
      return { ...s, themeMode: next, resolved };
    }

    case "setScreen": return { ...s, screen: a.screen };
    case "setSidebarTab": return { ...s, sidebarTab: a.tab };

    case "setActive": {
      if (a.id == null) {
        return { ...s, activeId: null, screen: "welcome", tabs: [], activeTabId: null, rightPanelOpen: false };
      }
      return { ...s, activeId: a.id, screen: "main" };
    }

    case "openConnPanel": return { ...s, connPanelOpen: true, editingConnId: a.id ?? null };
    case "closeConnPanel": return { ...s, connPanelOpen: false, editingConnId: null };

    case "openRightPanel": return { ...s, rightPanelOpen: true, rightPanelTarget: a.target };
    case "closeRightPanel": return { ...s, rightPanelOpen: false, rightPanelTarget: null };

    case "openTableTab": {
      const name = `${a.schema}.${a.table}`;
      const existing = s.tabs.find((t) => t.kind === "table" && t.name === name);
      if (existing) return { ...s, activeTabId: existing.id, screen: "main" };
      const id = `t-${s.nextTabId}`;
      const tab = {
        id, kind: "table", name,
        sql: `SELECT * FROM ${name}\nLIMIT 100;`,
        dirty: false, lastRoute: "olap",
      };
      return { ...s, tabs: [...s.tabs, tab], activeTabId: id, nextTabId: s.nextTabId + 1, screen: "main" };
    }

    case "openSqlTab": {
      const id = `t-${s.nextTabId}`;
      const tab = {
        id, kind: "sql", name: a.name || "untitled.sql",
        sql: a.sql || "",
        dirty: true, lastRoute: "oltp",
      };
      return { ...s, tabs: [...s.tabs, tab], activeTabId: id, nextTabId: s.nextTabId + 1, screen: "main" };
    }

    case "setActiveTab": return { ...s, activeTabId: a.id };
    case "closeTab": {
      const tabs = s.tabs.filter((t) => t.id !== a.id);
      const activeTabId = s.activeTabId === a.id ? (tabs[tabs.length - 1]?.id ?? null) : s.activeTabId;
      return { ...s, tabs, activeTabId };
    }
    case "updateSql": return {
      ...s,
      tabs: s.tabs.map((t) => t.id === a.id ? { ...t, sql: a.sql, dirty: true } : t),
    };
    case "runQuery": return { ...s, toast: "Query executed · 184ms · 25 rows" };

    case "openModal": return {
      ...s,
      modal: { kind: a.kind, target: a.target, payload: a.payload },
      ctxMenu: null,
    };
    case "closeModal": return { ...s, modal: null };

    case "ctxMenu": {
      if (a._e) { a._e.preventDefault(); a._e.stopPropagation(); }
      return { ...s, ctxMenu: { x: a.x, y: a.y, ...a.menu } };
    }
    case "closeCtxMenu": return { ...s, ctxMenu: null };

    case "toast": return { ...s, toast: a.msg };
    case "clearToast": return { ...s, toast: null };

    default: return s;
  }
}

function App() {
  const [state, dispatch] = useReducer(reducer, initial);
  window.__APP_STATE = state;

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", state.resolved);
    document.documentElement.style.colorScheme = state.resolved;
  }, [state.resolved]);

  useEffect(() => {
    if (state.themeMode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const handler = () => dispatch({ type: "setTheme", mode: "system" });
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, [state.themeMode]);

  useEffect(() => {
    if (!state.toast) return;
    const id = setTimeout(() => dispatch({ type: "clearToast" }), 2400);
    return () => clearTimeout(id);
  }, [state.toast]);

  return (
    <div className="app">
      <window.TitleBar state={state} dispatch={dispatch} />

      {state.screen === "welcome" && <window.Welcome state={state} dispatch={dispatch} />}

      {(state.screen === "main" || state.screen === "dashboard") && (
        <div className="main-layout">
          <window.Sidebar state={state} dispatch={dispatch} />
          {state.screen === "main" && <window.Workspace state={state} dispatch={dispatch} />}
          {state.screen === "main" && state.rightPanelOpen && <window.RightPanel state={state} dispatch={dispatch} />}
          {state.screen === "dashboard" && <window.Dashboard state={state} dispatch={dispatch} />}
        </div>
      )}

      {state.screen !== "welcome" && <window.StatusBar state={state} dispatch={dispatch} />}

      {state.connPanelOpen && <window.ConnectionPanel state={state} dispatch={dispatch} />}
      {state.modal && <window.ResourceModal ctx={state.modal} dispatch={dispatch} />}
      {state.ctxMenu && <window.ContextMenu menu={state.ctxMenu} dispatch={dispatch} />}

      {state.toast && (
        <div style={{
          position: "fixed", bottom: 36, left: "50%", transform: "translateX(-50%)",
          background: "var(--bg-3)", border: "1px solid var(--border-strong)",
          padding: "10px 18px", borderRadius: 99, fontSize: 12.5,
          color: "var(--text-1)", boxShadow: "var(--shadow-menu)",
          zIndex: 3000, display: "flex", alignItems: "center", gap: 8,
        }}>
          <span className="status-cyan">●</span> {state.toast}
        </div>
      )}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
