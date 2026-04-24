/**
 * S5-004: RBAC command authorization guard.
 *
 * Provides a lightweight permission check for IDE commands based on the
 * connection mode (admin / operator / tenant).  Destructive operations
 * always require an additional confirmation dialog in the caller.
 */

import { Connection } from "../models";

export type RbacOperation =
  | "query"
  | "schema-read"
  | "schema-write"
  | "drop"
  | "truncate";

export interface RbacCheckResult {
  allowed: boolean;
  reason?: string;
}

const ADMIN_ALLOWED: ReadonlySet<RbacOperation> = new Set([
  "query",
  "schema-read",
  "schema-write",
  "drop",
  "truncate",
]);

const OPERATOR_ALLOWED: ReadonlySet<RbacOperation> = new Set([
  "query",
  "schema-read",
  "schema-write",
]);

const TENANT_ALLOWED: ReadonlySet<RbacOperation> = new Set([
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
export function checkCommandPermission(
  connection: Connection,
  operation: RbacOperation
): RbacCheckResult {
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
      const _exhaustive: never = mode;
      void _exhaustive;
      return {
        allowed: false,
        reason: `Unknown connection mode '${mode}'. Access denied.`,
      };
    }
  }
}

/** Returns true when the operation is considered destructive (requires confirmation). */
export function isDestructiveOperation(operation: RbacOperation): boolean {
  return operation === "drop" || operation === "truncate";
}
