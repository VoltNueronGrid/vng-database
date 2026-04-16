import { Connection } from "../models/Connection";

export interface ConnectionNodePresentation {
  description: string;
  contextValue: "connectionActive" | "connectionInactive";
  browseMessage: string;
}

export function describeConnectionNode(connection: Connection): ConnectionNodePresentation {
  const badges: string[] = [];

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

export function getEmptyConnectionMessage(): string {
  return "No connections available. Create New Connection.";
}