import * as vscode from "vscode";
import { analyzeSql, executeSql } from "../client";
import { RuntimeConnection } from "../config";
import { ConnectionManager, QueryExecutionService, SchemaManager } from "../services";
import { Connection, QueryResult } from "../models";

const SQL_KEYWORDS = [
  "SELECT",
  "FROM",
  "WHERE",
  "INSERT",
  "UPDATE",
  "DELETE",
  "JOIN",
  "LEFT",
  "RIGHT",
  "INNER",
  "OUTER",
  "GROUP BY",
  "ORDER BY",
  "HAVING",
  "LIMIT",
  "OFFSET",
  "CREATE",
  "ALTER",
  "DROP",
  "TABLE",
  "INDEX",
  "VIEW",
  "COUNT",
  "SUM",
  "AVG",
  "MIN",
  "MAX",
  "DISTINCT",
  "AS",
  "AND",
  "OR",
  "NOT",
  "IN",
  "LIKE",
  "BETWEEN",
  "IS NULL",
  "TRUE",
  "FALSE",
];

const TABLE_DIAGNOSTIC_CODE = "VNG_SQL_UNKNOWN_TABLE";
const COLUMN_DIAGNOSTIC_CODE = "VNG_SQL_UNKNOWN_COLUMN";

interface SqlFunctionDefinition {
  name: string;
  signature: string;
  description: string;
  category: string;
  parameters: string[];
  snippet?: string;
}

interface SqlSnippetDefinition {
  label: string;
  detail: string;
  snippet: string;
  contexts: Array<"general" | "table" | "column">;
}

interface SqlDiagnosticData {
  kind: "table" | "column";
  suggestions: string[];
}

interface SchemaTableRef {
  database: string;
  schema: string;
  table: string;
  fullName: string;
  columns: Set<string>;
}

const SQL_FUNCTIONS: SqlFunctionDefinition[] = [
  { name: "COUNT", signature: "COUNT(expression)", description: "Returns the number of matching rows.", category: "aggregate", parameters: ["expression"], snippet: "COUNT(${1:*})" },
  { name: "SUM", signature: "SUM(expression)", description: "Returns the sum of numeric values.", category: "aggregate", parameters: ["expression"], snippet: "SUM(${1:expression})" },
  { name: "AVG", signature: "AVG(expression)", description: "Returns the average of numeric values.", category: "aggregate", parameters: ["expression"], snippet: "AVG(${1:expression})" },
  { name: "MIN", signature: "MIN(expression)", description: "Returns the minimum value.", category: "aggregate", parameters: ["expression"], snippet: "MIN(${1:expression})" },
  { name: "MAX", signature: "MAX(expression)", description: "Returns the maximum value.", category: "aggregate", parameters: ["expression"], snippet: "MAX(${1:expression})" },
  { name: "ROW_NUMBER", signature: "ROW_NUMBER() OVER (...)", description: "Assigns a sequential row number within a window.", category: "window", parameters: [], snippet: "ROW_NUMBER() OVER (${1:PARTITION BY column ORDER BY column})" },
  { name: "RANK", signature: "RANK() OVER (...)", description: "Ranks rows within a window, leaving gaps for ties.", category: "window", parameters: [], snippet: "RANK() OVER (${1:PARTITION BY column ORDER BY column})" },
  { name: "DENSE_RANK", signature: "DENSE_RANK() OVER (...)", description: "Ranks rows within a window without gaps.", category: "window", parameters: [], snippet: "DENSE_RANK() OVER (${1:PARTITION BY column ORDER BY column})" },
  { name: "LAG", signature: "LAG(expression, offset, default)", description: "Reads a prior row value in the current window.", category: "window", parameters: ["expression", "offset", "default"], snippet: "LAG(${1:expression}, ${2:1}, ${3:NULL}) OVER (${4:ORDER BY column})" },
  { name: "LEAD", signature: "LEAD(expression, offset, default)", description: "Reads a following row value in the current window.", category: "window", parameters: ["expression", "offset", "default"], snippet: "LEAD(${1:expression}, ${2:1}, ${3:NULL}) OVER (${4:ORDER BY column})" },
  { name: "COALESCE", signature: "COALESCE(value, ...)", description: "Returns the first non-null value.", category: "conditional", parameters: ["value", "fallback"], snippet: "COALESCE(${1:value}, ${2:fallback})" },
  { name: "NULLIF", signature: "NULLIF(left, right)", description: "Returns null when two expressions are equal.", category: "conditional", parameters: ["left", "right"], snippet: "NULLIF(${1:left}, ${2:right})" },
  { name: "SUBSTR", signature: "SUBSTR(value, start, length)", description: "Returns a substring from a string value.", category: "string", parameters: ["value", "start", "length"], snippet: "SUBSTR(${1:value}, ${2:1}, ${3:length})" },
  { name: "UPPER", signature: "UPPER(value)", description: "Converts a string to upper case.", category: "string", parameters: ["value"], snippet: "UPPER(${1:value})" },
  { name: "LOWER", signature: "LOWER(value)", description: "Converts a string to lower case.", category: "string", parameters: ["value"], snippet: "LOWER(${1:value})" },
  { name: "LENGTH", signature: "LENGTH(value)", description: "Returns the length of a string.", category: "string", parameters: ["value"], snippet: "LENGTH(${1:value})" },
  { name: "ROUND", signature: "ROUND(value, scale)", description: "Rounds a numeric value to the requested scale.", category: "numeric", parameters: ["value", "scale"], snippet: "ROUND(${1:value}, ${2:2})" },
  { name: "ABS", signature: "ABS(value)", description: "Returns the absolute value of a number.", category: "numeric", parameters: ["value"], snippet: "ABS(${1:value})" },
  { name: "NOW", signature: "NOW()", description: "Returns the current timestamp.", category: "date", parameters: [], snippet: "NOW()" },
  { name: "DATE_TRUNC", signature: "DATE_TRUNC(part, value)", description: "Truncates a date or timestamp to the requested precision.", category: "date", parameters: ["part", "value"], snippet: "DATE_TRUNC(${1:'day'}, ${2:value})" },
  { name: "EXTRACT", signature: "EXTRACT(part FROM value)", description: "Extracts a component from a date or timestamp.", category: "date", parameters: ["part", "value"], snippet: "EXTRACT(${1:year} FROM ${2:value})" },
];

