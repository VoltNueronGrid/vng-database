/**
 * Database lifecycle store — first-class CREATE/DROP DATABASE.
 *
 * Phase 1.3. Lives separately from the connection store because the user may
 * have many connections to the same server but the database list is a property
 * of the *server* (active connection's backend), not the connection itself.
 *
 * The store does **not** persist the list to localStorage — it is always
 * fetched fresh from the active connection on demand. This avoids stale state
 * after another operator creates/drops a database from a different client.
 */
import { create } from "zustand";
import type { DatabaseRecord } from "@/api/studio-client";

export type LoadStatus = "idle" | "loading" | "ok" | "error";

interface DatabasesState {
  databases: DatabaseRecord[];
  status: LoadStatus;
  error: string | null;
  /** Currently-selected database (used by the Sidebar to filter the schema tree). */
  selectedName: string | null;

  setDatabases(next: DatabaseRecord[]): void;
  setStatus(s: LoadStatus, error?: string | null): void;
  selectDatabase(name: string | null): void;
  /** Optimistic insertion — used by the create dialog so the new DB appears
      immediately while the next refresh confirms it. */
  upsertDatabase(record: DatabaseRecord): void;
  removeDatabase(name: string): void;
}

export const useDatabasesStore = create<DatabasesState>()((set) => ({
  databases: [],
  status: "idle",
  error: null,
  selectedName: null,

  setDatabases(next) {
    set({ databases: [...next].sort((a, b) => a.name.localeCompare(b.name)) });
  },

  setStatus(s, error = null) {
    set({ status: s, error });
  },

  selectDatabase(name) {
    set({ selectedName: name });
  },

  upsertDatabase(record) {
    set((state) => {
      const existing = state.databases.findIndex((d) => d.name === record.name);
      const next =
        existing >= 0
          ? state.databases.map((d, i) => (i === existing ? record : d))
          : [...state.databases, record];
      return { databases: next.sort((a, b) => a.name.localeCompare(b.name)) };
    });
  },

  removeDatabase(name) {
    set((state) => ({
      databases: state.databases.filter((d) => d.name !== name),
      selectedName: state.selectedName === name ? null : state.selectedName,
    }));
  },
}));
