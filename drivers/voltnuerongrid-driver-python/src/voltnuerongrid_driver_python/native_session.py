"""Minimal native wire session: Hello → Auth (optional) → Command (parity with TS `nativeSession.ts`)."""

from __future__ import annotations

import socket
from typing import Any, Dict, List, Optional

from .native_wire import encode_framed_json, read_framed_json


def native_command_roundtrip(
    host: str,
    port: int,
    session_id: str,
    command: str,
    body: Dict[str, Any],
    *,
    admin_api_key: Optional[str] = None,
    request_id_prefix: str = "py-native",
    connect_timeout_s: float = 5.0,
    idle_s: float = 30.0,
    max_frame_bytes: int = 1_048_576,
) -> Any:
    """General helper: TCP connect → Hello → Auth (optional) → Command → read Result → close socket."""
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

        payload: Dict[str, Any] = {"command": command}
        if body:
            payload["body"] = body

        cmd = {
            "frame_type": "Command",
            "protocol_version": "v1",
            "request_id": f"{rid}-cmd",
            "session_id": session_id,
            "payload": payload,
        }
        sock.sendall(encode_framed_json(cmd))
        return read_framed_json(sock, max_frame_bytes, idle_s)


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
    """Health command roundtrip — wraps native_command_roundtrip with command='health'."""
    return native_command_roundtrip(
        host,
        port,
        session_id,
        "health",
        {},
        admin_api_key=admin_api_key,
        request_id_prefix=request_id_prefix,
        connect_timeout_s=connect_timeout_s,
        idle_s=idle_s,
        max_frame_bytes=max_frame_bytes,
    )


def native_sql_execute_command_roundtrip(
    host: str,
    port: int,
    session_id: str,
    sql_batch: str,
    *,
    max_rows: Optional[int] = None,
    admin_api_key: Optional[str] = None,
    request_id_prefix: str = "py-native-sql-execute",
    connect_timeout_s: float = 5.0,
    idle_s: float = 30.0,
    max_frame_bytes: int = 1_048_576,
) -> Any:
    """SQL execute command roundtrip — command='sql.execute'."""
    body: Dict[str, Any] = {"sql_batch": sql_batch}
    if max_rows is not None:
        body["max_rows"] = max_rows
    return native_command_roundtrip(
        host,
        port,
        session_id,
        "sql.execute",
        body,
        admin_api_key=admin_api_key,
        request_id_prefix=request_id_prefix,
        connect_timeout_s=connect_timeout_s,
        idle_s=idle_s,
        max_frame_bytes=max_frame_bytes,
    )


def native_sql_analyze_command_roundtrip(
    host: str,
    port: int,
    session_id: str,
    sql_batch: str,
    *,
    admin_api_key: Optional[str] = None,
    request_id_prefix: str = "py-native-sql-analyze",
    connect_timeout_s: float = 5.0,
    idle_s: float = 30.0,
    max_frame_bytes: int = 1_048_576,
) -> Any:
    """SQL analyze command roundtrip — command='sql.analyze'."""
    return native_command_roundtrip(
        host,
        port,
        session_id,
        "sql.analyze",
        {"sql_batch": sql_batch},
        admin_api_key=admin_api_key,
        request_id_prefix=request_id_prefix,
        connect_timeout_s=connect_timeout_s,
        idle_s=idle_s,
        max_frame_bytes=max_frame_bytes,
    )


def native_sql_route_command_roundtrip(
    host: str,
    port: int,
    session_id: str,
    sql_batch: str,
    *,
    admin_api_key: Optional[str] = None,
    request_id_prefix: str = "py-native-sql-route",
    connect_timeout_s: float = 5.0,
    idle_s: float = 30.0,
    max_frame_bytes: int = 1_048_576,
) -> Any:
    """SQL route command roundtrip — command='sql.route'."""
    return native_command_roundtrip(
        host,
        port,
        session_id,
        "sql.route",
        {"sql_batch": sql_batch},
        admin_api_key=admin_api_key,
        request_id_prefix=request_id_prefix,
        connect_timeout_s=connect_timeout_s,
        idle_s=idle_s,
        max_frame_bytes=max_frame_bytes,
    )


def native_sql_transaction_command_roundtrip(
    host: str,
    port: int,
    session_id: str,
    statements: List[str],
    *,
    isolation_level: Optional[str] = None,
    admin_api_key: Optional[str] = None,
    request_id_prefix: str = "py-native-sql-transaction",
    connect_timeout_s: float = 5.0,
    idle_s: float = 30.0,
    max_frame_bytes: int = 1_048_576,
) -> Any:
    """SQL transaction command roundtrip — command='sql.transaction'."""
    body: Dict[str, Any] = {"statements": statements}
    if isolation_level is not None:
        body["isolation_level"] = isolation_level
    return native_command_roundtrip(
        host,
        port,
        session_id,
        "sql.transaction",
        body,
        admin_api_key=admin_api_key,
        request_id_prefix=request_id_prefix,
        connect_timeout_s=connect_timeout_s,
        idle_s=idle_s,
        max_frame_bytes=max_frame_bytes,
    )


def native_schema_registry_command_roundtrip(
    host: str,
    port: int,
    session_id: str,
    *,
    admin_api_key: Optional[str] = None,
    request_id_prefix: str = "py-native-schema-registry",
    connect_timeout_s: float = 5.0,
    idle_s: float = 30.0,
    max_frame_bytes: int = 1_048_576,
) -> Any:
    """Schema registry command roundtrip — command='ingest.schema.registry'."""
    return native_command_roundtrip(
        host,
        port,
        session_id,
        "ingest.schema.registry",
        {},
        admin_api_key=admin_api_key,
        request_id_prefix=request_id_prefix,
        connect_timeout_s=connect_timeout_s,
        idle_s=idle_s,
        max_frame_bytes=max_frame_bytes,
    )
