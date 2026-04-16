import { Column, Table } from "../models/Schema";
import { TableEditorCapabilities, TableEditorRow, TableEditorTarget } from "../models/TableEditor";

const BINARY_TYPES = new Set(["BYTEA"]);

export function quoteIdentifier(identifier: string): string {
  return `"${identifier.replace(/"/g, '""')}"`;
}

export function buildQualifiedTableName(target: TableEditorTarget): string {
  return `${quoteIdentifier(target.schema)}.${quoteIdentifier(target.tableName)}`;
}

export function toEditorValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "";
  }

  if (typeof value === "object") {
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }

  return String(value);
}

export function deriveTableEditorCapabilities(table: Table): TableEditorCapabilities {
  const primaryKeyColumns = table.columns.filter((column) => column.isPrimaryKey).map((column) => column.name);
  const uniqueIndexColumns = table.indexes.find((index) => index.isPrimary || index.isUnique)?.columns ?? [];
  const uniqueColumns = table.columns.filter((column) => column.isUnique).map((column) => column.name);
  const keyColumns = primaryKeyColumns.length > 0 ? primaryKeyColumns : uniqueIndexColumns.length > 0 ? uniqueIndexColumns : uniqueColumns;
  const canInsert = !table.isSystem;
  const canUpdate = canInsert && keyColumns.length > 0;
  const canDelete = canUpdate;

  return {
    canInsert,
    canUpdate,
    canDelete,
    keyColumns,
    readOnlyReason:
      keyColumns.length === 0
        ? "Inline update/delete requires a primary key or unique key. This table is currently read-only for existing rows."
        : undefined,
  };
}

export function buildSelectPageSql(target: TableEditorTarget, columns: Column[], page: number, pageSize: number): string {
  const projectedColumns = columns.map((column) => quoteIdentifier(column.name)).join(", ");
  const offset = Math.max(0, (page - 1) * pageSize);
  return `SELECT ${projectedColumns}\nFROM ${buildQualifiedTableName(target)}\nLIMIT ${pageSize + 1} OFFSET ${offset};`;
}

export function countPendingChanges(rows: TableEditorRow[], capabilities: TableEditorCapabilities): number {
  let total = 0;

  for (const row of rows) {
    if (row.kind === "draft") {
      if (hasAnyRowValue(row)) {
        total += 1;
      }
      continue;
    }

    if (row.isDeleted && capabilities.canDelete) {
      total += 1;
      continue;
    }

    if (buildChangedColumnNames(row, capabilities).length > 0) {
      total += 1;
    }
  }

  return total;
}

export function hasAnyRowValue(row: TableEditorRow): boolean {
  return Object.values(row.values).some((value) => value.trim().length > 0);
}

export function buildInsertStatement(target: TableEditorTarget, table: Table, row: TableEditorRow): string {
  const editableColumns = table.columns.filter((column) => !isBinaryColumn(column));
  const columnNames = editableColumns.map((column) => quoteIdentifier(column.name)).join(", ");
  const values = editableColumns.map((column) => encodeSqlValue(column, row.values[column.name] ?? "")).join(", ");
  return `INSERT INTO ${buildQualifiedTableName(target)} (${columnNames}) VALUES (${values});`;
}

export function buildUpdateStatement(
  target: TableEditorTarget,
  table: Table,
  row: TableEditorRow,
  capabilities: TableEditorCapabilities
): string | null {
  const changedColumns = buildChangedColumnNames(row, capabilities)
    .map((columnName) => table.columns.find((column) => column.name === columnName))
    .filter((column): column is Column => Boolean(column));

  if (changedColumns.length === 0) {
    return null;
  }

  const assignments = changedColumns
    .map((column) => `${quoteIdentifier(column.name)} = ${encodeSqlValue(column, row.values[column.name] ?? "")}`)
    .join(", ");
  const whereClause = buildWhereClause(table, row, capabilities);
  return `UPDATE ${buildQualifiedTableName(target)} SET ${assignments} WHERE ${whereClause};`;
}

