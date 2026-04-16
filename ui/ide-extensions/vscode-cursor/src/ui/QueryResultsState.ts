import { QueryResult } from "../models";

export interface QueryResultsState {
  operation: string;
  connectionName: string;
  result: QueryResult;
}

export function createDefaultQueryResultsState(connectionName = "No active connection", timestamp = Date.now()): QueryResultsState {
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

export function createQueryResultsState(result: QueryResult, operation: string, connectionName: string): QueryResultsState {
  return {
    operation,
    connectionName,
    result,
  };
}