const SQL_SNIPPETS: SqlSnippetDefinition[] = [
  {
    label: "SELECT template",
    detail: "Create a SELECT statement skeleton",
    snippet: "SELECT\n  ${1:*}\nFROM ${2:table_name}\nWHERE ${3:condition};",
    contexts: ["general", "column"],
  },
  {
    label: "INSERT template",
    detail: "Create an INSERT statement skeleton",
    snippet: "INSERT INTO ${1:table_name} (${2:column1}, ${3:column2})\nVALUES (${4:value1}, ${5:value2});",
    contexts: ["general", "table"],
  },
  {
    label: "JOIN template",
    detail: "Create an INNER JOIN clause skeleton",
    snippet: "INNER JOIN ${1:table_name} ${2:alias} ON ${3:left_column} = ${4:right_column}",
    contexts: ["general", "table"],
  },
  {
    label: "CTE template",
    detail: "Create a common table expression",
    snippet: "WITH ${1:cte_name} AS (\n  ${2:SELECT * FROM table_name}\n)\nSELECT ${3:*}\nFROM ${1:cte_name};",
    contexts: ["general"],
  },
];

export interface SqlEditorFeatureDependencies {
  context: vscode.ExtensionContext;
  output: vscode.OutputChannel;
  getConnection: () => Promise<RuntimeConnection | undefined>;
  connectionManager: ConnectionManager;
  queryExecutionService: QueryExecutionService;
  schemaManager: SchemaManager;
  onQueryResult?: (result: QueryResult, operation: string, connectionName: string) => Promise<void>;
}

