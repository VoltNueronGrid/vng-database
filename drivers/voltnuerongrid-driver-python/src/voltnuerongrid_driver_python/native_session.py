"""Minimal native wire session: Hello → Auth (optional) → Command health (parity with TS `nativeSession.ts`)."""

from __future__ import annotations

from typing import Any, Dict, Optional

from .native_wire import encode_framed_json, read_framed_json


def native_health_command_roundtrip(
    host: str,
    port: int,
    session_id: str,
    *,
    admin_api_key: Optional[str] = None,
    request_id_prefix: str = "py-native-health",
    connect_timeout_s: float = 5.0,
    idle_s: float = 30.0,
    max_frame_bytes: int = 1_048_576,
) -> Any:
    import socket

    addr = (host, port)
    rid = request_id_prefix
    with socket.create_connection(addr, timeout=connect_timeout_s) as sock:
        hello: Dict[str, Any] = {
            "frame_type": "Hello",
            "protocol_version": "v1",
            "request_id": f"{rid}-hello",
            "session_id": None,
            "payload": {
                "session_id": session_id,
                "protocol": "vng-native",
                "version": "v1",
            },
        }
        sock.sendall(encode_framed_json(hello))
        read_framed_json(sock, max_frame_bytes, idle_s)

        if admin_api_key:
            auth = {
                "frame_type": "Auth",
                "protocol_version": "v1",
                "request_id": f"{rid}-auth",
                "session_id": session_id,
                "payload": {"admin_api_key": admin_api_key},
            }
            sock.sendall(encode_framed_json(auth))
            read_framed_json(sock, max_frame_bytes, idle_s)

        cmd = {
            "frame_type": "Command",
            "protocol_version": "v1",
            "request_id": f"{rid}-cmd",
            "session_id": session_id,
            "payload": {"command": "health"},
        }
        sock.sendall(encode_framed_json(cmd))
        return read_framed_json(sock, max_frame_bytes, idle_s)
