"""Length-prefixed JSON native wire (v1) — minimal TCP helpers for probes and tests."""

from __future__ import annotations

import json
import socket
import struct
from typing import Any, Tuple


def encode_framed_json(payload: Any) -> bytes:
    body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    return struct.pack(">I", len(body)) + body


def read_exact(sock: socket.socket, n: int, idle_s: float) -> bytes:
    sock.settimeout(idle_s)
    chunks: list[bytes] = []
    got = 0
    while got < n:
        chunk = sock.recv(n - got)
        if not chunk:
            raise OSError("connection closed before read completed")
        chunks.append(chunk)
        got += len(chunk)
    return b"".join(chunks)


def read_framed_json(sock: socket.socket, max_payload_bytes: int, idle_s: float) -> Any:
    raw_len = read_exact(sock, 4, idle_s)
    (length,) = struct.unpack(">I", raw_len)
    if length > max_payload_bytes:
        raise ValueError(f"native frame payload {length} exceeds max {max_payload_bytes}")
    body = read_exact(sock, length, idle_s)
    return json.loads(body.decode("utf-8"))


def native_wire_roundtrip(
    host: str,
    port: int,
    outgoing: Any,
    *,
    connect_timeout_s: float = 5.0,
    idle_s: float = 30.0,
    max_frame_bytes: int = 1_048_576,
) -> Any:
    """TCP connect, send one framed JSON message, read one framed JSON reply."""
    addr = (host, port)
    with socket.create_connection(addr, timeout=connect_timeout_s) as sock:
        sock.sendall(encode_framed_json(outgoing))
        return read_framed_json(sock, max_frame_bytes, idle_s)


def split_host_port_for_tcp(hostport: str) -> Tuple[str, int]:
    """Parses `host:port` for TCP helpers (IPv4 / hostname; not full URL parsing)."""
    hp = hostport.strip()
    if ":" not in hp:
        raise ValueError("expected host:port")
    host, _, port_s = hp.rpartition(":")
    if host.startswith("[") and host.endswith("]"):
        host = host[1:-1]
    return host, int(port_s)