export function registerSqlEditorFeatures(deps: SqlEditorFeatureDependencies): vscode.Disposable[] {
  const diagnosticsCollection = vscode.languages.createDiagnosticCollection("voltnuerongrid.sql");
  const diagnosticDataByKey = new Map<string, SqlDiagnosticData>();

  const executeSelectionOrFile = vscode.commands.registerCommand("vng.sql.executeSelectionOrFile", async () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isSqlDocument(editor.document)) {
      vscode.window.showWarningMessage("Open a .sql file to run this command.");
      return;
    }

    const sql = getSqlToRun(editor);
    if (!sql.trim()) {
      vscode.window.showWarningMessage("No SQL found to execute.");
      return;
    }

    const activeConnection = await resolveManagedConnection(deps);
    if (!activeConnection) {
      vscode.window.showWarningMessage("No VoltNueronGrid connection configured.");
      return;
    }

    await runManagedSqlAndPresent("Execute SQL", sql, activeConnection, deps);
  });

  const analyzeSelectionOrFile = vscode.commands.registerCommand("vng.sql.analyzeSelectionOrFile", async () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !isSqlDocument(editor.document)) {
      vscode.window.showWarningMessage("Open a .sql file to run this command.");
      return;
    }

    const sql = getSqlToRun(editor);
    if (!sql.trim()) {
      vscode.window.showWarningMessage("No SQL found to analyze.");
      return;
    }

    const runtimeConnection = await deps.getConnection();
    if (!runtimeConnection) {
      vscode.window.showWarningMessage("No VoltNueronGrid connection configured.");
      return;
    }

    await runSqlAndPresent("Analyze SQL", sql, runtimeConnection, deps.output, true);
  });

  const completionProvider = vscode.languages.registerCompletionItemProvider(
    [{ language: "sql" }, { pattern: "**/*.sql" }],
    {
      provideCompletionItems: async (document, position) => {
        const items: vscode.CompletionItem[] = [];
        const sqlBeforeCursor = document.getText(new vscode.Range(new vscode.Position(0, 0), position));
        const activeConnection = deps.connectionManager.getActiveConnection();
        let tables: SchemaTableRef[] = [];
        let aliases = new Map<string, SchemaTableRef>();

        if (activeConnection) {
          try {
            tables = await listSchemaTables(deps.schemaManager, activeConnection);
            aliases = extractAliases(sqlBeforeCursor, tables);
          } catch {
            tables = [];
          }
        }

        const context = getCompletionContext(sqlBeforeCursor);
        const activeClause = getActiveClause(sqlBeforeCursor);
        const aliasTarget = getAliasTarget(sqlBeforeCursor);

        for (const fn of SQL_FUNCTIONS) {
          const functionItem = new vscode.CompletionItem(fn.name, vscode.CompletionItemKind.Function);
          functionItem.insertText = new vscode.SnippetString(fn.snippet ?? `${fn.name}()`);
          functionItem.detail = `${fn.category} function`;
          functionItem.documentation = new vscode.MarkdownString(`**${fn.signature}**\n\n${fn.description}`);
          functionItem.sortText = buildSortText("function", activeClause, fn.name);
          items.push(functionItem);
        }

        for (const snippet of SQL_SNIPPETS.filter((candidate) => candidate.contexts.includes(context))) {
          const snippetItem = new vscode.CompletionItem(snippet.label, vscode.CompletionItemKind.Snippet);
          snippetItem.insertText = new vscode.SnippetString(snippet.snippet);
          snippetItem.detail = snippet.detail;
          snippetItem.sortText = buildSortText("snippet", activeClause, snippet.label);
          items.push(snippetItem);
        }

        for (const keyword of SQL_KEYWORDS) {
          const keywordItem = new vscode.CompletionItem(keyword, vscode.CompletionItemKind.Keyword);
          keywordItem.insertText = keyword;
          keywordItem.sortText = buildSortText("keyword", activeClause, keyword);
          items.push(keywordItem);
        }

        if (tables.length > 0) {
          if (context === "table") {
            addTableCompletionItems(items, tables, activeClause);
          } else if (aliasTarget) {
            const resolvedTable = resolveAliasOrTableReference(aliasTarget, aliases, tables);
            if (resolvedTable) {
              addColumnCompletionItems(items, [resolvedTable], activeClause, aliasTarget);
            }
          } else {
            const scopedTables = aliases.size > 0 ? Array.from(new Set(aliases.values())) : tables;
            addColumnCompletionItems(items, scopedTables, activeClause);
            if (context !== "column") {
              addTableCompletionItems(items, tables, activeClause);
            }
          }
        }

        return items;
      },
    },
    ".",
    " ",
    "(",
    ","
  );

  const hoverProvider = vscode.languages.registerHoverProvider([{ language: "sql" }, { pattern: "**/*.sql" }], {
    provideHover(document, position) {
      const wordRange = document.getWordRangeAtPosition(position, /[A-Za-z_][\w$]*/);
      if (!wordRange) {
        return undefined;
      }

      const token = document.getText(wordRange).toUpperCase();
      const fn = SQL_FUNCTIONS.find((candidate) => candidate.name === token);
      if (!fn) {
        return undefined;
      }

      return new vscode.Hover(new vscode.MarkdownString(`**${fn.signature}**\n\n${fn.description}\n\nCategory: ${fn.category}`), wordRange);
    },
  });

  const signatureHelpProvider = vscode.languages.registerSignatureHelpProvider(
    [{ language: "sql" }, { pattern: "**/*.sql" }],
    {
      provideSignatureHelp(document, position) {
        const context = getSignatureContext(document.getText(new vscode.Range(new vscode.Position(0, 0), position)));
        if (!context) {
          return undefined;
        }

        const fn = SQL_FUNCTIONS.find((candidate) => candidate.name === context.functionName);
        if (!fn) {
          return undefined;
        }

        const signature = new vscode.SignatureInformation(fn.signature, new vscode.MarkdownString(fn.description));
        signature.parameters = fn.parameters.map((parameter) => new vscode.ParameterInformation(parameter));

        const help = new vscode.SignatureHelp();
        help.signatures = [signature];
        help.activeSignature = 0;
        help.activeParameter = Math.min(context.activeParameter, Math.max(signature.parameters.length - 1, 0));
        return help;
      },
    },
    "(",
    ","
  );

  const saveHook = vscode.workspace.onDidSaveTextDocument(async (document) => {
    if (!isSqlDocument(document)) {
      return;
    }

    const runOnSave = vscode.workspace
      .getConfiguration("voltnuerongrid")
      .get<string>("sql.runOnSave", "prompt");

    if (runOnSave === "never") {
      return;
    }

    if (runOnSave === "prompt") {
      const choice = await vscode.window.showInformationMessage(
        `Run SQL from ${document.fileName.split(/[\\/]/).pop()}?`,
        "Run",
        "Skip"
      );
      if (choice !== "Run") {
        return;
      }
    }

    const activeConnection = await resolveManagedConnection(deps);
    if (!activeConnection) {
      return;
    }

    const sql = document.getText();
    if (!sql.trim()) {
      return;
    }

    await runManagedSqlAndPresent("Auto-run on save", sql, activeConnection, deps);
  });

  const openHook = vscode.workspace.onDidOpenTextDocument(async (document) => {
    if (!isSqlDocument(document)) {
      return;
    }

    const runOnOpen = vscode.workspace
      .getConfiguration("voltnuerongrid")
      .get<string>("sql.runOnOpen", "never");

    if (runOnOpen === "never") {
      return;
    }

    if (runOnOpen === "prompt") {
      const choice = await vscode.window.showInformationMessage(
        `Run SQL from ${document.fileName.split(/[\\/]/).pop()} on open?`,
        "Run",
        "Skip"
      );
      if (choice !== "Run") {
        return;
      }
    }

    const activeConnection = await resolveManagedConnection(deps);
    if (!activeConnection) {
      return;
    }

    const sql = document.getText();
    if (!sql.trim()) {
      return;
    }

    await runManagedSqlAndPresent("Auto-run on open", sql, activeConnection, deps);
  });

  const diagnosticsProvider = vscode.languages.registerCodeActionsProvider(
    [{ language: "sql" }, { pattern: "**/*.sql" }],
    {
      provideCodeActions(document, range, context) {
        const actions: vscode.CodeAction[] = [];

        for (const diagnostic of context.diagnostics) {
          const data = diagnosticDataByKey.get(createDiagnosticKey(document.uri, diagnostic));
          if (!data || data.suggestions.length === 0) {
            continue;
          }

          const targetRange =
            range.isEqual(diagnostic.range) || range.contains(diagnostic.range.start)
              ? diagnostic.range
              : diagnostic.range;

          for (const suggestion of data.suggestions.slice(0, 3)) {
            const title =
              data.kind === "table"
                ? `Replace with table '${suggestion}'`
                : `Replace with column '${suggestion}'`;

            const action = new vscode.CodeAction(title, vscode.CodeActionKind.QuickFix);
            action.edit = new vscode.WorkspaceEdit();
            action.edit.replace(document.uri, targetRange, suggestion);
            action.diagnostics = [diagnostic];
            action.isPreferred = true;
            actions.push(action);
          }
        }

        return actions;
      },
    },
    {
      providedCodeActionKinds: [vscode.CodeActionKind.QuickFix],
    }
  );

  const updateSqlDiagnostics = async (document: vscode.TextDocument): Promise<void> => {
    if (!isSqlDocument(document)) {
      diagnosticsCollection.delete(document.uri);
      clearDiagnosticDataForDocument(document.uri, diagnosticDataByKey);
      return;
    }

    const activeConnection = deps.connectionManager.getActiveConnection();
    if (!activeConnection) {
      diagnosticsCollection.delete(document.uri);
      clearDiagnosticDataForDocument(document.uri, diagnosticDataByKey);
      return;
    }

    try {
      const diagnostics = await computeDiagnostics(document, deps.schemaManager, activeConnection);
      clearDiagnosticDataForDocument(document.uri, diagnosticDataByKey);
      for (const diagnostic of diagnostics) {
        const suggestions = getSuggestionsFromDiagnosticMessage(diagnostic.message);
        if (suggestions.length > 0) {
          const kind: SqlDiagnosticData["kind"] =
            diagnostic.code === TABLE_DIAGNOSTIC_CODE ? "table" : "column";
          diagnosticDataByKey.set(createDiagnosticKey(document.uri, diagnostic), {
            kind,
            suggestions,
          });
        }
      }
      diagnosticsCollection.set(document.uri, diagnostics);
    } catch {
      diagnosticsCollection.delete(document.uri);
      clearDiagnosticDataForDocument(document.uri, diagnosticDataByKey);
    }
  };

  const diagnosticsChangeHook = vscode.workspace.onDidChangeTextDocument(async (event) => {
    await updateSqlDiagnostics(event.document);
  });

  const diagnosticsSaveHook = vscode.workspace.onDidSaveTextDocument(async (document) => {
    await updateSqlDiagnostics(document);
  });

  const diagnosticsOpenHook = vscode.workspace.onDidOpenTextDocument(async (document) => {
    await updateSqlDiagnostics(document);
  });

  const diagnosticsCloseHook = vscode.workspace.onDidCloseTextDocument((document) => {
    diagnosticsCollection.delete(document.uri);
    clearDiagnosticDataForDocument(document.uri, diagnosticDataByKey);
  });

  void Promise.all(vscode.workspace.textDocuments.filter(isSqlDocument).map((document) => updateSqlDiagnostics(document)));

  return [
    executeSelectionOrFile,
    analyzeSelectionOrFile,
    completionProvider,
    saveHook,
    openHook,
    diagnosticsProvider,
    hoverProvider,
    signatureHelpProvider,
    diagnosticsChangeHook,
    diagnosticsSaveHook,
    diagnosticsOpenHook,
    diagnosticsCloseHook,
    diagnosticsCollection,
  ];
}

