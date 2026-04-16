/**
 * Database schema models: Database, Schema, Table, Column
 */

export type ColumnType =
  | "INT"
  | "BIGINT"
  | "SMALLINT"
  | "DECIMAL"
  | "FLOAT"
  | "DOUBLE"
  | "VARCHAR"
  | "TEXT"
  | "BOOLEAN"
  | "DATE"
  | "TIMESTAMP"
  | "JSON"
  | "BYTEA"
  | "UNKNOWN";

export interface Column {
  name: string;
  type: ColumnType;
  nullable: boolean;
  isPrimaryKey: boolean;
  isUnique: boolean;
  isForeignKey: boolean;
  defaultValue?: string;
  comment?: string;
}

export interface Table {
  name: string;
  schema: string;
  columns: Column[];
  indexes: Index[];
  comment?: string;
  rowCount?: number;
  isSystem?: boolean;
}

export interface Index {
  name: string;
  columns: string[];
  isUnique: boolean;
  isPrimary: boolean;
}

export interface Schema {
  name: string;
  database: string;
  tables: Table[];
}

export interface Database {
  name: string;
  schemas: Schema[];
}

export interface SchemaRegistry {
  databases: Database[];
  timestamp: number;
}

/**
 * Parse column type string to ColumnType
 */
export function parseColumnType(typeStr: string): ColumnType {
  const normalized = typeStr.toUpperCase().trim();
  if (normalized.includes("BIGINT")) return "BIGINT";
  if (normalized.includes("SMALLINT")) return "SMALLINT";
  if (normalized.includes("INT")) return "INT";
  if (normalized.includes("DECIMAL") || normalized.includes("NUMERIC")) return "DECIMAL";
  if (normalized.includes("FLOAT")) return "FLOAT";
  if (normalized.includes("DOUBLE")) return "DOUBLE";
  if (normalized.includes("VARCHAR") || normalized.includes("STRING")) return "VARCHAR";
  if (normalized.includes("TEXT")) return "TEXT";
  if (normalized.includes("BOOL")) return "BOOLEAN";
  if (normalized.includes("DATE")) return "DATE";
  if (normalized.includes("TIMESTAMP")) return "TIMESTAMP";
  if (normalized.includes("JSON")) return "JSON";
  if (normalized.includes("BYTE")) return "BYTEA";
  return "UNKNOWN";
}

/**
 * Get column type icon/display
 */
export function getColumnTypeDisplay(type: ColumnType): { icon: string; label: string } {
  const map: Record<ColumnType, { icon: string; label: string }> = {
    INT: { icon: "$(symbol-number)", label: "INT" },
    BIGINT: { icon: "$(symbol-number)", label: "BIGINT" },
    SMALLINT: { icon: "$(symbol-number)", label: "SMALLINT" },
    DECIMAL: { icon: "$(symbol-number)", label: "DECIMAL" },
    FLOAT: { icon: "$(symbol-number)", label: "FLOAT" },
    DOUBLE: { icon: "$(symbol-number)", label: "DOUBLE" },
    VARCHAR: { icon: "$(symbol-string)", label: "VARCHAR" },
    TEXT: { icon: "$(symbol-string)", label: "TEXT" },
    BOOLEAN: { icon: "$(symbol-boolean)", label: "BOOLEAN" },
    DATE: { icon: "$(symbol-misc)", label: "DATE" },
    TIMESTAMP: { icon: "$(symbol-misc)", label: "TIMESTAMP" },
    JSON: { icon: "$(json)", label: "JSON" },
    BYTEA: { icon: "$(file-binary)", label: "BYTEA" },
    UNKNOWN: { icon: "$(symbol-misc)", label: "UNKNOWN" },
  };
  return map[type] || map.UNKNOWN;
}
