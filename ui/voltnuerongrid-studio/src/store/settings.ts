/**
 * Studio-wide global settings store (persisted per-user in localStorage).
 *
 * Settings are not connection-specific — they apply across all databases and
 * connections for the current browser / desktop user.
 */
import { create } from "zustand";
import { persist } from "zustand/middleware";

// ─── Schema-object double-click action ───────────────────────────────────────

/** What happens when the user double-clicks a schema object (view, trigger, …). */
export type DdlDoubleClickAction =
  | "open_tab"       // Open the DDL in a new unsaved SQL editor tab (default)
  | "copy_clipboard"; // Copy DDL text to the clipboard silently

// ─── Default query limit ─────────────────────────────────────────────────────

export type TimeDisplayUnit = "auto" | "ms" | "s";

// ─── M-6: Per-connection defaults ────────────────────────────────────────────

/**
 * ACID isolation level sent with every SQL execute request.
 * Maps to the server's `isolation_level` field in SqlExecuteRequest.
 */
export type IsolationLevel =
  | "read_committed"    // Default — no dirty reads, but non-repeatable reads possible
  | "repeatable_read"   // Snapshot at BEGIN; no phantom reads within the transaction
  | "serializable";     // Full SSI (row-level OCC); highest consistency, more aborts

// ─── Full settings shape ──────────────────────────────────────────────────────

export interface StudioSettings {
  /**
   * What to do when the user double-clicks a schema object in the sidebar.
   * @default "open_tab"
   */
  ddlDoubleClickAction: DdlDoubleClickAction;

  /**
   * Default row limit applied to queries that do not include an explicit LIMIT.
   * @default 1000
   */
  defaultQueryLimit: number;

  /**
   * How to display query elapsed time in the results toolbar.
   * "auto" picks the best unit automatically (µs / ms / s).
   * @default "auto"
   */
  timeDisplayUnit: TimeDisplayUnit;

  /**
   * Whether to confirm before closing an unsaved SQL tab.
   * @default true
   */
  confirmUnsavedClose: boolean;

  // ── M-6: Connection defaults ──────────────────────────────────────────────

  /**
   * Default ACID isolation level passed with every SQL execute request.
   * @default "read_committed"
   */
  defaultIsolationLevel: IsolationLevel;

  /**
   * Client-side statement timeout hint (milliseconds, 0 = no limit).
   * Sent to the server as `statement_timeout_ms` in SqlExecuteRequest.
   * @default 0
   */
  statementTimeoutMs: number;
}

// ─── Store interface ──────────────────────────────────────────────────────────

interface SettingsState extends StudioSettings {
  /** Replace one or more settings fields. */
  update(patch: Partial<StudioSettings>): void;
  /** Reset all settings to their defaults. */
  reset(): void;
}

const DEFAULTS: StudioSettings = {
  ddlDoubleClickAction: "open_tab",
  defaultQueryLimit: 1000,
  timeDisplayUnit: "auto",
  confirmUnsavedClose: true,
  defaultIsolationLevel: "read_committed",
  statementTimeoutMs: 0,
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      ...DEFAULTS,

      update(patch) {
        set((s) => ({ ...s, ...patch }));
      },

      reset() {
        set({ ...DEFAULTS });
      },
    }),
    {
      name: "vng-studio-settings", // localStorage key
      version: 1,
    }
  )
);