function isSqlDocument(document: vscode.TextDocument): boolean {
  return document.languageId === "sql" || document.fileName.toLowerCase().endsWith(".sql");
}

function getSqlToRun(editor: vscode.TextEditor): string {
  const selected = editor.document.getText(editor.selection);
  return selected.trim().length > 0 ? selected : editor.document.getText();
}

async function runSqlAndPresent(
  operation: string,
  sql: string,
  connection: RuntimeConnection,
  output: vscode.OutputChannel,
  analyze: boolean
): Promise<void> {
  const response = analyze ? await analyzeSql(connection, sql) : await executeSql(connection, sql);

  output.appendLine(`[${operation}] HTTP ${response.status}`);
  output.appendLine(response.bodyText || "(empty response)");
  output.appendLine("---");
  output.show(true);

  if (response.status === 200) {
    vscode.window.showInformationMessage(`${operation} succeeded.`);
  } else {
    vscode.window.showErrorMessage(`${operation} failed with HTTP ${response.status}.`);
  }
}

async function runManagedSqlAndPresent(
  operation: string,
  sql: string,
  connection: Connection,
  deps: SqlEditorFeatureDependencies
): Promise<void> {
  const statements = deps.queryExecutionService.parseStatements(sql);
  const executionId = `sql-${Date.now()}`;
  const timeoutMs = connection.settings.advanced.connectionTimeout ?? 30000;
  const results = await deps.queryExecutionService.executeStatementsStream(connection, statements, {
    executionId,
    timeoutMs,
    stopOnError: true,
    onResult: async (result, index, total) => {
      deps.output.appendLine(
        `[${operation}] statement ${index}/${total} status=${result.status} rows=${result.rowCount} time=${result.executionTime}ms`
      );
      if (result.status !== "success" && result.error) {
        deps.output.appendLine(`[${operation}] ${result.error.message}`);
        if (result.error.detail) {
          deps.output.appendLine(result.error.detail);
        }
      }
      deps.output.appendLine("---");
      deps.output.show(true);

      if (deps.onQueryResult) {
        await deps.onQueryResult(result, `${operation} (${index}/${total})`, connection.settings.name);
      }
    },
  });

  const failed = results.find((result) => result.status !== "success");
  if (!failed) {
    const totalRows = results.reduce((sum, result) => sum + result.rowCount, 0);
    const totalTime = results.reduce((sum, result) => sum + result.executionTime, 0);
    vscode.window.showInformationMessage(`${operation} succeeded (${results.length} statement(s), ${totalRows} rows, ${totalTime} ms).`);
    return;
  }

  if (failed.status === "cancelled") {
    vscode.window.showWarningMessage(`${operation} cancelled.`);
    return;
  }

  vscode.window.showErrorMessage(`${operation} failed: ${failed.error?.message ?? "Unknown error"}`);
}

