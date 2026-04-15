import * as vscode from "vscode";
import { ConnectionMode, RuntimeConnectionSettings, RuntimeTarget } from "./types";

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

export async function runConnectionWizard(context: vscode.ExtensionContext): Promise<RuntimeConnection | undefined> {
  const config = vscode.workspace.getConfiguration(SECTION);
  const defaultUrl = config.get<string>("baseUrl") ?? "http://127.0.0.1:8080";

  const runtimePick = await vscode.window.showQuickPick(
    [
      { label: "Local", description: "127.0.0.1 host runtime", value: "local" as RuntimeTarget },
      { label: "Docker", description: "host.docker.internal or mapped localhost", value: "docker" as RuntimeTarget },
      { label: "Cloud", description: "Hosted HTTPS endpoint", value: "cloud" as RuntimeTarget },
      { label: "Custom", description: "Manually enter any runtime URL", value: "custom" as RuntimeTarget },
    ],
    {
      title: "Runtime Target",
      placeHolder: "Where is VoltNueronGrid running?",
      ignoreFocusOut: true,
    }
  );

  if (!runtimePick) {
    return undefined;
  }

  const baseUrlSeed = runtimeDefaultUrl(runtimePick.value, defaultUrl);

  const baseUrl = await vscode.window.showInputBox({
    title: "VoltNueronGrid Connection Wizard",
    prompt: "Service base URL",
    value: baseUrlSeed,
    ignoreFocusOut: true,
    validateInput: (value) => validateUrl(value),
  });

  if (!baseUrl) {
    return undefined;
  }

  const modePick = await vscode.window.showQuickPick(
    [
      { label: "Admin", value: "admin" as ConnectionMode },
      { label: "Operator", value: "operator" as ConnectionMode },
      { label: "Tenant", value: "tenant" as ConnectionMode },
    ],
    {
      title: "Connection Mode",
      placeHolder: "Choose an identity mode",
      ignoreFocusOut: true,
    }
  );

  if (!modePick) {
    return undefined;
  }

  let adminApiKey: string | undefined;
  let operatorId: string | undefined;
  let tenantId: string | undefined;
  let userId: string | undefined;

  if (modePick.value === "admin" || modePick.value === "operator") {
    adminApiKey = await vscode.window.showInputBox({
      title: "Admin API Key",
      prompt: "Required for admin/operator flows",
      password: true,
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim().length === 0 ? "Admin API key is required." : undefined),
    });
    if (!adminApiKey) {
      return undefined;
    }
  }

  if (modePick.value === "operator") {
    operatorId = await vscode.window.showInputBox({
      title: "Operator ID",
      prompt: "x-vng-operator-id header value",
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim().length === 0 ? "Operator ID is required." : undefined),
    });
    if (!operatorId) {
      return undefined;
    }
  }

  if (modePick.value === "tenant") {
    tenantId = await vscode.window.showInputBox({
      title: "Tenant ID",
      prompt: "x-vng-tenant-id header value",
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim().length === 0 ? "Tenant ID is required." : undefined),
    });
    if (!tenantId) {
      return undefined;
    }

    userId = await vscode.window.showInputBox({
      title: "User ID",
      prompt: "x-vng-user-id header value",
      ignoreFocusOut: true,
      validateInput: (value) => (value.trim().length === 0 ? "User ID is required." : undefined),
    });
    if (!userId) {
      return undefined;
    }
  }

  const settings: RuntimeConnectionSettings = {
    baseUrl: trimTrailingSlash(baseUrl),
    runtimeTarget: runtimePick.value,
    mode: modePick.value,
    operatorId: operatorId?.trim(),
    tenantId: tenantId?.trim(),
    userId: userId?.trim(),
  };

  await context.globalState.update(SETTINGS_KEY, settings);
  if (adminApiKey?.trim()) {
    await context.secrets.store(SECRET_ADMIN_KEY, adminApiKey.trim());
  } else {
    await context.secrets.delete(SECRET_ADMIN_KEY);
  }

  return {
    settings,
    adminApiKey: adminApiKey?.trim(),
  };
}

function validateUrl(value: string): string | undefined {
  const normalized = value.trim();
  if (normalized.length === 0) {
    return "Base URL is required.";
  }

  try {
    const url = new URL(normalized);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return "URL must use http or https.";
    }
  } catch {
    return "Enter a valid URL such as http://127.0.0.1:8080";
  }

  return undefined;
}

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}

function runtimeDefaultUrl(target: RuntimeTarget, configuredDefault: string): string {
  if (target === "local") {
    return configuredDefault || "http://127.0.0.1:8080";
  }
  if (target === "docker") {
    return "http://host.docker.internal:8080";
  }
  if (target === "cloud") {
    return "https://your-cloud-vng-endpoint";
  }
  return configuredDefault || "http://127.0.0.1:8080";
}
