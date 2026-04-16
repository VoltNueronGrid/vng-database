"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.TableEditorService = void 0;
exports.createTableEditorService = createTableEditorService;
const TableEditorSql_1 = require("./TableEditorSql");
const QueryResult_1 = require("../models/QueryResult");
class TableEditorService {
    constructor(httpClient, schemaManager) {
        this.httpClient = httpClient;
        this.schemaManager = schemaManager;
    }
    async openSession(connection, target, page = 1, pageSize = 50, infoMessage) {
        const table = await this.schemaManager.getTable(connection, target.database, target.schema, target.tableName);
        if (!table) {
            throw new Error(`Table '${target.schema}.${target.tableName}' was not found in the schema registry.`);
        }
        return this.loadPage(connection, target, table, page, pageSize, infoMessage);
    }
    updateCell(session, rowId, columnName, value) {
        return this.withRows(session, session.rows.map((row) => (row.rowId === rowId ? { ...row, values: { ...row.values, [columnName]: value } } : row)));
    }
    addDraftRow(session) {
        const values = Object.fromEntries(session.columns.map((column) => [column.name, ""]));
        const row = {
            rowId: `draft-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
            kind: "draft",
            values,
            isDeleted: false,
        };
        return this.withRows(session, [row, ...session.rows], "Draft row added.");
    }
    toggleDeleteRow(session, rowId) {
        const nextRows = [];
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
    async discardChanges(connection, session) {
        return this.openSession(connection, session.target, session.page, session.pageSize, "Changes discarded.");
    }
    async changePage(connection, session, direction) {
        const nextPage = direction === "next" ? session.page + 1 : session.page - 1;
        if (nextPage < 1) {
            return session;
        }
        return this.openSession(connection, session.target, nextPage, session.pageSize);
    }
    async saveSession(connection, session) {
        const statements = this.buildStatements(session);
        if (statements.length === 0) {
            return { ...session, infoMessage: "No changes to save.", errorMessage: undefined };
        }
        for (const statement of statements) {
            const result = await this.executeStatement(connection, statement);
            if (result.status !== "success") {
                const message = result.error?.message ?? "Save failed.";
                return {
                    ...session,
                    errorMessage: `${message} Some changes may have been applied already. Refresh before retrying if needed.`,
                    infoMessage: undefined,
                };
            }
        }
        return this.openSession(connection, session.target, session.page, session.pageSize, `Saved ${statements.length} change(s).`);
    }
    async loadPage(connection, target, table, page, pageSize, infoMessage) {
        const sql = (0, TableEditorSql_1.buildSelectPageSql)(target, table.columns, page, pageSize);
        const result = await this.executeStatement(connection, sql);
        if (result.status !== "success") {
            throw new Error(result.error?.message ?? `Failed to load rows for ${target.schema}.${target.tableName}.`);
        }
        const pageRows = result.rows.slice(0, pageSize);
        return {
            target,
            table,
            columns: table.columns,
            capabilities: (0, TableEditorSql_1.deriveTableEditorCapabilities)(table),
            rows: pageRows.map((row, index) => this.toTableEditorRow(row, table, page, index)),
            page,
            pageSize,
            hasNextPage: result.rows.length > pageSize,
            dirty: false,
            infoMessage,
            errorMessage: undefined,
        };
    }
    buildStatements(session) {
        const statements = [];
        for (const row of session.rows) {
            if (row.kind === "draft") {
                if (!(0, TableEditorSql_1.hasAnyRowValue)(row)) {
                    continue;
                }
                if (!session.capabilities.canInsert) {
                    throw new Error("This table does not allow inserting new rows from the editor.");
                }
                const validationErrors = (0, TableEditorSql_1.validateDraftRow)(session.table, row);
                if (validationErrors.length > 0) {
                    throw new Error(validationErrors.join("\n"));
                }
                statements.push((0, TableEditorSql_1.buildInsertStatement)(session.target, session.table, row));
                continue;
            }
            if (row.isDeleted) {
                if (!session.capabilities.canDelete) {
                    throw new Error("Delete requires a primary key or unique key.");
                }
                statements.push((0, TableEditorSql_1.buildDeleteStatement)(session.target, session.table, row, session.capabilities));
                continue;
            }
            const updateStatement = (0, TableEditorSql_1.buildUpdateStatement)(session.target, session.table, row, session.capabilities);
            if (updateStatement) {
                if (!session.capabilities.canUpdate) {
                    throw new Error("Update requires a primary key or unique key.");
                }
                statements.push(updateStatement);
            }
        }
        return statements;
    }
    async executeStatement(connection, sql) {
        const startedAt = Date.now();
        const response = await this.httpClient.executeQuery(connection, sql, {
            requestId: `table-editor-${startedAt}`,
            timeoutMs: connection.settings.advanced.connectionTimeout ?? 30000,
        });
        const executionTime = Date.now() - startedAt;
        if (response.status === 200) {
            const result = (0, QueryResult_1.parseQueryResult)(sql, response.data, executionTime);
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
    toTableEditorRow(row, table, page, index) {
        const values = Object.fromEntries(table.columns.map((column) => [column.name, (0, TableEditorSql_1.toEditorValue)(row[column.name])]));
        return {
            rowId: `existing-${page}-${index}`,
            kind: "existing",
            values,
            originalValues: { ...values },
            isDeleted: false,
        };
    }
    withRows(session, rows, infoMessage) {
        return {
            ...session,
            rows,
            dirty: (0, TableEditorSql_1.countPendingChanges)(rows, session.capabilities) > 0,
            infoMessage,
            errorMessage: undefined,
        };
    }
}
exports.TableEditorService = TableEditorService;
function createTableEditorService(httpClient, schemaManager) {
    return new TableEditorService(httpClient, schemaManager);
}
//# sourceMappingURL=TableEditorService.js.map