export function buildDeleteStatement(
  target: TableEditorTarget,
  table: Table,
  row: TableEditorRow,
  capabilities: TableEditorCapabilities
): string {
  const whereClause = buildWhereClause(table, row, capabilities);
  return `DELETE FROM ${buildQualifiedTableName(target)} WHERE ${whereClause};`;
}

export function validateDraftRow(table: Table, row: TableEditorRow): string[] {
  const errors: string[] = [];
  for (const column of table.columns) {
    if (isBinaryColumn(column)) {
      continue;
    }

    const rawValue = row.values[column.name] ?? "";
    if (!column.nullable && !column.defaultValue && rawValue.trim().length === 0) {
      errors.push(`Column '${column.name}' is required.`);
      continue;
    }

    const validationError = validateColumnInput(column, rawValue);
    if (validationError) {
      errors.push(`Column '${column.name}': ${validationError}`);
    }
  }

  return errors;
}

export function validateColumnInput(column: Column, rawValue: string): string | undefined {
  try {
    encodeSqlValue(column, rawValue);
    return undefined;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

function buildChangedColumnNames(row: TableEditorRow, capabilities: TableEditorCapabilities): string[] {
  const originalValues = row.originalValues ?? {};
  return Object.keys(row.values).filter((columnName) => {
    if (capabilities.keyColumns.includes(columnName)) {
      return false;
    }

    return (row.values[columnName] ?? "") !== (originalValues[columnName] ?? "");
  });
}

function buildWhereClause(table: Table, row: TableEditorRow, capabilities: TableEditorCapabilities): string {
  if (capabilities.keyColumns.length === 0) {
    throw new Error(`Table '${table.schema}.${table.name}' does not expose a key for update/delete operations.`);
  }

  const originalValues = row.originalValues ?? row.values;
  return capabilities.keyColumns
    .map((columnName) => {
      const column = table.columns.find((candidate) => candidate.name === columnName);
      if (!column) {
        throw new Error(`Key column '${columnName}' is missing from table metadata.`);
      }

      return `${quoteIdentifier(columnName)} = ${encodeSqlValue(column, originalValues[columnName] ?? "")}`;
    })
    .join(" AND ");
}

export function encodeSqlValue(column: Column, rawValue: string): string {
  const trimmed = rawValue.trim();

  if (trimmed.length === 0) {
    if (column.nullable) {
      return "NULL";
    }

    if (column.defaultValue) {
      return column.defaultValue;
    }
  }

  if (trimmed.toUpperCase() === "NULL") {
    if (!column.nullable) {
      throw new Error("cannot be NULL");
    }
    return "NULL";
  }

  switch (column.type) {
    case "INT":
    case "BIGINT":
    case "SMALLINT": {
      if (!/^-?\d+$/.test(trimmed)) {
        throw new Error("must be an integer");
      }
      return trimmed;
    }
    case "DECIMAL":
    case "FLOAT":
    case "DOUBLE": {
      if (!/^-?\d+(\.\d+)?$/.test(trimmed)) {
        throw new Error("must be numeric");
      }
      return trimmed;
    }
    case "BOOLEAN": {
      const lowered = trimmed.toLowerCase();
      if (["true", "1", "yes"].includes(lowered)) {
        return "true";
      }
      if (["false", "0", "no"].includes(lowered)) {
        return "false";
      }
      throw new Error("must be true/false");
    }
    case "JSON": {
      try {
        JSON.parse(trimmed);
      } catch {
        throw new Error("must be valid JSON");
      }
      return `'${escapeSqlLiteral(trimmed)}'`;
    }
    case "DATE":
    case "TIMESTAMP":
    case "VARCHAR":
    case "TEXT":
    case "UNKNOWN":
      return `'${escapeSqlLiteral(rawValue)}'`;
    case "BYTEA":
      throw new Error("binary columns are read-only in the table editor");
    default:
      return `'${escapeSqlLiteral(rawValue)}'`;
  }
}

function escapeSqlLiteral(value: string): string {
  return value.replace(/'/g, "''");
}

function isBinaryColumn(column: Column): boolean {
  return BINARY_TYPES.has(column.type);
}