async function resolveManagedConnection(deps: SqlEditorFeatureDependencies): Promise<Connection | undefined> {
  const active = deps.connectionManager.getActiveConnection();
  if (active) {
    return active;
  }

  await deps.getConnection();
  return deps.connectionManager.getActiveConnection() ?? undefined;
}

async function computeDiagnostics(
  document: vscode.TextDocument,
  schemaManager: SchemaManager,
  connection: Connection
): Promise<vscode.Diagnostic[]> {
  const diagnostics: vscode.Diagnostic[] = [];
  const text = document.getText();
  if (!text.trim()) {
    return diagnostics;
  }

  const tables = await listSchemaTables(schemaManager, connection);
  const aliases = extractAliases(text, tables);
  if (tables.length === 0) {
    return diagnostics;
  }

  const tableRegex = /\b(from|join|update|into|table)\s+([a-zA-Z_][\w$]*(?:\.[a-zA-Z_][\w$]*){0,2})/gi;
  let match: RegExpExecArray | null;
  while ((match = tableRegex.exec(text)) !== null) {
    const rawTableName = match[2];
    const normalizedTable = normalizeIdentifier(rawTableName);
    const resolvedTable = resolveTableReference(normalizedTable, tables);
    if (!resolvedTable) {
      const start = match.index + match[0].lastIndexOf(rawTableName);
      const end = start + rawTableName.length;
      const range = new vscode.Range(document.positionAt(start), document.positionAt(end));
      const suggestions = suggestNames(normalizedTable, tables.map((table) => table.fullName));
      const diagnostic = new vscode.Diagnostic(
        range,
        `Unknown table '${rawTableName}'.`,
        vscode.DiagnosticSeverity.Error
      );
      diagnostic.code = TABLE_DIAGNOSTIC_CODE;
      diagnostic.source = "voltnuerongrid-sql";
      if (suggestions.length > 0) {
        diagnostic.message = `${diagnostic.message} Suggestions: ${suggestions.join(", ")}`;
      }
      diagnostics.push(diagnostic);
    }
  }

  const columnRegex = /\b([a-zA-Z_][\w$]*)\.([a-zA-Z_][\w$]*)\b/g;
  while ((match = columnRegex.exec(text)) !== null) {
    const tablePart = normalizeIdentifier(match[1]);
    const columnPart = normalizeIdentifier(match[2]);
    const resolvedTable = resolveAliasOrTableReference(tablePart, aliases, tables);
    if (!resolvedTable) {
      continue;
    }

    if (!resolvedTable.columns.has(columnPart.toLowerCase())) {
      const start = match.index + match[0].lastIndexOf(match[2]);
      const end = start + match[2].length;
      const range = new vscode.Range(document.positionAt(start), document.positionAt(end));
      const suggestions = suggestNames(columnPart, Array.from(resolvedTable.columns));
      const diagnostic = new vscode.Diagnostic(
        range,
        `Unknown column '${match[2]}' on table '${resolvedTable.fullName}'.`,
        vscode.DiagnosticSeverity.Warning
      );
      diagnostic.code = COLUMN_DIAGNOSTIC_CODE;
      diagnostic.source = "voltnuerongrid-sql";
      if (suggestions.length > 0) {
        diagnostic.message = `${diagnostic.message} Suggestions: ${suggestions.join(", ")}`;
      }
      diagnostics.push(diagnostic);
    }
  }

  return diagnostics;
}

