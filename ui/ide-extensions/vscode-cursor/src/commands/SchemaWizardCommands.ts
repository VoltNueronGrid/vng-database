import * as vscode from "vscode";
import { Connection, ColumnType } from "../models";
import { SchemaTreeItem } from "../providers/DatabaseExplorerProvider";
import { SchemaManager } from "../services";
import { quoteIdentifier } from "../services/TableEditorSql";

interface CreateColumnDraft {
  name: string;
  type: ColumnType;
  nullable: boolean;
  isPrimaryKey: boolean;
  isUnique: boolean;
  defaultValue?: string;
}

interface CreateTableDraft {
  schema: string;
  tableName: string;
  columns: CreateColumnDraft[];
}

type AlterTableOperation =
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

const COLUMN_TYPES: ColumnType[] = [
  "INT",
  "BIGINT",
  "SMALLINT",
  "DECIMAL",
  "FLOAT",
  "DOUBLE",
  "VARCHAR",
  "TEXT",
  "BOOLEAN",
  "DATE",
  "TIMESTAMP",
  "JSON",
];

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

export async function runCreateTableWizard(
  connection: Connection,
  schemaManager: SchemaManager,
  executeSql: (sql: string, operation: string) => Promise<void>,
  element?: SchemaTreeItem
): Promise<void> {
  const registries = await schemaManager.getSchemaRegistry(connection, false);
  if (registries.databases.length === 0) {
    vscode.window.showWarningMessage("No databases available for schema management.");
    return;
  }

  const selectedDatabase =
    element?.type === "schema" ? (element.data as { database: string }).database : registries.databases[0].name;

  const schemas = registries.databases.find((database) => database.name === selectedDatabase)?.schemas ?? [];
  if (schemas.length === 0) {
    vscode.window.showWarningMessage(`Database '${selectedDatabase}' has no schemas.`);
    return;
  }

  const schemaDefault =
    element?.type === "schema" ? (element.data as { schema: { name: string } }).schema.name : schemas[0].name;
  const schemaName = await pickSchemaName(schemas.map((schema) => schema.name), schemaDefault, "Create Table Wizard: choose schema");
  if (!schemaName) {
    return;
  }

  const tableName = await promptIdentifier("Create Table Wizard: table name", "Enter table name", "orders");
  if (!tableName) {
    return;
  }

  const columnCountRaw = await vscode.window.showInputBox({
    title: "Create Table Wizard: number of columns",
    value: "3",
    prompt: "How many columns should be created?",
    ignoreFocusOut: true,
    validateInput: (value) => {
      const parsed = Number(value);
      if (!Number.isInteger(parsed) || parsed < 1 || parsed > 30) {
        return "Enter an integer between 1 and 30.";
      }
      return undefined;
    },
  });
  if (!columnCountRaw) {
    return;
  }

  const columns: CreateColumnDraft[] = [];
  const columnCount = Number(columnCountRaw);
  for (let index = 0; index < columnCount; index += 1) {
    const defaultName = index === 0 ? "id" : `column_${index + 1}`;
    const name = await promptIdentifier(`Create Table Wizard: column ${index + 1}`, "Column name", defaultName);
    if (!name) {
      return;
    }

    const typePick = await vscode.window.showQuickPick(
      COLUMN_TYPES.map((type) => ({ label: type })),
      {
        title: `Create Table Wizard: column ${name} type`,
        canPickMany: false,
      }
    );
    if (!typePick) {
      return;
    }

    const nullable = await pickBoolean(`Create Table Wizard: column ${name} nullable`, true);
    if (nullable === undefined) {
      return;
    }

    const isPrimaryKey = await pickBoolean(`Create Table Wizard: column ${name} primary key`, index === 0);
    if (isPrimaryKey === undefined) {
      return;
    }

    const isUnique = isPrimaryKey
      ? true
      : (await pickBoolean(`Create Table Wizard: column ${name} unique`, false)) ?? false;

    const defaultValue = await vscode.window.showInputBox({
      title: `Create Table Wizard: column ${name} default value`,
      prompt: "Optional SQL default expression (leave empty for none)",
      ignoreFocusOut: true,
    });

    columns.push({
      name,
      type: typePick.label as ColumnType,
      nullable,
      isPrimaryKey,
      isUnique,
      defaultValue: defaultValue?.trim() || undefined,
    });
  }

  const duplicateColumn = findDuplicateColumn(columns);
  if (duplicateColumn) {
    vscode.window.showErrorMessage(`Column '${duplicateColumn}' appears more than once.`);
    return;
  }

  const ddl = buildCreateTableDDL({
    schema: schemaName,
    tableName,
    columns,
  });

  await previewAndDispatchDdl(ddl, "Create Table Wizard DDL", executeSql, "Create Table Wizard");
}

