"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.describeConnectionNode = describeConnectionNode;
exports.getEmptyConnectionMessage = getEmptyConnectionMessage;
function describeConnectionNode(connection) {
    const badges = [];
    if (connection.isActive) {
        badges.push("Active");
    }
    badges.push(connection.isConnected ? "Connected" : "Not verified");
    return {
        description: badges.join(" • "),
        contextValue: connection.isActive ? "connectionActive" : "connectionInactive",
        browseMessage: connection.isActive
            ? `Browsing ${connection.settings.name}`
            : `Activate ${connection.settings.name} to browse databases.`,
    };
}
function getEmptyConnectionMessage() {
    return "No connections available. Create New Connection.";
}
//# sourceMappingURL=DatabaseExplorerTree.js.map