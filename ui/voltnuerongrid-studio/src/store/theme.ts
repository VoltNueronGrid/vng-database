import { create } from "zustand";
import { persist } from "zustand/middleware";

export type ThemeMode = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

interface ThemeState {
  mode: ThemeMode;
  resolved: ResolvedTheme;

  setMode(m: ThemeMode): void;
  cycleMode(): void;

  /** Internal: re-evaluate `resolved` from `mode` + system preference. */
  applyResolved(): void;
}

function detectSystem(): ResolvedTheme {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

function applyToDom(resolved: ResolvedTheme) {
  if (typeof document === "undefined") return;
  document.documentElement.setAttribute("data-theme", resolved);
  document.documentElement.style.colorScheme = resolved;
}

export const useThemeStore = create<ThemeState>()(
  persist(
    (set, get) => ({
      mode: "system",
      resolved: detectSystem(),

      setMode(m) {
        set({ mode: m });
        get().applyResolved();
      },

      cycleMode() {
        const order: ThemeMode[] = ["light", "dark", "system"];
        const i = order.indexOf(get().mode);
        get().setMode(order[(i + 1) % order.length]);
      },

      applyResolved() {
        const m = get().mode;
        const resolved: ResolvedTheme = m === "system" ? detectSystem() : m;
        set({ resolved });
        applyToDom(resolved);
      },
    }),
    {
      name: "vng-studio-theme",
      partialize: (s) => ({ mode: s.mode }),
      onRehydrateStorage: () => (state) => {
        // Re-evaluate after rehydration so DOM matches stored mode.
        state?.applyResolved();
      },
    }
  )
);

/** Call once at app boot to listen for OS theme changes when mode = "system". */
export function initThemeWatcher(): () => void {
  if (typeof window === "undefined") return () => {};
  // Apply once synchronously in case persist hasn't rehydrated yet.
  useThemeStore.getState().applyResolved();

  const mq = window.matchMedia("(prefers-color-scheme: light)");
  const handler = () => {
    if (useThemeStore.getState().mode === "system") {
      useThemeStore.getState().applyResolved();
    }
  };
  mq.addEventListener("change", handler);
  return () => mq.removeEventListener("change", handler);
}
