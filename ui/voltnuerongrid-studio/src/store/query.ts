import { create } from "zustand";
import type { RoutePath } from "@/api/studio-client";

export interface ResultColumn {
  name: string;
  type: string;
}

export interface QueryResult {
  tabId: string;
  status: string;
  routePath: RoutePath;
  elapsedMs: number;
  rejectedCount: number;
  transactionId?: string;
  columns: ResultColumn[];
  rows: Array<Record<string, unknown>>;
  rowCount: number;
  error: string | null;
  executedAt: number;
}

interface QueryState {
  results: Record<string, QueryResult>;
  executing: Set<string>;

  setResult(r: QueryResult): void;
  setExecuting(tabId: string, v: boolean): void;
  getResult(tabId: string): QueryResult | null;
  isExecuting(tabId: string): boolean;
}

export const useQueryStore = create<QueryState>()((set, get) => ({
  results: {},
  executing: new Set<string>(),

  setResult(r) {
    set((s) => ({ results: { ...s.results, [r.tabId]: r } }));
  },

  setExecuting(tabId, v) {
    set((s) => {
      const next = new Set(s.executing);
      v ? next.add(tabId) : next.delete(tabId);
      return { executing: next };
    });
  },

  getResult(tabId) {
    return get().results[tabId] ?? null;
  },

  isExecuting(tabId) {
    return get().executing.has(tabId);
  },
}));
