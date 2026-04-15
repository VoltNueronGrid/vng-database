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
exports.activate = activate;
exports.deactivate = deactivate;
const vscode = __importStar(require("vscode"));
const config_1 = require("./config");
const client_1 = require("./client");
function activate(context) {
    const output = vscode.window.createOutputChannel("VoltNueronGrid");
    const connect = vscode.commands.registerCommand("vng.connectWizard", async () => {
        const connection = await (0, config_1.runConnectionWizard)(context);
        if (!connection) {
            vscode.window.showInformationMessage("VoltNueronGrid connection wizard canceled.");
            return;
        }
        vscode.window.showInformationMessage(`Saved VoltNueronGrid connection for ${connection.settings.mode} mode.`);
    });
    const test = vscode.commands.registerCommand("vng.testConnection", async () => {
        const connection = await ensureConnection(context);
        if (!connection) {
            vscode.window.showWarningMessage("No VoltNueronGrid connection configured.");
            return;
        }
        await vscode.window.withProgress({
            location: vscode.ProgressLocation.Notification,
            title: "VoltNueronGrid: Testing connectivity",
        }, async () => {
            try {
                const checks = await (0, client_1.runConnectivityChecks)(connection);
                const failed = checks.filter((check) => !check.ok);
                const summary = checks
                    .map((check) => `${check.method} ${check.endpoint} -> ${check.status} (${check.ok ? "ok" : "failed"})`)
                    .join("\n");
                if (failed.length > 0) {
                    output.appendLine("[Connectivity] Failed checks:");
                    output.appendLine(summary);
                    output.show(true);
                    vscode.window.showErrorMessage("Connectivity test failed. See VoltNueronGrid output channel for details.");
                    return;
                }
                output.appendLine("[Connectivity] Passed checks:");
                output.appendLine(summary);
                vscode.window.showInformationMessage("Connectivity test passed.");
            }
            catch (error) {
                const message = error instanceof Error ? error.message : "Unknown connectivity error";
                vscode.window.showErrorMessage(`Connectivity test failed: ${message}`);
            }
        });
    });
    const queryRunner = vscode.commands.registerCommand("vng.runQuery", async () => {
        const connection = await ensureConnection(context);
        if (!connection) {
            return;
        }
        const sql = await vscode.window.showInputBox({
            title: "VoltNueronGrid Query Runner",
            prompt: "Enter SQL to execute",
            placeHolder: "SELECT 1;",
            ignoreFocusOut: true,
            validateInput: (value) => (value.trim().length === 0 ? "SQL is required." : undefined),
        });
        if (!sql) {
            return;
        }
        const response = await (0, client_1.executeSql)(connection, sql);
        await presentResponse("Query Runner", response.status, response.bodyText, connection, output);
    });
    const diagnostics = vscode.commands.registerCommand("vng.analyzeQuery", async () => {
        const connection = await ensureConnection(context);
        if (!connection) {
            return;
        }
        const sql = await vscode.window.showInputBox({
            title: "VoltNueronGrid Query Diagnostics",
            prompt: "Enter SQL to analyze",
            placeHolder: "SELECT * FROM tenant/acme/users;",
            ignoreFocusOut: true,
            validateInput: (value) => (value.trim().length === 0 ? "SQL is required." : undefined),
        });
        if (!sql) {
            return;
        }
        const response = await (0, client_1.analyzeSql)(connection, sql);
        await presentResponse("Query Diagnostics", response.status, response.bodyText, connection, output);
    });
    const schema = vscode.commands.registerCommand("vng.showSchemaRegistry", async () => {
        const connection = await ensureConnection(context);
        if (!connection) {
            return;
        }
        const response = await (0, client_1.getSchemaRegistry)(connection);
        await presentResponse("Schema Registry", response.status, response.bodyText, connection, output);
    });
    context.subscriptions.push(connect, test, queryRunner, diagnostics, schema, output);
}
function deactivate() {
    // No long-running resources to dispose.
}
async function ensureConnection(context) {
    const current = await (0, config_1.readConnection)(context);
    if (current) {
        return current;
    }
    return (0, config_1.runConnectionWizard)(context);
}
async function presentResponse(operation, status, bodyText, connection, output) {
    output.appendLine(`[${operation}] HTTP ${status}`);
    output.appendLine(bodyText || "(empty response)");
    output.appendLine("---");
    output.show(true);
    if (status === 200) {
        vscode.window.showInformationMessage(`${operation} completed successfully.`);
        return;
    }
    const message = (0, client_1.toPermissionMessage)(status, connection.settings.mode);
    if (message) {
        vscode.window.showWarningMessage(`${operation}: ${message}`);
        return;
    }
    vscode.window.showErrorMessage(`${operation} failed with HTTP ${status}.`);
}
//# sourceMappingURL=extension.js.map