/**
 * Services module - export all service classes
 */

export { ConnectionManager, createConnectionManager } from "./ConnectionManager";
export { connectionToDriverConfig, makeVngDriver, executeDriverRequest, DriverError } from "./DriverAdapter";
export { HttpClient, createHttpClient } from "./HttpClient";
export { NativeClient, createNativeClient } from "./NativeClient";
export {
	QueryExecutionOptions,
	QueryExecutionService,
	QueryStreamOptions,
	createQueryExecutionService,
} from "./QueryExecutionService";
export { createQueryHistoryEntry, findOldestHistoryEntryId, toQueryHistoryStatus } from "./QueryHistory";
export { SchemaManager, createSchemaManager } from "./SchemaManager";
export { redactSecrets, toSafeErrorMessage } from "./SecretSafeErrors";
export { TableEditorService, createTableEditorService } from "./TableEditorService";
export { buildRemediationHint } from "./RemediationHints";
export {
  checkCommandPermission,
  isDestructiveOperation,
  RbacOperation,
  RbacCheckResult,
} from "./RbacGuard";
