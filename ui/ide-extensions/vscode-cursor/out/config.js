"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.readConnection = readConnection;
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
//# sourceMappingURL=config.js.map