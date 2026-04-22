import * as net from "node:net";

/** Length-prefixed JSON frame (native wire v1). */
export function encodeFramedJson(payload: unknown): Buffer {
  const body = Buffer.from(JSON.stringify(payload), "utf8");
  const header = Buffer.allocUnsafe(4);
  header.writeUInt32BE(body.length, 0);
  return Buffer.concat([header, body]);
}

/**
 * Stateful framed-JSON reader that maintains a persistent buffer across reads.
 * Required because a single TCP `data` event may deliver more bytes than the
 * current `readExactly` call requests; excess bytes must not be discarded.
 *
 * Create one FramedReader per socket and pass it to every `readFramedJson`
 * call on that socket so the shared buffer is preserved between reads.
 */
export class FramedReader {
  private buf: Buffer = Buffer.alloc(0);
  private waiters: Array<{ needed: number; resolve: (b: Buffer) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }> = [];

  constructor(private socket: net.Socket) {
    socket.on("data", (chunk: Buffer) => {
      this.buf = Buffer.concat([this.buf, chunk]);
      this.drain();
    });
    socket.once("error", (e) => {
      for (const w of this.waiters) {
        clearTimeout(w.timer);
        w.reject(e);
      }
      this.waiters = [];
    });
    socket.once("close", () => {
      const e = new Error("socket closed before read completed");
      for (const w of this.waiters) {
        clearTimeout(w.timer);
        w.reject(e);
      }
      this.waiters = [];
    });
  }

  private drain() {
    while (this.waiters.length > 0 && this.buf.length >= this.waiters[0].needed) {
      const w = this.waiters.shift()!;
      clearTimeout(w.timer);
      const data = this.buf.subarray(0, w.needed);
      this.buf = this.buf.subarray(w.needed);
      w.resolve(Buffer.from(data)); // copy to avoid subarray aliasing
    }
  }

  readExactly(n: number, idleMs: number): Promise<Buffer> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const idx = this.waiters.findIndex((w) => w.resolve === resolve);
        if (idx >= 0) this.waiters.splice(idx, 1);
        reject(new Error(`native wire read timeout after ${idleMs}ms`));
      }, idleMs);
      this.waiters.push({ needed: n, resolve, reject, timer });
      this.drain(); // satisfy immediately if data already buffered
    });
  }
}

/** Reads one framed JSON value from the socket (4-byte big-endian length + UTF-8 JSON). */
export async function readFramedJson(
  socket: net.Socket,
  maxPayloadBytes: number,
  idleMs: number
): Promise<unknown>;
/** Overload: accepts a pre-built FramedReader to share the buffer across calls. */
export async function readFramedJson(
  reader: FramedReader,
  maxPayloadBytes: number,
  idleMs: number
): Promise<unknown>;
export async function readFramedJson(
  socketOrReader: net.Socket | FramedReader,
  maxPayloadBytes: number,
  idleMs: number
): Promise<unknown> {
  const reader =
    socketOrReader instanceof FramedReader
      ? socketOrReader
      : new FramedReader(socketOrReader);

  const lenBuf = await reader.readExactly(4, idleMs);
  const len = lenBuf.readUInt32BE(0);
  if (len > maxPayloadBytes) {
    throw new Error(`native frame payload ${len} exceeds max ${maxPayloadBytes}`);
  }
  const body = await reader.readExactly(len, idleMs);
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
