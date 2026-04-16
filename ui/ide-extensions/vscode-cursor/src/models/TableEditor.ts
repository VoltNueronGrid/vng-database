import { Column, Table } from "./Schema";

export interface TableEditorTarget {
  database: string;
  schema: string;
  tableName: string;
}

export interface TableEditorCapabilities {
  canInsert: boolean;
  canUpdate: boolean;
  canDelete: boolean;
  keyColumns: string[];
  readOnlyReason?: string;
}

export interface TableEditorRow {
  rowId: string;
  kind: "existing" | "draft";
  values: Record<string, string>;
  originalValues?: Record<string, string>;
  isDeleted: boolean;
}

export interface TableEditorSession {
  target: TableEditorTarget;
  table: Table;
  columns: Column[];
  capabilities: TableEditorCapabilities;
  rows: TableEditorRow[];
  page: number;
  pageSize: number;
  hasNextPage: boolean;
  dirty: boolean;
  infoMessage?: string;
  errorMessage?: string;
  cellErrors?: Record<string, Record<string, string>>;
  pendingSaveSql?: string[];
  partialSave?: {
    applied: number;
    total: number;
    failedAt: number;
  };
}