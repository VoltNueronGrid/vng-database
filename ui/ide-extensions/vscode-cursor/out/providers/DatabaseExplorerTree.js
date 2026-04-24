"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.describeConnectionNode = describeConnectionNode;
exports.shouldExpandConnectionToDatabases = shouldExpandConnectionToDatabases;
exports.getConnectionFlowSnapshot = getConnectionFlowSnapshot;
function describeConnectionNode(connection) {
    const badges = [];
    if (connection.isActive) {
        badges.push("Active");
    }
    badges.push(connection.isConnected ? "Verified" : "Not verified");
    return {
        description: badges.join(" • "),
        contextValue: connection.isActive ? "connectionActive" : "connectionInactive",
        browseMessage: connection.isActive
            ? `Browsing ${connection.settings.name}`
            : `Activate ${connection.settings.name} to browse databases.`,
    };
}
function shouldExpandConnectionToDatabases(connection) {
    return connection.isActive;
}
function getConnectionFlowSnapshot(connections, selected) {
    return {
        rootKind: connections.length === 0 ? "empty" : "connections",
        canExpand: selected ? shouldExpandConnectionToDatabases(selected) : false,
    };
}
//# sourceMappingURL=DatabaseExplorerTree.js.map