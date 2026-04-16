import { Connection, QueryResult, Table } from "../models";
import { TableEditorRow, TableEditorSession, TableEditorTarget } from "../models/TableEditor";
import { HttpClient } from "./HttpClient";
import { SchemaManager } from "./SchemaManager";
import {
  buildDeleteStatement,
  buildInsertStatement,
  buildSelectPageSql,
  buildUpdateStatement,
  countPendingChanges,
  deriveTableEditorCapabilities,
  hasAnyRowValue,
  toEditorValue,
  validateColumnInput,
  validateDraftRow,
} from "./TableEditorSql";
import { parseQueryResult } from "../models/QueryResult";

export class TableEditorService {
  constructor(
    private readonly httpClient: HttpClient,
    private readonly schemaManager: SchemaManager
  ) {}

  async openSession(
    connection: Connection,
    target: TableEditorTarget,
    page: number = 1,
    pageSize: number = 50,
    infoMessage?: string
  ): Promise<TableEditorSession> {
    const table = await this.schemaManager.getTable(connection, target.database, target.schema, target.tableName);
    if (!table) {
      throw new Error(`Table '${target.schema}.${target.tableName}' was not found in the schema registry.`);
    }

    return this.loadPage(connection, target, table, page, pageSize, infoMessage);
  }

  updateCell(session: TableEditorSession, rowId: string, columnName: string, value: string): TableEditorSession {
    const updatedRows = session.rows.map((row) => (row.rowId === rowId ? { ...row, values: { ...row.values, [columnName]: value } } : row));
    return this.withRows(session, updatedRows);
  }

