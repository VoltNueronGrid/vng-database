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
exports.FramedReader = void 0;
exports.encodeFramedJson = encodeFramedJson;
exports.readFramedJson = readFramedJson;
exports.nativeWireRoundtrip = nativeWireRoundtrip;
const net = __importStar(require("node:net"));
/** Length-prefixed JSON frame (native wire v1). */
function encodeFramedJson(payload) {
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
class FramedReader {
    socket;
    buf = Buffer.alloc(0);
    waiters = [];
    constructor(socket) {
        this.socket = socket;
        socket.on("data", (chunk) => {
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
    drain() {
        while (this.waiters.length > 0 && this.buf.length >= this.waiters[0].needed) {
            const w = this.waiters.shift();
            clearTimeout(w.timer);
            const data = this.buf.subarray(0, w.needed);
            this.buf = this.buf.subarray(w.needed);
            w.resolve(Buffer.from(data)); // copy to avoid subarray aliasing
        }
    }
    readExactly(n, idleMs) {
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                const idx = this.waiters.findIndex((w) => w.resolve === resolve);
                if (idx >= 0)
                    this.waiters.splice(idx, 1);
                reject(new Error(`native wire read timeout after ${idleMs}ms`));
            }, idleMs);
            this.waiters.push({ needed: n, resolve, reject, timer });
            this.drain(); // satisfy immediately if data already buffered
        });
    }
}
exports.FramedReader = FramedReader;
async function readFramedJson(socketOrReader, maxPayloadBytes, idleMs) {
    const reader = socketOrReader instanceof FramedReader
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
/**
 * TCP connect + one framed write + one framed read (for probes and minimal native I/O tests).
 * Caller supplies a valid protocol frame (e.g. Hello / Command per `native-protocol-v1.md`).
 */
function nativeWireRoundtrip(host, port, outgoing, opts) {
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
                    .catch((e) => {
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
