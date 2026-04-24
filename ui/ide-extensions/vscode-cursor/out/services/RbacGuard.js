"use strict";
/**
 * S5-004: RBAC command authorization guard.
 *
 * Provides a lightweight permission check for IDE commands based on the
 * connection mode (admin / operator / tenant).  Destructive operations
 * always require an additional confirmation dialog in the caller.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.checkCommandPermission = checkCommandPermission;
exports.isDestructiveOperation = isDestructiveOperation;
const ADMIN_ALLOWED = new Set([
    "query",
    "schema-read",
    "schema-write",
    "drop",
    "truncate",
]);
const OPERATOR_ALLOWED = new Set([
    "query",
    "schema-read",
    "schema-write",
]);
const TENANT_ALLOWED = new Set([
    "query",
    "schema-read",
]);
/**
 * Check whether the given connection's role permits the requested operation.
 *
 * @param connection  The managed connection whose mode drives the check.
 * @param operation   The operation to authorise.
 * @returns           `{ allowed: true }` when permitted, or
 *                    `{ allowed: false, reason: string }` otherwise.
 */
function checkCommandPermission(connection, operation) {
    const mode = connection.settings.mode;
    switch (mode) {
        case "admin":
            return ADMIN_ALLOWED.has(operation)
                ? { allowed: true }
                : { allowed: false, reason: `Operation '${operation}' is not recognised.` };
        case "operator":
            if (OPERATOR_ALLOWED.has(operation)) {
                return { allowed: true };
            }
            return {
                allowed: false,
                reason: `Operator mode does not permit '${operation}'. Use an admin connection for destructive operations.`,
            };
        case "tenant":
            if (TENANT_ALLOWED.has(operation)) {
                return { allowed: true };
            }
            return {
                allowed: false,
                reason: `Tenant mode does not permit '${operation}'. Escalate to operator or admin.`,
            };
        default: {
            // Exhaustive fallback — unknown modes are denied by default.
            const _exhaustive = mode;
            void _exhaustive;
            return {
                allowed: false,
                reason: `Unknown connection mode '${mode}'. Access denied.`,
            };
        }
    }
}
/** Returns true when the operation is considered destructive (requires confirmation). */
function isDestructiveOperation(operation) {
    return operation === "drop" || operation === "truncate";
}
//# sourceMappingURL=RbacGuard.js.map