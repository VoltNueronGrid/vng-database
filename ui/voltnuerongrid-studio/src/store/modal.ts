import { create } from "zustand";

export type ResourceModalKind =
  | "create-database"
  | "drop-database"
  | "create-schema"
  | "drop-schema"
  | "create-table"
  | "drop-table"
  | "truncate-table"
  | "rename-table"
  | "edit-column"
  | "drop-column"
  | "create-user"
  | "drop-user"
  | "create-role"
  | "grant-role"
  | "view-ddl";

export interface ResourceModalContext {
  kind: ResourceModalKind;
  /** Resource path in dot notation: db, db.schema, db.schema.table, etc. */
  target?: string;
  /** Optional pre-filled DDL or extra payload (used by view-ddl, edit-column). */
  payload?: Record<string, unknown>;
}

interface ModalState {
  current: ResourceModalContext | null;
  open(ctx: ResourceModalContext): void;
  close(): void;
}

export const useModalStore = create<ModalState>()((set) => ({
  current: null,
  open(ctx) {
    set({ current: ctx });
  },
  close() {
    set({ current: null });
  },
}));
