export interface NativeHealthSessionOptions {
    host: string;
    port: number;
    sessionId: string;
    requestIdPrefix?: string;
    adminApiKey?: string;
    connectTimeoutMs?: number;
    idleMs?: number;
    maxFrameBytes?: number;
}
/** Extends base session options — no extra fields needed; kept for forward compatibility. */
export type NativeCommandSessionOptions = NativeHealthSessionOptions;
/**
 * Core session helper: TCP connect → Hello → Auth (optional) → send Command frame →
 * read Result frame → end socket.
 *
 * Returns the decoded Result payload JSON.
 */
export declare function nativeCommandRoundtrip(opts: NativeCommandSessionOptions, command: string, body: Record<string, unknown>): Promise<unknown>;
/**
 * Minimal native wire session: TCP → Hello → Auth (optional) → Command `health` (Rust driver parity for probes).
 * Returns the decoded Result payload JSON.
 */
export declare function nativeHealthCommandRoundtrip(opts: NativeHealthSessionOptions): Promise<unknown>;
/** Body type for sql.execute command. */
export interface SqlExecuteBody {
    sql_batch: string;
    max_rows?: number;
}
/**
 * Native session roundtrip for `sql.execute`.
 * Sends Command frame with `command: "sql.execute"` and `body: { sql_batch, max_rows? }`.
 */
export declare function nativeSqlExecuteCommandRoundtrip(opts: NativeCommandSessionOptions, body: SqlExecuteBody): Promise<unknown>;
/** Body type for sql.analyze command. */
export interface SqlAnalyzeBody {
    sql_batch: string;
}
/**
 * Native session roundtrip for `sql.analyze`.
 * Sends Command frame with `command: "sql.analyze"` and `body: { sql_batch }`.
 */
export declare function nativeSqlAnalyzeCommandRoundtrip(opts: NativeCommandSessionOptions, body: SqlAnalyzeBody): Promise<unknown>;
/** Body type for sql.route command. */
export interface SqlRouteBody {
    sql_batch: string;
}
/**
 * Native session roundtrip for `sql.route`.
 * Sends Command frame with `command: "sql.route"` and `body: { sql_batch }`.
 */
export declare function nativeSqlRouteCommandRoundtrip(opts: NativeCommandSessionOptions, body: SqlRouteBody): Promise<unknown>;
/** Body type for sql.transaction command. */
export interface SqlTransactionBody {
    statements: string[];
    isolation_level?: string;
}
/**
 * Native session roundtrip for `sql.transaction`.
 * Sends Command frame with `command: "sql.transaction"` and `body: { statements, isolation_level? }`.
 */
export declare function nativeSqlTransactionCommandRoundtrip(opts: NativeCommandSessionOptions, body: SqlTransactionBody): Promise<unknown>;
/**
 * Native session roundtrip for `ingest.schema.registry`.
 * Sends Command frame with `command: "ingest.schema.registry"` and empty body `{}`.
 */
export declare function nativeSchemaRegistryCommandRoundtrip(opts: NativeCommandSessionOptions): Promise<unknown>;
