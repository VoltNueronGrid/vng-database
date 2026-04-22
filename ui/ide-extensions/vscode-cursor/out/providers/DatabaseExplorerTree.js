"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.describeTableRowCount = describeTableRowCount;
exports.describeTableSections = describeTableSections;
exports.groupConnectionsForTree = groupConnectionsForTree;
exports.describeConnectionNode = describeConnectionNode;
exports.shouldExpandConnectionToDatabases = shouldExpandConnectionToDatabases;
exports.getConnectionFlowSnapshot = getConnectionFlowSnapshot;
function describeTableRowCount(table) {
    if (table.rowCount === undefined || Number.isNaN(table.rowCount) || table.rowCount < 0) {
        return "";
    }
    if (table.rowCount < 1000) {
        return `${table.rowCount} rows`;
    }
    if (table.rowCount < 1000000) {
        return `~${(table.rowCount / 1000).toFixed(1)}K rows`;
    }
    if (table.rowCount < 1000000000) {
        return `~${(table.rowCount / 1000000).toFixed(1)}M rows`;
    }
    return `~${(table.rowCount / 1000000000).toFixed(1)}B rows`;
}
function describeTableSections(table) {
    return [
        {
            kind: "columns",
            label: "Columns",
            count: table.columns.length,
        },
        {
            kind: "indexes",
            label: "Indexes",
            count: table.indexes.length,
        },
        {
            kind: "triggers",
            label: "Triggers",
            count: table.triggers?.length ?? 0,
        },
    ];
}
function groupConnectionsForTree(connections) {
    const grouped = new Map();
    for (const connection of connections) {
        const label = connection.settings.group?.trim() || "localmachine";
        const key = label.toLowerCase();
        const bucket = grouped.get(key);
        if (bucket) {
            bucket.push(connection);
            continue;
        }
        grouped.set(key, [connection]);
    }
    return Array.from(grouped.entries())
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, bucket]) => ({
        groupLabel: bucket[0]?.settings.group?.trim() || (key === "localmachine" ? "localmachine" : key),
        connections: bucket.sort((left, right) => left.settings.name.localeCompare(right.settings.name)),
    }));
}
function describeConnectionNode(connection) {
    const badges = [];
    if (connection.isActive) {
        badges.push("Active");
    }
    if (connection.state === "verified") {
        badges.push("Verified");
    }
    else if (connection.state === "degraded") {
        badges.push("Degraded");
    }
    else if (connection.state === "error") {
        badges.push("Error");
    }
    else {
        badges.push(connection.isConnected ? "Verified" : "Not verified");
    }
    return {
        description: badges.join(" • "),
        contextValue: connection.isActive ? "connectionActive" : "connectionInactive",
        browseMessage: connection.isActive
            ? connection.isConnected
                ? `Browsing ${connection.settings.name}`
                : `Connection is active but not verified. Run Connect/Test to browse databases.`
            : `Activate ${connection.settings.name} to browse databases.`,
    };
}
function shouldExpandConnectionToDatabases(connection) {
    return connection.isActive && connection.state === "verified";
}
function getConnectionFlowSnapshot(connections, selected) {
    return {
        rootKind: connections.length === 0 ? "empty" : "connections",
        canExpand: selected ? shouldExpandConnectionToDatabases(selected) : false,
    };
}
//# sourceMappingURL=DatabaseExplorerTree.js.map