function addTableCompletionItems(items: vscode.CompletionItem[], tables: SchemaTableRef[], activeClause: string): void {
  const seen = new Set<string>();
  for (const table of tables) {
    if (seen.has(table.fullName)) {
      continue;
    }
    seen.add(table.fullName);
    const tableItem = new vscode.CompletionItem(table.table, vscode.CompletionItemKind.Struct);
    tableItem.detail = `${table.database}.${table.schema}`;
    tableItem.insertText = table.table;
    tableItem.sortText = buildSortText("table", activeClause, table.table);
    items.push(tableItem);

    const qualifiedItem = new vscode.CompletionItem(table.fullName, vscode.CompletionItemKind.Struct);
    qualifiedItem.detail = "qualified table name";
    qualifiedItem.insertText = table.fullName;
    qualifiedItem.sortText = buildSortText("table", activeClause, table.fullName);
    items.push(qualifiedItem);
  }
}

function addColumnCompletionItems(
  items: vscode.CompletionItem[],
  tables: SchemaTableRef[],
  activeClause: string,
  aliasTarget?: string
): void {
  const seen = new Set<string>();
  for (const table of tables) {
    for (const column of table.columns) {
      const key = `${table.fullName}.${column}`;
      if (seen.has(key)) {
        continue;
      }
      seen.add(key);
      const columnItem = new vscode.CompletionItem(column, vscode.CompletionItemKind.Field);
      columnItem.detail = `${aliasTarget ?? table.schema}.${table.table}.${column}`;
      columnItem.insertText = column;
      columnItem.sortText = buildSortText("column", activeClause, `${table.table}.${column}`);
      items.push(columnItem);
    }
  }
}

