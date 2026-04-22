import { Connection } from "../models/Connection";

function truncate(value: string, maxLength: number): string {
  if (value.length <= maxLength) {
    return value;
  }
  return `${value.slice(0, Math.max(0, maxLength - 3))}...`;
}

export function getConnectionHostLabel(connection: Connection): string {
  try {
    const parsed = new URL(connection.settings.baseUrl);
    return `${parsed.hostname}:${parsed.port || (parsed.protocol === "https:" ? "443" : "80")}`;
  } catch {
    const host = connection.settings.host || "unknown";
    const port = connection.settings.port || 0;
    return `${host}:${port}`;
  }
}

export function toConnectionExportJson(connection: Connection): string {
  return JSON.stringify(
    {
      id: connection.id,
      state: connection.state,
      isActive: connection.isActive,
      isConnected: connection.isConnected,
      diagnostics: connection.diagnostics,
      settings: {
        ...connection.settings,
        adminKey: connection.settings.adminKey ? "<redacted>" : undefined,
      },
    },
    null,
    2
  );
}

export function buildConnectionStatusSummary(connection: Connection, historyCount: number): string[] {
  const diagnosticsDetail = connection.diagnostics.detail ? truncate(connection.diagnostics.detail, 140) : "n/a";
  return [
    `Connection: ${connection.settings.name}`,
    `Mode: ${connection.settings.mode}`,
    `State: ${connection.state}`,
    `Base URL: ${connection.settings.baseUrl}`,
    `Host: ${getConnectionHostLabel(connection)}`,
    `History entries: ${historyCount}`,
    `Last check reason: ${connection.diagnostics.reason ?? "n/a"}`,
    `Last check detail: ${diagnosticsDetail}`,
  ];
}
