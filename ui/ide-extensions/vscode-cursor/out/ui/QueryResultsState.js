"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createDefaultQueryResultsState = createDefaultQueryResultsState;
exports.createQueryResultsState = createQueryResultsState;
function createDefaultQueryResultsState(connectionName = "No active connection", timestamp = Date.now()) {
    return {
        operation: "Query Results",
        connectionName,
        result: {
            id: "empty",
            query: "",
            status: "success",
            rows: [],
            columns: [],
            rowCount: 0,
            executionTime: 0,
            timestamp,
        },
    };
}
function createQueryResultsState(result, operation, connectionName) {
    return {
        operation,
        connectionName,
        result,
    };
}
//# sourceMappingURL=QueryResultsState.js.map