import { QueryHistoryEntry, QueryResult } from "../models";

export function toQueryHistoryStatus(status: QueryResult["status"]): QueryHistoryEntry["status"] {
  if (status === "success" || status === "cancelled") {
    return status;
  }

  return "error";
}

export function createQueryHistoryEntry(
  connectionId: string,
  resultId: string,
  query: string,
  result: QueryResult,
  timestamp = Date.now()
): QueryHistoryEntry {
  return {
    id: `hist-${resultId}`,
    query,
    connectionId,
    timestamp,
    executionTime: result.executionTime,
    status: toQueryHistoryStatus(result.status),
    resultId,
  };
}

export function findOldestHistoryEntryId(entries: Iterable<[string, QueryHistoryEntry]>): string | undefined {
  let oldest: [string, QueryHistoryEntry] | undefined;

  for (const entry of entries) {
    if (!oldest || entry[1].timestamp < oldest[1].timestamp) {
      oldest = entry;
    }
  }

  return oldest?.[0];
}