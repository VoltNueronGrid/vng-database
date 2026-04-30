import { create } from "zustand";

export type TabType = "sql" | "table" | "dashboard";

export interface Tab {
  id: string;
  type: TabType;
  title: string;
  sql: string;
  tableName?: string;
  isDirty: boolean;
  filePath?: string;
}

function newSqlTab(n: number): Tab {
  return {
    id: `tab-${Date.now()}-${n}`,
    type: "sql",
    title: `query_${n}.sql`,
    sql: "",
    isDirty: false,
  };
}

interface EditorState {
  tabs: Tab[];
  activeTabId: string | null;
  tabCounter: number;

  openSqlTab(sql?: string, title?: string): string;
  openTableTab(tableName: string, schema: string): string;
  openDashboardTab(): void;
  closeTab(id: string): void;
  setActiveTab(id: string): void;
  updateSql(tabId: string, sql: string): void;
  setFilePath(tabId: string, path: string): void;
  markSaved(tabId: string): void;

  getActiveTab(): Tab | null;

  /** Ref to the active Monaco editor instance — set by SqlEditorPane on mount. */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  editorInstance: any | null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  setEditorInstance(inst: any | null): void;
  /** Returns the currently selected text if non-empty, otherwise null. */
  getSelectedSql(): string | null;
  /** Insert text at the current cursor position in the active Monaco editor. */
  insertTextIntoActiveTab(text: string): void;
}

export const useEditorStore = create<EditorState>()((set, get) => {
  const firstTab = newSqlTab(1);
  return {
    tabs: [firstTab],
    activeTabId: firstTab.id,
    tabCounter: 1,

    openSqlTab(sql = "", title?: string) {
      const { tabCounter } = get();
      const n = tabCounter + 1;
      const tab: Tab = {
        id: `tab-${Date.now()}`,
        type: "sql",
        title: title ?? `query_${n}.sql`,
        sql,
        isDirty: false,
      };
      set((s) => ({
        tabs: [...s.tabs, tab],
        activeTabId: tab.id,
        tabCounter: n,
      }));
      return tab.id;
    },

    openTableTab(tableName, schema) {
      const existing = get().tabs.find(
        (t) => t.type === "table" && t.tableName === `${schema}.${tableName}`
      );
      if (existing) {
        set({ activeTabId: existing.id });
        return existing.id;
      }
      const tab: Tab = {
        id: `tab-table-${tableName}`,
        type: "table",
        title: tableName,
        sql: `SELECT *\nFROM   ${schema}.${tableName}\nLIMIT  100;`,
        tableName: `${schema}.${tableName}`,
        isDirty: false,
      };
      set((s) => ({ tabs: [...s.tabs, tab], activeTabId: tab.id }));
      return tab.id;
    },

    openDashboardTab() {
      const existing = get().tabs.find((t) => t.type === "dashboard");
      if (existing) {
        set({ activeTabId: existing.id });
        return;
      }
      const tab: Tab = {
        id: "tab-dashboard",
        type: "dashboard",
        title: "Dashboard",
        sql: "",
        isDirty: false,
      };
      set((s) => ({ tabs: [...s.tabs, tab], activeTabId: tab.id }));
    },

    closeTab(id) {
      const { tabs, activeTabId } = get();
      if (tabs.length === 1) return; // keep at least one tab
      const idx = tabs.findIndex((t) => t.id === id);
      const next =
        activeTabId === id
          ? (tabs[idx + 1] ?? tabs[idx - 1])?.id ?? null
          : activeTabId;
      set({ tabs: tabs.filter((t) => t.id !== id), activeTabId: next });
    },

    setActiveTab(id) {
      set({ activeTabId: id });
    },

    updateSql(tabId, sql) {
      set((s) => ({
        tabs: s.tabs.map((t) =>
          t.id === tabId ? { ...t, sql, isDirty: true } : t
        ),
      }));
    },

    setFilePath(tabId, path) {
      const name = path.split(/[\\/]/).pop() ?? path;
      set((s) => ({
        tabs: s.tabs.map((t) =>
          t.id === tabId ? { ...t, filePath: path, title: name } : t
        ),
      }));
    },

    markSaved(tabId) {
      set((s) => ({
        tabs: s.tabs.map((t) =>
          t.id === tabId ? { ...t, isDirty: false } : t
        ),
      }));
    },

    getActiveTab() {
      const { tabs, activeTabId } = get();
      return tabs.find((t) => t.id === activeTabId) ?? null;
    },

    editorInstance: null,
    setEditorInstance(inst) {
      set({ editorInstance: inst });
    },
    getSelectedSql() {
      const inst = get().editorInstance;
      if (!inst) return null;
      const selection = inst.getSelection?.();
      if (!selection) return null;
      const model = inst.getModel?.();
      if (!model) return null;
      const text = model.getValueInRange(selection);
      return text && text.trim() ? text : null;
    },
    insertTextIntoActiveTab(text: string) {
      const inst = get().editorInstance;
      if (!inst) return;
      // Execute an edit at the current cursor position
      const selection = inst.getSelection?.();
      if (!selection) return;
      inst.executeEdits?.("", [{ range: selection, text, forceMoveMarkers: true }]);
      inst.focus?.();
    },
  };
});
