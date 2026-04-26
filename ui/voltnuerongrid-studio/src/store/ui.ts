import { create } from "zustand";

export type Screen = "welcome" | "main" | "dashboard";
export type SidebarTab = "connections" | "history" | "saved";

interface UiState {
  screen: Screen;
  sidebarTab: SidebarTab;
  connectionPanelOpen: boolean;
  editingConnectionId: string | null;
  rightPanelOpen: boolean;
  rightPanelTable: string | null; // "schema.table"

  setScreen(s: Screen): void;
  openConnectionPanel(id?: string | null): void;
  closeConnectionPanel(): void;
  setSidebarTab(t: SidebarTab): void;
  openRightPanel(table: string): void;
  closeRightPanel(): void;
}

export const useUiStore = create<UiState>()((set) => ({
  screen: "welcome",
  sidebarTab: "connections",
  connectionPanelOpen: false,
  editingConnectionId: null,
  rightPanelOpen: false,
  rightPanelTable: null,

  setScreen(s) {
    set({ screen: s });
  },

  openConnectionPanel(id = null) {
    set({ connectionPanelOpen: true, editingConnectionId: id ?? null });
  },

  closeConnectionPanel() {
    set({ connectionPanelOpen: false, editingConnectionId: null });
  },

  setSidebarTab(t) {
    set({ sidebarTab: t });
  },

  openRightPanel(table) {
    set({ rightPanelOpen: true, rightPanelTable: table });
  },

  closeRightPanel() {
    set({ rightPanelOpen: false, rightPanelTable: null });
  },
}));
