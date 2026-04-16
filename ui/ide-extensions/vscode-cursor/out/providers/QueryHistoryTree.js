"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.summarizeQuery = summarizeQuery;
exports.buildQueryHistoryItems = buildQueryHistoryItems;
exports.describeQueryHistoryEntry = describeQueryHistoryEntry;
function summarizeQuery(query) {
    const normalized = query.replace(/\s+/g, " ").trim();
    if (normalized.length <= 70) {
        return normalized;
    }
    return `${normalized.slice(0, 67)}...`;
}
function buildQueryHistoryItems(entries, limit = 50) {
    if (entries.length === 0) {
        return [{ type: "empty", label: "No query history yet" }];
    }
    return entries.slice(0, limit).map((entry) => ({
        type: "entry",
        label: summarizeQuery(entry.query),
        entry,
    }));
}
function describeQueryHistoryEntry(entry, formatTime = (date) => date.toLocaleTimeString(), formatDateTime = (date) => date.toLocaleString()) {
    const timestamp = new Date(entry.timestamp);
    return {
        description: `${entry.status} • ${entry.executionTime ?? 0} ms • ${formatTime(timestamp)}`,
        tooltip: `${entry.query}\n\nStatus: ${entry.status}\nExecution: ${entry.executionTime ?? 0} ms\nTimestamp: ${formatDateTime(timestamp)}`,
        iconId: entry.status === "success" ? "pass-filled" : "error",
    };
}
//# sourceMappingURL=QueryHistoryTree.js.map