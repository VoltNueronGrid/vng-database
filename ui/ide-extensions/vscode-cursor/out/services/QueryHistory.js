"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.toQueryHistoryStatus = toQueryHistoryStatus;
exports.createQueryHistoryEntry = createQueryHistoryEntry;
exports.findOldestHistoryEntryId = findOldestHistoryEntryId;
function toQueryHistoryStatus(status) {
    if (status === "success" || status === "cancelled") {
        return status;
    }
    return "error";
}
function createQueryHistoryEntry(connectionId, resultId, query, result, timestamp = Date.now()) {
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
function findOldestHistoryEntryId(entries) {
    let oldest;
    for (const entry of entries) {
        if (!oldest || entry[1].timestamp < oldest[1].timestamp) {
            oldest = entry;
        }
    }
    return oldest?.[0];
}
//# sourceMappingURL=QueryHistory.js.map