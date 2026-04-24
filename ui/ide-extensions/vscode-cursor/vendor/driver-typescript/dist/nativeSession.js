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
var __importStar = (this && this.__importStar) || function (mod) {
    if (mod && mod.__esModule) return mod;
    var result = {};
    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
    __setModuleDefault(result, mod);
    return result;
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.nativeCommandRoundtrip = nativeCommandRoundtrip;
exports.nativeHealthCommandRoundtrip = nativeHealthCommandRoundtrip;
exports.nativeSqlExecuteCommandRoundtrip = nativeSqlExecuteCommandRoundtrip;
exports.nativeSqlAnalyzeCommandRoundtrip = nativeSqlAnalyzeCommandRoundtrip;
exports.nativeSqlRouteCommandRoundtrip = nativeSqlRouteCommandRoundtrip;
exports.nativeSqlTransactionCommandRoundtrip = nativeSqlTransactionCommandRoundtrip;
exports.nativeSchemaRegistryCommandRoundtrip = nativeSchemaRegistryCommandRoundtrip;
const net = __importStar(require("node:net"));
const nativeWire_1 = require("./nativeWire");
/**
 * Core session helper: TCP connect → Hello → Auth (optional) → send Command frame →
 * read Result frame → end socket.
 *
 * Returns the decoded Result payload JSON.
 */
async function nativeCommandRoundtrip(opts, command, body) {
    const connectTimeoutMs = opts.connectTimeoutMs ?? 5000;
    const idleMs = opts.idleMs ?? 30_000;
    const maxFrameBytes = opts.maxFrameBytes ?? 1_048_576;
    const rid = opts.requestIdPrefix ?? `ts-native-${command}`;
    return new Promise((resolve, reject) => {
        let socket;
        const timer = setTimeout(() => {
            try {
                socket?.destroy();
            }
            catch {
                /* ignore */
            }
            reject(new Error(`native session connect timeout after ${connectTimeoutMs}ms`));
        }, connectTimeoutMs);
        socket = net.createConnection({ host: opts.host, port: opts.port }, async () => {
            clearTimeout(timer);
            // One FramedReader per connection — shares the buffer across all reads
            // so excess bytes delivered in a single TCP segment are never discarded.
            const reader = new nativeWire_1.FramedReader(socket);
            try {
                const hello = {
                    frame_type: "Hello",
                    protocol_version: "v1",
                    request_id: `${rid}-hello`,
                    session_id: null,
                    payload: { session_id: opts.sessionId, protocol: "vng-native", version: "v1" },
                };
                socket.write((0, nativeWire_1.encodeFramedJson)(hello));
                await (0, nativeWire_1.readFramedJson)(reader, maxFrameBytes, idleMs);
                if (opts.adminApiKey) {
                    const auth = {
                        frame_type: "Auth",
                        protocol_version: "v1",
                        request_id: `${rid}-auth`,
                        session_id: opts.sessionId,
                        payload: { admin_api_key: opts.adminApiKey },
                    };
                    socket.write((0, nativeWire_1.encodeFramedJson)(auth));
                    await (0, nativeWire_1.readFramedJson)(reader, maxFrameBytes, idleMs);
                }
                const cmd = {
                    frame_type: "Command",
                    protocol_version: "v1",
                    request_id: `${rid}-cmd`,
                    session_id: opts.sessionId,
                    payload: { command, body },
                };
                socket.write((0, nativeWire_1.encodeFramedJson)(cmd));
                const result = await (0, nativeWire_1.readFramedJson)(reader, maxFrameBytes, idleMs);
                socket.end();
                resolve(result);
            }
            catch (e) {
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
async function nativeHealthCommandRoundtrip(opts) {
    const connectTimeoutMs = opts.connectTimeoutMs ?? 5000;
    const idleMs = opts.idleMs ?? 30_000;
    const maxFrameBytes = opts.maxFrameBytes ?? 1_048_576;
    const rid = opts.requestIdPrefix ?? "ts-native-health";
    return new Promise((resolve, reject) => {
        let socket;
        const timer = setTimeout(() => {
            try {
                socket?.destroy();
            }
            catch {
                /* ignore */
            }
            reject(new Error(`native session connect timeout after ${connectTimeoutMs}ms`));
        }, connectTimeoutMs);
        socket = net.createConnection({ host: opts.host, port: opts.port }, async () => {
            clearTimeout(timer);
            // One FramedReader per connection — shares the buffer across all reads.
            const reader = new nativeWire_1.FramedReader(socket);
            try {
                const hello = {
                    frame_type: "Hello",
                    protocol_version: "v1",
                    request_id: `${rid}-hello`,
                    session_id: null,
                    payload: { session_id: opts.sessionId, protocol: "vng-native", version: "v1" },
                };
                socket.write((0, nativeWire_1.encodeFramedJson)(hello));
                await (0, nativeWire_1.readFramedJson)(reader, maxFrameBytes, idleMs);
                if (opts.adminApiKey) {
                    const auth = {
                        frame_type: "Auth",
                        protocol_version: "v1",
                        request_id: `${rid}-auth`,
                        session_id: opts.sessionId,
                        payload: { admin_api_key: opts.adminApiKey },
                    };
                    socket.write((0, nativeWire_1.encodeFramedJson)(auth));
                    await (0, nativeWire_1.readFramedJson)(reader, maxFrameBytes, idleMs);
                }
                const cmd = {
                    frame_type: "Command",
                    protocol_version: "v1",
                    request_id: `${rid}-cmd`,
                    session_id: opts.sessionId,
                    payload: { command: "health" },
                };
                socket.write((0, nativeWire_1.encodeFramedJson)(cmd));
                const result = await (0, nativeWire_1.readFramedJson)(reader, maxFrameBytes, idleMs);
                socket.end();
                resolve(result);
            }
            catch (e) {
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
 * Native session roundtrip for `sql.execute`.
 * Sends Command frame with `command: "sql.execute"` and `body: { sql_batch, max_rows? }`.
 */
function nativeSqlExecuteCommandRoundtrip(opts, body) {
    const frameBody = { sql_batch: body.sql_batch };
    if (body.max_rows !== undefined) {
        frameBody.max_rows = body.max_rows;
    }
    return nativeCommandRoundtrip(opts, "sql.execute", frameBody);
}
/**
 * Native session roundtrip for `sql.analyze`.
 * Sends Command frame with `command: "sql.analyze"` and `body: { sql_batch }`.
 */
function nativeSqlAnalyzeCommandRoundtrip(opts, body) {
    return nativeCommandRoundtrip(opts, "sql.analyze", { sql_batch: body.sql_batch });
}
/**
 * Native session roundtrip for `sql.route`.
 * Sends Command frame with `command: "sql.route"` and `body: { sql_batch }`.
 */
function nativeSqlRouteCommandRoundtrip(opts, body) {
    return nativeCommandRoundtrip(opts, "sql.route", { sql_batch: body.sql_batch });
}
/**
 * Native session roundtrip for `sql.transaction`.
 * Sends Command frame with `command: "sql.transaction"` and `body: { statements, isolation_level? }`.
 */
function nativeSqlTransactionCommandRoundtrip(opts, body) {
    const frameBody = { statements: body.statements };
    if (body.isolation_level !== undefined) {
        frameBody.isolation_level = body.isolation_level;
    }
    return nativeCommandRoundtrip(opts, "sql.transaction", frameBody);
}
/**
 * Native session roundtrip for `ingest.schema.registry`.
 * Sends Command frame with `command: "ingest.schema.registry"` and empty body `{}`.
 */
function nativeSchemaRegistryCommandRoundtrip(opts) {
    return nativeCommandRoundtrip(opts, "ingest.schema.registry", {});
}
