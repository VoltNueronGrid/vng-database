import * as vscode from "vscode";
import { RuntimeConnectionSettings } from "./types";

const SECTION = "voltnuerongrid";
const SETTINGS_KEY = "connectionSettings";
const SECRET_ADMIN_KEY = "voltnuerongrid.adminApiKey";

export interface RuntimeConnection {
  settings: RuntimeConnectionSettings;
  adminApiKey?: string;
}

export async function readConnection(context: vscode.ExtensionContext): Promise<RuntimeConnection | undefined> {
  const stored = context.globalState.get<Partial<RuntimeConnectionSettings>>(SETTINGS_KEY);
  if (!stored) {
    return undefined;
  }

  const normalized: RuntimeConnectionSettings = {
    baseUrl: stored.baseUrl ?? "http://127.0.0.1:8080",
    runtimeTarget: stored.runtimeTarget ?? "custom",
    mode: stored.mode ?? "tenant",
    operatorId: stored.operatorId,
    tenantId: stored.tenantId,
    userId: stored.userId,
  };

  const adminApiKey = await context.secrets.get(SECRET_ADMIN_KEY);
  return {
    settings: normalized,
    adminApiKey: adminApiKey ?? undefined,
  };
}
