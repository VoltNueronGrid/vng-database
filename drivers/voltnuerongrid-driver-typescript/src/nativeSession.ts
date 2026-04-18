import * as net from "node:net";
import { encodeFramedJson, readFramedJson } from "./nativeWire";

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
      try {
        const hello = {
          frame_type: "Hello",
          protocol_version: "v1",
          request_id: `${rid}-hello`,
          session_id: null,
          payload: { session_id: opts.sessionId, protocol: "vng-native", version: "v1" },
        };
        socket.write(encodeFramedJson(hello));
        await readFramedJson(socket, maxFrameBytes, idleMs);

        if (opts.adminApiKey) {
          const auth = {
            frame_type: "Auth",
            protocol_version: "v1",
            request_id: `${rid}-auth`,
            session_id: opts.sessionId,
            payload: { admin_api_key: opts.adminApiKey },
          };
          socket.write(encodeFramedJson(auth));
          await readFramedJson(socket, maxFrameBytes, idleMs);
        }

        const cmd = {
          frame_type: "Command",
          protocol_version: "v1",
          request_id: `${rid}-cmd`,
          session_id: opts.sessionId,
          payload: { command: "health" },
        };
        socket.write(encodeFramedJson(cmd));
        const result = await readFramedJson(socket, maxFrameBytes, idleMs);
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
