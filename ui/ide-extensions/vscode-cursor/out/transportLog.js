"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getTransportOutputChannel = getTransportOutputChannel;
exports.appendTransportLogLine = appendTransportLogLine;
let channel;
function loadVscodeModule() {
    try {
        // Keep runtime compatibility for node --test where the vscode module is not present.
        // eslint-disable-next-line @typescript-eslint/no-require-imports
        return require("vscode");
    }
    catch {
        return undefined;
    }
}
function createFallbackChannel() {
    return {
        appendLine: () => {
            // No-op in non-extension runtimes (unit tests).
        },
    };
}
function getTransportOutputChannel() {
    if (!channel) {
        const vscode = loadVscodeModule();
        channel = vscode?.window.createOutputChannel("VoltNueronGrid Transport") ?? createFallbackChannel();
    }
    return channel;
}
/** NT-S5-002 scaffold: structured lines for transport preference vs actual data-plane (HTTP until TS native execution lands). */
function appendTransportLogLine(message) {
    const ts = new Date().toISOString();
    getTransportOutputChannel().appendLine(`[${ts}] ${message}`);
}
//# sourceMappingURL=transportLog.js.map