function getCompletionContext(sqlBeforeCursor: string): "table" | "column" | "general" {
  if (/([a-zA-Z_][\w$]*)\.\s*$/.test(sqlBeforeCursor)) {
    return "column";
  }

  const activeClause = getActiveClause(sqlBeforeCursor);

  if (["FROM", "JOIN", "UPDATE", "INTO", "TABLE"].includes(activeClause)) {
    return "table";
  }

  if (["SELECT", "WHERE", "GROUP BY", "ORDER BY", "HAVING"].includes(activeClause)) {
    return "column";
  }

  return "general";
}

function getActiveClause(sqlBeforeCursor: string): string {
  const upper = sqlBeforeCursor.toUpperCase();
  const clauses = ["GROUP BY", "ORDER BY", "HAVING", "SELECT", "FROM", "JOIN", "WHERE", "UPDATE", "INTO", "TABLE", "ON"];
  let activeClause = "";
  let bestIndex = -1;

  for (const clause of clauses) {
    const index = upper.lastIndexOf(clause);
    if (index > bestIndex) {
      bestIndex = index;
      activeClause = clause;
    }
  }

  return activeClause;
}

function buildSortText(role: "snippet" | "function" | "keyword" | "table" | "column", activeClause: string, label: string): string {
  const clausePriority: Record<string, Record<string, string>> = {
    SELECT: { column: "01", function: "02", snippet: "03", keyword: "04", table: "09" },
    WHERE: { column: "01", function: "02", keyword: "03", snippet: "04", table: "09" },
    HAVING: { column: "01", function: "02", keyword: "03", snippet: "04", table: "09" },
    "GROUP BY": { column: "01", function: "02", keyword: "03", snippet: "04", table: "09" },
    "ORDER BY": { column: "01", keyword: "02", function: "03", snippet: "04", table: "09" },
    FROM: { table: "01", snippet: "02", keyword: "03", function: "08", column: "09" },
    JOIN: { table: "01", snippet: "02", keyword: "03", function: "08", column: "09" },
    INTO: { table: "01", snippet: "02", keyword: "03", function: "08", column: "09" },
    UPDATE: { table: "01", keyword: "02", snippet: "03", function: "08", column: "09" },
    TABLE: { table: "01", keyword: "02", snippet: "03", function: "08", column: "09" },
    ON: { column: "01", keyword: "02", function: "03", snippet: "04", table: "09" },
  };

  const normalizedClause = clausePriority[activeClause] ? activeClause : "";
  const defaultPriority: Record<string, string> = { snippet: "01", function: "02", keyword: "03", table: "04", column: "05" };
  const priority = normalizedClause ? clausePriority[normalizedClause][role] ?? defaultPriority[role] : defaultPriority[role];
  return `${priority}_${label.toLowerCase()}`;
}

function getSignatureContext(sqlBeforeCursor: string): { functionName: string; activeParameter: number } | undefined {
  let depth = 0;
  let activeParameter = 0;
  for (let index = sqlBeforeCursor.length - 1; index >= 0; index -= 1) {
    const char = sqlBeforeCursor[index];
    if (char === ")") {
      depth += 1;
      continue;
    }
    if (char === "(") {
      if (depth === 0) {
        const functionMatch = sqlBeforeCursor.slice(0, index).match(/([A-Za-z_][\w$]*)\s*$/);
        if (!functionMatch) {
          return undefined;
        }
        return {
          functionName: functionMatch[1].toUpperCase(),
          activeParameter,
        };
      }
      depth -= 1;
      continue;
    }
    if (char === "," && depth === 0) {
      activeParameter += 1;
    }
  }

  return undefined;
}

function getAliasTarget(sqlBeforeCursor: string): string | undefined {
  const match = sqlBeforeCursor.match(/([a-zA-Z_][\w$]*)\.\s*$/);
  return match ? normalizeIdentifier(match[1]) : undefined;
}

function extractAliases(text: string, tables: SchemaTableRef[]): Map<string, SchemaTableRef> {
  const aliases = new Map<string, SchemaTableRef>();
  const aliasRegex = /\b(from|join)\s+([a-zA-Z_][\w$]*(?:\.[a-zA-Z_][\w$]*){0,2})(?:\s+(?:as\s+)?([a-zA-Z_][\w$]*))?/gi;
  let match: RegExpExecArray | null;

  while ((match = aliasRegex.exec(text)) !== null) {
    const tableRef = normalizeIdentifier(match[2]);
    const alias = match[3] ? normalizeIdentifier(match[3]) : undefined;
    const resolvedTable = resolveTableReference(tableRef, tables);
    if (alias && resolvedTable) {
      aliases.set(alias.toLowerCase(), resolvedTable);
    }
  }

  return aliases;
}