  addDraftRow(session: TableEditorSession): TableEditorSession {
    const values = Object.fromEntries(session.columns.map((column) => [column.name, ""]));
    const row: TableEditorRow = {
      rowId: `draft-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      kind: "draft",
      values,
      isDeleted: false,
    };

    return this.withRows(session, [row, ...session.rows], "Draft row added.");
  }

  toggleDeleteRow(session: TableEditorSession, rowId: string): TableEditorSession {
    const nextRows: TableEditorRow[] = [];

    for (const row of session.rows) {
      if (row.rowId !== rowId) {
        nextRows.push(row);
        continue;
      }

      if (row.kind === "draft") {
        continue;
      }

      nextRows.push({ ...row, isDeleted: !row.isDeleted });
    }

    return this.withRows(session, nextRows);
  }

  async discardChanges(connection: Connection, session: TableEditorSession): Promise<TableEditorSession> {
    return this.openSession(connection, session.target, session.page, session.pageSize, "Changes discarded.");
  }

  async changePage(connection: Connection, session: TableEditorSession, direction: "next" | "previous"): Promise<TableEditorSession> {
    const nextPage = direction === "next" ? session.page + 1 : session.page - 1;
    if (nextPage < 1) {
      return session;
    }

    return this.openSession(connection, session.target, nextPage, session.pageSize);
  }

  async saveSession(connection: Connection, session: TableEditorSession): Promise<TableEditorSession> {
    const preflightSession = this.withRows(session, session.rows);
    if (this.hasCellErrors(preflightSession)) {
      return {
        ...preflightSession,
        errorMessage: "Fix validation errors before saving changes.",
        infoMessage: undefined,
      };
    }

    const statements = this.buildStatements(preflightSession);
    if (statements.length === 0) {
      return {
        ...preflightSession,
        pendingSaveSql: undefined,
        partialSave: undefined,
        infoMessage: "No changes to save.",
        errorMessage: undefined,
      };
    }

    for (let index = 0; index < statements.length; index += 1) {
      const statement = statements[index];
      const result = await this.executeStatement(connection, statement);
      if (result.status !== "success") {
        const message = result.error?.message ?? "Save failed.";
        return {
          ...preflightSession,
          errorMessage: `${message} ${index} of ${statements.length} change(s) were applied before failure.`,
          infoMessage: undefined,
          pendingSaveSql: statements.slice(index),
          partialSave: {
            applied: index,
            total: statements.length,
            failedAt: index + 1,
          },
        };
      }
    }

    return this.openSession(connection, session.target, session.page, session.pageSize, `Saved ${statements.length} change(s).`);
  }

  private async loadPage(
    connection: Connection,
    target: TableEditorTarget,
    table: Table,
    page: number,
    pageSize: number,
    infoMessage?: string
  ): Promise<TableEditorSession> {
    const sql = buildSelectPageSql(target, table.columns, page, pageSize);
    const result = await this.executeStatement(connection, sql);

    if (result.status !== "success") {
      throw new Error(result.error?.message ?? `Failed to load rows for ${target.schema}.${target.tableName}.`);
    }

    const pageRows = result.rows.slice(0, pageSize);
    return {
      target,
      table,
      columns: table.columns,
      capabilities: deriveTableEditorCapabilities(table),
      rows: pageRows.map((row, index) => this.toTableEditorRow(row, table, page, index)),
      page,
      pageSize,
      hasNextPage: result.rows.length > pageSize,
      dirty: false,
      infoMessage,
      errorMessage: undefined,
      cellErrors: undefined,
      pendingSaveSql: undefined,
      partialSave: undefined,
    };
  }

  private buildStatements(session: TableEditorSession): string[] {
    const statements: string[] = [];

    for (const row of session.rows) {
      if (row.kind === "draft") {
        if (!hasAnyRowValue(row)) {
          continue;
        }

        if (!session.capabilities.canInsert) {
          throw new Error("This table does not allow inserting new rows from the editor.");
        }

        const validationErrors = validateDraftRow(session.table, row);
        if (validationErrors.length > 0) {
          throw new Error(validationErrors.join("\n"));
        }

        statements.push(buildInsertStatement(session.target, session.table, row));
        continue;
      }

      if (row.isDeleted) {
        if (!session.capabilities.canDelete) {
          throw new Error("Delete requires a primary key or unique key.");
        }

        statements.push(buildDeleteStatement(session.target, session.table, row, session.capabilities));
        continue;
      }

      const updateStatement = buildUpdateStatement(session.target, session.table, row, session.capabilities);
      if (updateStatement) {
        if (!session.capabilities.canUpdate) {
          throw new Error("Update requires a primary key or unique key.");
        }
        statements.push(updateStatement);
      }
    }

    return statements;
  }

  private async executeStatement(connection: Connection, sql: string): Promise<QueryResult> {
    const startedAt = Date.now();
    const response = await this.httpClient.executeQuery(connection, sql, {
      requestId: `table-editor-${startedAt}`,
      timeoutMs: connection.settings.advanced.connectionTimeout ?? 30000,
    });
    const executionTime = Date.now() - startedAt;

    if (response.status === 200) {
      const result = parseQueryResult(sql, response.data, executionTime);
      result.id = `table-editor-${startedAt}`;
      return result;
    }

    return {
      id: `table-editor-${startedAt}`,
      query: sql,
      status: "error",
      rows: [],
      columns: [],
      rowCount: 0,
      executionTime,
      timestamp: Date.now(),
      error: {
        message: response.error || `Server returned status ${response.status}`,
        code: String(response.status),
      },
    };
  }

  private toTableEditorRow(row: Record<string, unknown>, table: Table, page: number, index: number): TableEditorRow {
    const values = Object.fromEntries(table.columns.map((column) => [column.name, toEditorValue(row[column.name])])) as Record<string, string>;

    return {
      rowId: `existing-${page}-${index}`,
      kind: "existing",
      values,
      originalValues: { ...values },
      isDeleted: false,
    };
  }

  private withRows(session: TableEditorSession, rows: TableEditorRow[], infoMessage?: string): TableEditorSession {
    const cellErrors = this.collectCellErrors(session, rows);
    return {
      ...session,
      rows,
      dirty: countPendingChanges(rows, session.capabilities) > 0,
      cellErrors: Object.keys(cellErrors).length > 0 ? cellErrors : undefined,
      pendingSaveSql: undefined,
      partialSave: undefined,
      infoMessage,
      errorMessage: undefined,
    };
  }

  private collectCellErrors(
    session: TableEditorSession,
    rows: TableEditorRow[]
  ): Record<string, Record<string, string>> {
    const errorsByRow: Record<string, Record<string, string>> = {};

    for (const row of rows) {
      if (row.isDeleted) {
        continue;
      }

      for (const column of session.columns) {
        const isKeyColumn = session.capabilities.keyColumns.includes(column.name);
        const skipValidation = row.kind === "existing" && isKeyColumn;
        if (skipValidation) {
          continue;
        }

        const rawValue = row.values[column.name] ?? "";
        const message = validateColumnInput(column, rawValue);
        if (!message) {
          continue;
        }

        if (!errorsByRow[row.rowId]) {
          errorsByRow[row.rowId] = {};
        }
        errorsByRow[row.rowId][column.name] = message;
      }
    }

    for (const row of rows) {
      if (row.kind !== "draft" || row.isDeleted || !hasAnyRowValue(row)) {
        continue;
      }

      const draftErrors = validateDraftRow(session.table, row);
      for (const draftError of draftErrors) {
        const match = /^Column '([^']+)':\s*(.+)$/.exec(draftError);
        if (!match) {
          continue;
        }

        const [, columnName, message] = match;
        if (!errorsByRow[row.rowId]) {
          errorsByRow[row.rowId] = {};
        }
        errorsByRow[row.rowId][columnName] = message;
      }
    }

    return errorsByRow;
  }

  private hasCellErrors(session: TableEditorSession): boolean {
    if (!session.cellErrors) {
      return false;
    }

    return Object.values(session.cellErrors).some((rowErrors) => Object.keys(rowErrors).length > 0);
  }
}

export function createTableEditorService(httpClient: HttpClient, schemaManager: SchemaManager): TableEditorService {
  return new TableEditorService(httpClient, schemaManager);
}