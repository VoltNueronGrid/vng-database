import { ColumnType } from "../models";
import { quoteIdentifier } from "../services/TableEditorSql";

export interface CreateColumnDraft {
  name: string;
  type: ColumnType;
  nullable: boolean;
  isPrimaryKey: boolean;
  isUnique: boolean;
  defaultValue?: string;
}

export interface CreateTableDraft {
  schema: string;
  tableName: string;
  columns: CreateColumnDraft[];
}

export type AlterTableOperation =
  | {
      kind: "addColumn";
      schema: string;
      tableName: string;
      column: CreateColumnDraft;
    }
  | {
      kind: "renameTable";
      schema: string;
      tableName: string;
      newTableName: string;
    };

export function buildCreateTableDDL(draft: CreateTableDraft): string {
  const definitions = draft.columns
    .map((column) => {
      let definition = `${quoteIdentifier(column.name)} ${column.type}`;
      if (!column.nullable) {
        definition += " NOT NULL";
      }
      if (column.isPrimaryKey) {
        definition += " PRIMARY KEY";
      }
      if (column.isUnique && !column.isPrimaryKey) {
        definition += " UNIQUE";
      }
      if (column.defaultValue && column.defaultValue.trim().length > 0) {
        definition += ` DEFAULT ${column.defaultValue.trim()}`;
      }
      return definition;
    })
    .join(",\n  ");

  return `CREATE TABLE ${quoteIdentifier(draft.schema)}.${quoteIdentifier(draft.tableName)} (\n  ${definitions}\n);`;
}

export function buildAlterTableDDL(operation: AlterTableOperation): string {
  if (operation.kind === "renameTable") {
    return `ALTER TABLE ${quoteIdentifier(operation.schema)}.${quoteIdentifier(operation.tableName)} RENAME TO ${quoteIdentifier(operation.newTableName)};`;
  }

  let columnDefinition = `${quoteIdentifier(operation.column.name)} ${operation.column.type}`;
  if (!operation.column.nullable) {
    columnDefinition += " NOT NULL";
  }
  if (operation.column.defaultValue && operation.column.defaultValue.trim().length > 0) {
    columnDefinition += ` DEFAULT ${operation.column.defaultValue.trim()}`;
  }

  return `ALTER TABLE ${quoteIdentifier(operation.schema)}.${quoteIdentifier(operation.tableName)} ADD COLUMN ${columnDefinition};`;
}
