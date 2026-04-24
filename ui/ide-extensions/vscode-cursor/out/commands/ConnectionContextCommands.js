"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getConnectionHostLabel = getConnectionHostLabel;
exports.toConnectionExportJson = toConnectionExportJson;
exports.buildConnectionStatusSummary = buildConnectionStatusSummary;
function truncate(value, maxLength) {
    if (value.length <= maxLength) {
        return value;
    }
    return `${value.slice(0, Math.max(0, maxLength - 3))}...`;
}
function getConnectionHostLabel(connection) {
    try {
        const parsed = new URL(connection.settings.baseUrl);
        return `${parsed.hostname}:${parsed.port || (parsed.protocol === "https:" ? "443" : "80")}`;
    }
    catch {
        const host = connection.settings.host || "unknown";
        const port = connection.settings.port || 0;
        return `${host}:${port}`;
    }
}
function toConnectionExportJson(connection) {
    return JSON.stringify({
        id: connection.id,
        state: connection.diagnostic.state,
        isActive: connection.isActive,
        isConnected: connection.isConnected,
        diagnostic: connection.diagnostic,
        settings: {
            ...connection.settings,
            adminKey: connection.settings.adminKey ? "<redacted>" : undefined,
        },
    }, null, 2);
}
function buildConnectionStatusSummary(connection, historyCount) {
    const diagnosticMsg = connection.diagnostic.message ? truncate(connection.diagnostic.message, 140) : "n/a";
    return [
        `Connection: ${connection.settings.name}`,
        `Mode: ${connection.settings.mode}`,
        `State: ${connection.diagnostic.state}`,
        `Base URL: ${connection.settings.baseUrl}`,
        `Host: ${getConnectionHostLabel(connection)}`,
        `History entries: ${historyCount}`,
        `Last diagnostic: ${diagnosticMsg}`,
    ];
}
//# sourceMappingURL=ConnectionContextCommands.js.map