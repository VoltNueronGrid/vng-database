"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.readConnection = readConnection;
exports.runConnectionWizard = runConnectionWizard;
const vscode = __importStar(require("vscode"));
const SECTION = "voltnuerongrid";
const SETTINGS_KEY = "connectionSettings";
const SECRET_ADMIN_KEY = "voltnuerongrid.adminApiKey";
async function readConnection(context) {
    const stored = context.globalState.get(SETTINGS_KEY);
    if (!stored) {
        return undefined;
    }
    const normalized = {
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
async function runConnectionWizard(context) {
    const config = vscode.workspace.getConfiguration(SECTION);
    const defaultUrl = config.get("baseUrl") ?? "http://127.0.0.1:8080";
    const runtimePick = await vscode.window.showQuickPick([
        { label: "Local", description: "127.0.0.1 host runtime", value: "local" },
        { label: "Docker", description: "host.docker.internal or mapped localhost", value: "docker" },
        { label: "Cloud", description: "Hosted HTTPS endpoint", value: "cloud" },
        { label: "Custom", description: "Manually enter any runtime URL", value: "custom" },
    ], {
        title: "Runtime Target",
        placeHolder: "Where is VoltNueronGrid running?",
        ignoreFocusOut: true,
    });
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
    const modePick = await vscode.window.showQuickPick([
        { label: "Admin", value: "admin" },
        { label: "Operator", value: "operator" },
        { label: "Tenant", value: "tenant" },
    ], {
        title: "Connection Mode",
        placeHolder: "Choose an identity mode",
        ignoreFocusOut: true,
    });
    if (!modePick) {
        return undefined;
    }
    let adminApiKey;
    let operatorId;
    let tenantId;
    let userId;
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
    const settings = {
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
    }
    else {
        await context.secrets.delete(SECRET_ADMIN_KEY);
    }
    return {
        settings,
        adminApiKey: adminApiKey?.trim(),
    };
}
function validateUrl(value) {
    const normalized = value.trim();
    if (normalized.length === 0) {
        return "Base URL is required.";
    }
    try {
        const url = new URL(normalized);
        if (url.protocol !== "http:" && url.protocol !== "https:") {
            return "URL must use http or https.";
        }
    }
    catch {
        return "Enter a valid URL such as http://127.0.0.1:8080";
    }
    return undefined;
}
function trimTrailingSlash(value) {
    return value.replace(/\/+$/, "");
}
function runtimeDefaultUrl(target, configuredDefault) {
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
//# sourceMappingURL=config.js.map