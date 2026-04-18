import * as net from "node:net";

/** Length-prefixed JSON frame (native wire v1). */
export function encodeFramedJson(payload: unknown): Buffer {
  const body = Buffer.from(JSON.stringify(payload), "utf8");
  const header = Buffer.allocUnsafe(4);
  header.writeUInt32BE(body.length, 0);
  return Buffer.concat([header, body]);
}

function readExactly(socket: net.Socket, n: number, idleMs: number): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let total = 0;

    const timer = setTimeout(() => {
      cleanup();
      socket.destroy();
      reject(new Error(`native wire read timeout after ${idleMs}ms`));
    }, idleMs);

    const cleanup = () => {
      clearTimeout(timer);
      socket.removeListener("data", onData);
      socket.removeListener("error", onErr);
      socket.removeListener("close", onClose);
    };

    const tryFinish = () => {
      if (total < n) {
        return;
      }
      const joined = Buffer.concat(chunks);
      cleanup();
      resolve(joined.subarray(0, n));
    };

    const onData = (chunk: Buffer) => {
      chunks.push(chunk);
      total += chunk.length;
      tryFinish();
    };

    const onErr = (e: Error) => {
      cleanup();
      reject(e);
    };

    const onClose = () => {
      cleanup();
      reject(new Error("socket closed before read completed"));
    };

    socket.on("data", onData);
    socket.once("error", onErr);
    socket.once("close", onClose);

    const pending = socket.read();
    if (pending) {
      onData(pending);
    }
  });
}

/** Reads one framed JSON value from the socket (4-byte big-endian length + UTF-8 JSON). */
export async function readFramedJson(
  socket: net.Socket,
  maxPayloadBytes: number,
  idleMs: number
): Promise<unknown> {
  const lenBuf = await readExactly(socket, 4, idleMs);
  const len = lenBuf.readUInt32BE(0);
  if (len > maxPayloadBytes) {
    throw new Error(`native frame payload ${len} exceeds max ${maxPayloadBytes}`);
  }
  const body = await readExactly(socket, len, idleMs);
  return JSON.parse(body.toString("utf8"));
}

export interface NativeWireRoundtripOptions {
  connectTimeoutMs?: number;
  idleMs?: number;
  maxFrameBytes?: number;
}

/**
 * TCP connect + one framed write + one framed read (for probes and minimal native I/O tests).
 * Caller supplies a valid protocol frame (e.g. Hello / Command per `native-protocol-v1.md`).
 */
export function nativeWireRoundtrip(
  host: string,
  port: number,
  outgoing: unknown,
  opts?: NativeWireRoundtripOptions
): Promise<unknown> {
  const connectTimeoutMs = opts?.connectTimeoutMs ?? 5000;
  const idleMs = opts?.idleMs ?? 30_000;
  const maxFrameBytes = opts?.maxFrameBytes ?? 1_048_576;

  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error(`native wire connect timeout after ${connectTimeoutMs}ms`));
    }, connectTimeoutMs);

    const socket = net.createConnection({ host, port }, () => {
      clearTimeout(timer);
      const payload = encodeFramedJson(outgoing);
      socket.write(payload, (err) => {
        if (err) {
          socket.destroy();
          reject(err);
          return;
        }
        readFramedJson(socket, maxFrameBytes, idleMs)
          .then((incoming) => {
            socket.end();
            resolve(incoming);
          })
          .catch((e: unknown) => {
            socket.destroy();
            reject(e instanceof Error ? e : new Error(String(e)));
          });
      });
    });

    socket.once("error", (e) => {
      clearTimeout(timer);
      socket.destroy();
      reject(e);
    });
  });
}
