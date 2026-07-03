import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Screen = "welcome" | "main" | "dashboard";
export type SidebarTab = "connections" | "databases" | "users" | "history" | "saved";

interface UiState {
  screen: Screen;
  sidebarTab: SidebarTab;
  sidebarWidth: number;
  sidebarPinned: boolean;
  connectionPanelOpen: boolean;
  editingConnectionId: string | null;
  rightPanelOpen: boolean;
  rightPanelTable: string | null; // "schema.table"
  settingsPanelOpen: boolean;

  setScreen(s: Screen): void;
  setSidebarWidth(width: number): void;
  setSidebarPinned(pinned: boolean): void;
  toggleSidebarPinned(): void;
  openConnectionPanel(id?: string | null): void;
  closeConnectionPanel(): void;
  setSidebarTab(t: SidebarTab): void;
  openRightPanel(table: string): void;
  closeRightPanel(): void;
  openSettings(): void;
  closeSettings(): void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      screen: "welcome",
      sidebarTab: "connections",
      sidebarWidth: 300,
      sidebarPinned: true,
      connectionPanelOpen: false,
      editingConnectionId: null,
      rightPanelOpen: false,
      rightPanelTable: null,
      settingsPanelOpen: false,

      setScreen(s) {
        set({ screen: s });
      },

      setSidebarWidth(width) {
        const clamped = Math.min(560, Math.max(220, Math.round(width)));
        set({ sidebarWidth: clamped });
      },

      setSidebarPinned(pinned) {
        set({ sidebarPinned: pinned });
      },

      toggleSidebarPinned() {
        set((s) => ({ sidebarPinned: !s.sidebarPinned }));
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

      openSettings() {
        set({ settingsPanelOpen: true });
      },

      closeSettings() {
        set({ settingsPanelOpen: false });
      },
    }),
    {
      name: "vng-studio-ui",
      partialize: (s) => ({
        sidebarTab: s.sidebarTab,
        sidebarWidth: s.sidebarWidth,
        sidebarPinned: s.sidebarPinned,
      }),
    }
  )
);
