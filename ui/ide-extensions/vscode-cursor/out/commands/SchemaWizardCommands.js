"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.buildCreateTableDDL = exports.buildAlterTableDDL = void 0;
exports.runCreateTableWizard = runCreateTableWizard;
exports.runAlterTableWizard = runAlterTableWizard;
const vscode = __importStar(require("vscode"));
const SchemaWizardSql_1 = require("./SchemaWizardSql");
Object.defineProperty(exports, "buildAlterTableDDL", { enumerable: true, get: function () { return SchemaWizardSql_1.buildAlterTableDDL; } });
Object.defineProperty(exports, "buildCreateTableDDL", { enumerable: true, get: function () { return SchemaWizardSql_1.buildCreateTableDDL; } });
const COLUMN_TYPES = [
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
async function runCreateTableWizard(connection, schemaManager, executeSql, element) {
    const registries = await schemaManager.getSchemaRegistry(connection, false);
    if (registries.databases.length === 0) {
        vscode.window.showWarningMessage("No databases available for schema management.");
        return;
    }
    const selectedDatabase = element?.type === "schema" ? element.data.database : registries.databases[0].name;
    const schemas = registries.databases.find((database) => database.name === selectedDatabase)?.schemas ?? [];
    if (schemas.length === 0) {
        vscode.window.showWarningMessage(`Database '${selectedDatabase}' has no schemas.`);
        return;
    }
    const schemaDefault = element?.type === "schema" ? element.data.schema.name : schemas[0].name;
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
    const columns = [];
    const columnCount = Number(columnCountRaw);
    for (let index = 0; index < columnCount; index += 1) {
        const defaultName = index === 0 ? "id" : `column_${index + 1}`;
        const name = await promptIdentifier(`Create Table Wizard: column ${index + 1}`, "Column name", defaultName);
        if (!name) {
            return;
        }
        const typePick = await vscode.window.showQuickPick(COLUMN_TYPES.map((type) => ({ label: type })), {
            title: `Create Table Wizard: column ${name} type`,
            canPickMany: false,
        });
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
            type: typePick.label,
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
    const ddl = (0, SchemaWizardSql_1.buildCreateTableDDL)({
        schema: schemaName,
        tableName,
        columns,
    });
    await previewAndDispatchDdl(ddl, "Create Table Wizard DDL", executeSql, "Create Table Wizard");
}
async function runAlterTableWizard(connection, schemaManager, executeSql, element) {
    const tableTarget = await resolveTableTarget(connection, schemaManager, element);
    if (!tableTarget) {
        return;
    }
    const operationPick = await vscode.window.showQuickPick([
        { label: "Add Column", value: "addColumn" },
        { label: "Rename Table", value: "renameTable" },
    ], {
        title: "Alter Table Wizard: choose operation",
        canPickMany: false,
    });
    if (!operationPick) {
        return;
    }
    if (operationPick.value === "renameTable") {
        const newTableName = await promptIdentifier("Alter Table Wizard: rename table", "Enter new table name", `${tableTarget.tableName}_new`);
        if (!newTableName) {
            return;
        }
        const ddl = (0, SchemaWizardSql_1.buildAlterTableDDL)({
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
    const typePick = await vscode.window.showQuickPick(COLUMN_TYPES.map((type) => ({ label: type })), {
        title: `Alter Table Wizard: column ${columnName} type`,
        canPickMany: false,
    });
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
    const ddl = (0, SchemaWizardSql_1.buildAlterTableDDL)({
        kind: "addColumn",
        schema: tableTarget.schema,
        tableName: tableTarget.tableName,
        column: {
            name: columnName,
            type: typePick.label,
            nullable,
            isPrimaryKey: false,
            isUnique: false,
            defaultValue: defaultValue?.trim() || undefined,
        },
    });
    await previewAndDispatchDdl(ddl, "Alter Table Wizard DDL", executeSql, "Alter Table Wizard");
}
async function resolveTableTarget(connection, schemaManager, element) {
    if (element?.type === "table") {
        const payload = element.data;
        return {
            schema: payload.schema,
            tableName: payload.table.name,
        };
    }
    const registry = await schemaManager.getSchemaRegistry(connection, false);
    const tableItems = registry.databases.flatMap((database) => database.schemas.flatMap((schema) => schema.tables
        .filter((table) => !table.isSystem)
        .map((table) => ({
        label: table.name,
        description: schema.name,
        detail: database.name,
        schema: schema.name,
        tableName: table.name,
    }))));
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
async function previewAndDispatchDdl(ddl, title, executeSql, operation) {
    const choice = await vscode.window.showQuickPick([
        { label: "Open DDL in SQL Editor", value: "open" },
        { label: "Copy DDL to Clipboard", value: "copy" },
        { label: "Execute DDL Now", value: "execute" },
    ], {
        title,
        canPickMany: false,
        placeHolder: "Choose how to continue",
        ignoreFocusOut: true,
    });
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
function findDuplicateColumn(columns) {
    const seen = new Set();
    for (const column of columns) {
        const normalized = column.name.toLowerCase();
        if (seen.has(normalized)) {
            return column.name;
        }
        seen.add(normalized);
    }
    return undefined;
}
async function promptIdentifier(title, prompt, value) {
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
async function pickSchemaName(schemaNames, defaultSchema, title) {
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
async function pickBoolean(title, defaultValue) {
    const picked = await vscode.window.showQuickPick([
        { label: "Yes", value: true, description: defaultValue ? "default" : undefined },
        { label: "No", value: false, description: !defaultValue ? "default" : undefined },
    ], {
        title,
        canPickMany: false,
    });
    return picked?.value;
}
//# sourceMappingURL=SchemaWizardCommands.js.map