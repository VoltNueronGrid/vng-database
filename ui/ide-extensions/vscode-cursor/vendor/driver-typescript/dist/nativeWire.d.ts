import * as net from "node:net";
/** Length-prefixed JSON frame (native wire v1). */
export declare function encodeFramedJson(payload: unknown): Buffer;
/**
 * Stateful framed-JSON reader that maintains a persistent buffer across reads.
 * Required because a single TCP `data` event may deliver more bytes than the
 * current `readExactly` call requests; excess bytes must not be discarded.
 *
 * Create one FramedReader per socket and pass it to every `readFramedJson`
 * call on that socket so the shared buffer is preserved between reads.
 */
export declare class FramedReader {
    private socket;
    private buf;
    private waiters;
    constructor(socket: net.Socket);
    private drain;
    readExactly(n: number, idleMs: number): Promise<Buffer>;
}
/** Reads one framed JSON value from the socket (4-byte big-endian length + UTF-8 JSON). */
export declare function readFramedJson(socket: net.Socket, maxPayloadBytes: number, idleMs: number): Promise<unknown>;
/** Overload: accepts a pre-built FramedReader to share the buffer across calls. */
export declare function readFramedJson(reader: FramedReader, maxPayloadBytes: number, idleMs: number): Promise<unknown>;
export interface NativeWireRoundtripOptions {
    connectTimeoutMs?: number;
    idleMs?: number;
    maxFrameBytes?: number;
}
/**
 * TCP connect + one framed write + one framed read (for probes and minimal native I/O tests).
 * Caller supplies a valid protocol frame (e.g. Hello / Command per `native-protocol-v1.md`).
 */
export declare function nativeWireRoundtrip(host: string, port: number, outgoing: unknown, opts?: NativeWireRoundtripOptions): Promise<unknown>;