export async function runAlterTableWizard(
  connection: Connection,
  schemaManager: SchemaManager,
  executeSql: (sql: string, operation: string) => Promise<void>,
  element?: SchemaTreeItem
): Promise<void> {
  const tableTarget = await resolveTableTarget(connection, schemaManager, element);
  if (!tableTarget) {
    return;
  }

  const operationPick = await vscode.window.showQuickPick(
    [
      { label: "Add Column", value: "addColumn" as const },
      { label: "Rename Table", value: "renameTable" as const },
    ],
    {
      title: "Alter Table Wizard: choose operation",
      canPickMany: false,
    }
  );
  if (!operationPick) {
    return;
  }

  if (operationPick.value === "renameTable") {
    const newTableName = await promptIdentifier("Alter Table Wizard: rename table", "Enter new table name", `${tableTarget.tableName}_new`);
    if (!newTableName) {
      return;
    }

    const ddl = buildAlterTableDDL({
      kind: "renameTable",
      schema: tableTarget.schema,
      tableName: tableTarget.tableName,
      newTableName,
    });
    await previewAndDispatchDdl(ddl, "Alter Table Wizard DDL", executeSql, "Alter Table Wizard");
    return;
  }

  const columnName = await promptIdentifier("Alter Table Wizard: add column", "New column name", "new_column");
  if (!columnName) {
    return;
  }

  const typePick = await vscode.window.showQuickPick(
    COLUMN_TYPES.map((type) => ({ label: type })),
    {
      title: `Alter Table Wizard: column ${columnName} type`,
      canPickMany: false,
    }
  );
  if (!typePick) {
    return;
  }

  const nullable = await pickBoolean(`Alter Table Wizard: column ${columnName} nullable`, true);
  if (nullable === undefined) {
    return;
  }

  const defaultValue = await vscode.window.showInputBox({
    title: `Alter Table Wizard: column ${columnName} default value`,
    prompt: "Optional SQL default expression (leave empty for none)",
    ignoreFocusOut: true,
  });

  const ddl = buildAlterTableDDL({
    kind: "addColumn",
    schema: tableTarget.schema,
    tableName: tableTarget.tableName,
    column: {
      name: columnName,
      type: typePick.label as ColumnType,
      nullable,
      isPrimaryKey: false,
      isUnique: false,
      defaultValue: defaultValue?.trim() || undefined,
    },
  });
  await previewAndDispatchDdl(ddl, "Alter Table Wizard DDL", executeSql, "Alter Table Wizard");
}

async function resolveTableTarget(
  connection: Connection,
  schemaManager: SchemaManager,
  element?: SchemaTreeItem
): Promise<{ schema: string; tableName: string } | undefined> {
  if (element?.type === "table") {
    const payload = element.data as { schema: string; table: { name: string } };
    return {
      schema: payload.schema,
      tableName: payload.table.name,
    };
  }

  const registry = await schemaManager.getSchemaRegistry(connection, false);
  const tableItems = registry.databases.flatMap((database) =>
    database.schemas.flatMap((schema) =>
      schema.tables
        .filter((table) => !table.isSystem)
        .map((table) => ({
          label: table.name,
          description: schema.name,
          detail: database.name,
          schema: schema.name,
          tableName: table.name,
        }))
    )
  );

  if (tableItems.length === 0) {
    vscode.window.showWarningMessage("No editable tables found in schema registry.");
    return undefined;
  }

  const picked = await vscode.window.showQuickPick(tableItems, {
    title: "Alter Table Wizard: choose table",
    canPickMany: false,
    placeHolder: "Select table",
  });

  if (!picked) {
    return undefined;
  }

  return {
    schema: picked.schema,
    tableName: picked.tableName,
  };
}

async function previewAndDispatchDdl(
  ddl: string,
  title: string,
  executeSql: (sql: string, operation: string) => Promise<void>,
  operation: string
): Promise<void> {
  const choice = await vscode.window.showQuickPick(
    [
      { label: "Open DDL in SQL Editor", value: "open" },
      { label: "Copy DDL to Clipboard", value: "copy" },
      { label: "Execute DDL Now", value: "execute" },
    ],
    {
      title,
      canPickMany: false,
      placeHolder: "Choose how to continue",
      ignoreFocusOut: true,
    }
  );

  if (!choice) {
    return;
  }

  if (choice.value === "copy") {
    await vscode.env.clipboard.writeText(ddl);
    vscode.window.showInformationMessage("DDL copied to clipboard.");
    return;
  }

  if (choice.value === "open") {
    const document = await vscode.workspace.openTextDocument({
      language: "sql",
      content: ddl,
    });
    await vscode.window.showTextDocument(document, vscode.ViewColumn.Active);
    return;
  }

  await executeSql(ddl, operation);
}

function findDuplicateColumn(columns: CreateColumnDraft[]): string | undefined {
  const seen = new Set<string>();
  for (const column of columns) {
    const normalized = column.name.toLowerCase();
    if (seen.has(normalized)) {
      return column.name;
    }
    seen.add(normalized);
  }

  return undefined;
}

async function promptIdentifier(title: string, prompt: string, value: string): Promise<string | undefined> {
  return vscode.window.showInputBox({
    title,
    prompt,
    value,
    ignoreFocusOut: true,
    validateInput: (input) => {
      const trimmed = input.trim();
      if (!trimmed) {
        return "Value is required.";
      }
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(trimmed)) {
        return "Use letters, numbers, and underscore only; cannot start with a number.";
      }
      return undefined;
    },
  });
}

async function pickSchemaName(
  schemaNames: string[],
  defaultSchema: string,
  title: string
): Promise<string | undefined> {
  const picks = schemaNames.map((schemaName) => ({
    label: schemaName,
    description: schemaName === defaultSchema ? "default" : undefined,
  }));
  const picked = await vscode.window.showQuickPick(picks, {
    title,
    canPickMany: false,
    placeHolder: "Select schema",
  });

  return picked?.label;
}

async function pickBoolean(title: string, defaultValue: boolean): Promise<boolean | undefined> {
  const picked = await vscode.window.showQuickPick(
    [
      { label: "Yes", value: true, description: defaultValue ? "default" : undefined },
      { label: "No", value: false, description: !defaultValue ? "default" : undefined },
    ],
    {
      title,
      canPickMany: false,
    }
  );

  return picked?.value;
}