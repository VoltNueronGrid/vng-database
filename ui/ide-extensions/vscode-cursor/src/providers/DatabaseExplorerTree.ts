import { Connection } from "../models/Connection";

export interface ConnectionNodePresentation {
  description: string;
  contextValue: "connectionActive" | "connectionInactive";
  browseMessage: string;
}

export interface ConnectionFlowSnapshot {
  rootKind: "empty" | "connections";
  canExpand: boolean;
}

export function describeConnectionNode(connection: Connection): ConnectionNodePresentation {
  const badges: string[] = [];

  if (connection.isActive) {
    badges.push("Active");
  }

  badges.push(connection.isConnected ? "Verified" : "Not verified");

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

export function shouldExpandConnectionToDatabases(connection: Connection): boolean {
  return connection.isActive && connection.isConnected;
}

export function getConnectionFlowSnapshot(connections: Connection[], selected?: Connection): ConnectionFlowSnapshot {
  return {
    rootKind: connections.length === 0 ? "empty" : "connections",
    canExpand: selected ? shouldExpandConnectionToDatabases(selected) : false,
  };
}