function resolveAliasOrTableReference(
  value: string,
  aliases: Map<string, SchemaTableRef>,
  tables: SchemaTableRef[]
): SchemaTableRef | undefined {
  const normalized = normalizeIdentifier(value).toLowerCase();
  return aliases.get(normalized) ?? resolveTableReference(value, tables);
}

async function listSchemaTables(schemaManager: SchemaManager, connection: Connection): Promise<SchemaTableRef[]> {
  const registry = await schemaManager.getSchemaRegistry(connection);
  const result: SchemaTableRef[] = [];

  for (const database of registry.databases) {
    for (const schema of database.schemas || []) {
      for (const table of schema.tables || []) {
        result.push({
          database: database.name,
          schema: schema.name,
          table: table.name,
          fullName: `${database.name}.${schema.name}.${table.name}`,
          columns: new Set((table.columns || []).map((column) => column.name.toLowerCase())),
        });
      }
    }
  }

  return result;
}

function resolveTableReference(value: string, tables: SchemaTableRef[]): SchemaTableRef | undefined {
  const normalized = normalizeIdentifier(value);
  const parts = normalized.split(".");

  if (parts.length >= 3) {
    const [database, schema, table] = parts.slice(parts.length - 3);
    return tables.find(
      (candidate) =>
        candidate.database.toLowerCase() === database.toLowerCase() &&
        candidate.schema.toLowerCase() === schema.toLowerCase() &&
        candidate.table.toLowerCase() === table.toLowerCase()
    );
  }

  if (parts.length === 2) {
    const [schema, table] = parts;
    return tables.find(
      (candidate) =>
        candidate.schema.toLowerCase() === schema.toLowerCase() && candidate.table.toLowerCase() === table.toLowerCase()
    );
  }

  return tables.find((candidate) => candidate.table.toLowerCase() === normalized.toLowerCase());
}

function normalizeIdentifier(value: string): string {
  return value.replace(/["`\[\]]/g, "").trim();
}

function suggestNames(input: string, candidates: string[]): string[] {
  const normalizedInput = input.toLowerCase();
  const prefixMatches = candidates.filter((candidate) => candidate.toLowerCase().startsWith(normalizedInput));
  if (prefixMatches.length > 0) {
    return prefixMatches.slice(0, 3);
  }

  const fuzzyMatches = candidates
    .map((candidate) => ({
      candidate,
      score: levenshtein(normalizedInput, candidate.toLowerCase()),
    }))
    .sort((left, right) => left.score - right.score)
    .map((entry) => entry.candidate);

  return fuzzyMatches.slice(0, 3);
}

function levenshtein(source: string, target: string): number {
  if (source === target) {
    return 0;
  }
  if (source.length === 0) {
    return target.length;
  }
  if (target.length === 0) {
    return source.length;
  }

  const matrix: number[][] = Array.from({ length: source.length + 1 }, () =>
    Array.from<number>({ length: target.length + 1 }).fill(0)
  );

  for (let i = 0; i <= source.length; i++) {
    matrix[i][0] = i;
  }

  for (let j = 0; j <= target.length; j++) {
    matrix[0][j] = j;
  }

  for (let i = 1; i <= source.length; i++) {
    for (let j = 1; j <= target.length; j++) {
      const cost = source[i - 1] === target[j - 1] ? 0 : 1;
      matrix[i][j] = Math.min(
        matrix[i - 1][j] + 1,
        matrix[i][j - 1] + 1,
        matrix[i - 1][j - 1] + cost
      );
    }
  }

  return matrix[source.length][target.length];
}

function createDiagnosticKey(uri: vscode.Uri, diagnostic: vscode.Diagnostic): string {
  return `${uri.toString()}|${String(diagnostic.code)}|${diagnostic.range.start.line}:${diagnostic.range.start.character}|${diagnostic.range.end.line}:${diagnostic.range.end.character}|${diagnostic.message}`;
}

function clearDiagnosticDataForDocument(uri: vscode.Uri, map: Map<string, SqlDiagnosticData>): void {
  const prefix = `${uri.toString()}|`;
  for (const key of map.keys()) {
    if (key.startsWith(prefix)) {
      map.delete(key);
    }
  }
}

function getSuggestionsFromDiagnosticMessage(message: string): string[] {
  const marker = " Suggestions: ";
  const markerIndex = message.indexOf(marker);
  if (markerIndex === -1) {
    return [];
  }

  return message
    .slice(markerIndex + marker.length)
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}
