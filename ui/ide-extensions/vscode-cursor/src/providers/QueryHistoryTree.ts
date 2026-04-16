import { QueryHistoryEntry } from "../models";

export interface QueryHistoryListItem {
  type: "entry" | "empty";
  label: string;
  entry?: QueryHistoryEntry;
}

export interface QueryHistoryEntryPresentation {
  description: string;
  tooltip: string;
  iconId: "pass-filled" | "error";
}

export function summarizeQuery(query: string): string {
  const normalized = query.replace(/\s+/g, " ").trim();
  if (normalized.length <= 70) {
    return normalized;
  }

  return `${normalized.slice(0, 67)}...`;
}

export function buildQueryHistoryItems(entries: QueryHistoryEntry[], limit = 50): QueryHistoryListItem[] {
  if (entries.length === 0) {
    return [{ type: "empty", label: "No query history yet" }];
  }

  return entries.slice(0, limit).map((entry) => ({
    type: "entry",
    label: summarizeQuery(entry.query),
    entry,
  }));
}

export function describeQueryHistoryEntry(
  entry: QueryHistoryEntry,
  formatTime: (date: Date) => string = (date) => date.toLocaleTimeString(),
  formatDateTime: (date: Date) => string = (date) => date.toLocaleString()
): QueryHistoryEntryPresentation {
  const timestamp = new Date(entry.timestamp);
  return {
    description: `${entry.status} • ${entry.executionTime ?? 0} ms • ${formatTime(timestamp)}`,
    tooltip: `${entry.query}\n\nStatus: ${entry.status}\nExecution: ${entry.executionTime ?? 0} ms\nTimestamp: ${formatDateTime(timestamp)}`,
    iconId: entry.status === "success" ? "pass-filled" : "error",
  };
}