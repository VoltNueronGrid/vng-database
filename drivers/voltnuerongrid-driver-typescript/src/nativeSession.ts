import * as net from "node:net";
import { encodeFramedJson, readFramedJson, FramedReader } from "./nativeWire";

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
export async function nativeCommandRoundtrip(
  opts: NativeCommandSessionOptions,
  command: string,
  body: Record<string, unknown>
): Promise<unknown> {
  const connectTimeoutMs = opts.connectTimeoutMs ?? 5000;
  const idleMs = opts.idleMs ?? 30_000;
  const maxFrameBytes = opts.maxFrameBytes ?? 1_048_576;
  const rid = opts.requestIdPrefix ?? `ts-native-${command}`;

  return new Promise((resolve, reject) => {
    let socket: net.Socket;
    const timer = setTimeout(() => {
      try {
        socket?.destroy();
      } catch {
        /* ignore */
      }
      reject(new Error(`native session connect timeout after ${connectTimeoutMs}ms`));
    }, connectTimeoutMs);

    socket = net.createConnection({ host: opts.host, port: opts.port }, async () => {
      clearTimeout(timer);
      // One FramedReader per connection — shares the buffer across all reads
      // so excess bytes delivered in a single TCP segment are never discarded.
      const reader = new FramedReader(socket);
      try {
        const hello = {
          frame_type: "Hello",
          protocol_version: "v1",
          request_id: `${rid}-hello`,
          session_id: null,
          payload: { session_id: opts.sessionId, protocol: "vng-native", version: "v1" },
        };
        socket.write(encodeFramedJson(hello));
        await readFramedJson(reader, maxFrameBytes, idleMs);

        if (opts.adminApiKey) {
          const auth = {
            frame_type: "Auth",
            protocol_version: "v1",
            request_id: `${rid}-auth`,
            session_id: opts.sessionId,
            payload: { admin_api_key: opts.adminApiKey },
          };
          socket.write(encodeFramedJson(auth));
          await readFramedJson(reader, maxFrameBytes, idleMs);
        }

        const cmd = {
          frame_type: "Command",
          protocol_version: "v1",
          request_id: `${rid}-cmd`,
          session_id: opts.sessionId,
          payload: { command, body },
        };
        socket.write(encodeFramedJson(cmd));
        const result = await readFramedJson(reader, maxFrameBytes, idleMs);
        socket.end();
        resolve(result);
      } catch (e) {
        socket.destroy();
        reject(e instanceof Error ? e : new Error(String(e)));
      }
    });

    socket.once("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
  });
}

/**
 * Minimal native wire session: TCP → Hello → Auth (optional) → Command `health` (Rust driver parity for probes).
 * Returns the decoded Result payload JSON.
 */
export async function nativeHealthCommandRoundtrip(
  opts: NativeHealthSessionOptions
): Promise<unknown> {
  const connectTimeoutMs = opts.connectTimeoutMs ?? 5000;
  const idleMs = opts.idleMs ?? 30_000;
  const maxFrameBytes = opts.maxFrameBytes ?? 1_048_576;
  const rid = opts.requestIdPrefix ?? "ts-native-health";

  return new Promise((resolve, reject) => {
    let socket: net.Socket;
    const timer = setTimeout(() => {
      try {
        socket?.destroy();
      } catch {
        /* ignore */
      }
      reject(new Error(`native session connect timeout after ${connectTimeoutMs}ms`));
    }, connectTimeoutMs);

    socket = net.createConnection({ host: opts.host, port: opts.port }, async () => {
      clearTimeout(timer);
      // One FramedReader per connection — shares the buffer across all reads.
      const reader = new FramedReader(socket);
      try {
        const hello = {
          frame_type: "Hello",
          protocol_version: "v1",
          request_id: `${rid}-hello`,
          session_id: null,
          payload: { session_id: opts.sessionId, protocol: "vng-native", version: "v1" },
        };
        socket.write(encodeFramedJson(hello));
        await readFramedJson(reader, maxFrameBytes, idleMs);

        if (opts.adminApiKey) {
          const auth = {
            frame_type: "Auth",
            protocol_version: "v1",
            request_id: `${rid}-auth`,
            session_id: opts.sessionId,
            payload: { admin_api_key: opts.adminApiKey },
          };
          socket.write(encodeFramedJson(auth));
          await readFramedJson(reader, maxFrameBytes, idleMs);
        }

        const cmd = {
          frame_type: "Command",
          protocol_version: "v1",
          request_id: `${rid}-cmd`,
          session_id: opts.sessionId,
          payload: { command: "health" },
        };
        socket.write(encodeFramedJson(cmd));
        const result = await readFramedJson(reader, maxFrameBytes, idleMs);
        socket.end();
        resolve(result);
      } catch (e) {
        socket.destroy();
        reject(e instanceof Error ? e : new Error(String(e)));
      }
    });

    socket.once("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
  });
}

/** Body type for sql.execute command. */
export interface SqlExecuteBody {
  sql_batch: string;
  max_rows?: number;
}

/**
 * Native session roundtrip for `sql.execute`.
 * Sends Command frame with `command: "sql.execute"` and `body: { sql_batch, max_rows? }`.
 */
export function nativeSqlExecuteCommandRoundtrip(
  opts: NativeCommandSessionOptions,
  body: SqlExecuteBody
): Promise<unknown> {
  const frameBody: Record<string, unknown> = { sql_batch: body.sql_batch };
  if (body.max_rows !== undefined) {
    frameBody.max_rows = body.max_rows;
  }
  return nativeCommandRoundtrip(opts, "sql.execute", frameBody);
}

/** Body type for sql.analyze command. */
export interface SqlAnalyzeBody {
  sql_batch: string;
}

/**
 * Native session roundtrip for `sql.analyze`.
 * Sends Command frame with `command: "sql.analyze"` and `body: { sql_batch }`.
 */
export function nativeSqlAnalyzeCommandRoundtrip(
  opts: NativeCommandSessionOptions,
  body: SqlAnalyzeBody
): Promise<unknown> {
  return nativeCommandRoundtrip(opts, "sql.analyze", { sql_batch: body.sql_batch });
}

/** Body type for sql.route command. */
export interface SqlRouteBody {
  sql_batch: string;
}

/**
 * Native session roundtrip for `sql.route`.
 * Sends Command frame with `command: "sql.route"` and `body: { sql_batch }`.
 */
export function nativeSqlRouteCommandRoundtrip(
  opts: NativeCommandSessionOptions,
  body: SqlRouteBody
): Promise<unknown> {
  return nativeCommandRoundtrip(opts, "sql.route", { sql_batch: body.sql_batch });
}

/** Body type for sql.transaction command. */
export interface SqlTransactionBody {
  statements: string[];
  isolation_level?: string;
}

/**
 * Native session roundtrip for `sql.transaction`.
 * Sends Command frame with `command: "sql.transaction"` and `body: { statements, isolation_level? }`.
 */
export function nativeSqlTransactionCommandRoundtrip(
  opts: NativeCommandSessionOptions,
  body: SqlTransactionBody
): Promise<unknown> {
  const frameBody: Record<string, unknown> = { statements: body.statements };
  if (body.isolation_level !== undefined) {
    frameBody.isolation_level = body.isolation_level;
  }
  return nativeCommandRoundtrip(opts, "sql.transaction", frameBody);
}

/**
 * Native session roundtrip for `ingest.schema.registry`.
 * Sends Command frame with `command: "ingest.schema.registry"` and empty body `{}`.
 */
export function nativeSchemaRegistryCommandRoundtrip(
  opts: NativeCommandSessionOptions
): Promise<unknown> {
  return nativeCommandRoundtrip(opts, "ingest.schema.registry", {});
}
