from __future__ import annotations

from dataclasses import dataclass, replace
from enum import Enum
from typing import Dict, List, Optional, Tuple
import json
import os
import socket


class DriverTransportMode(str, Enum):
    HTTP = "http"
    NATIVE = "native"
    AUTO = "auto"


@dataclass
class TransportResolution:
    active: DriverTransportMode
    used_auto_resolution: bool
    notes: Optional[str] = None


@dataclass
class TransportCapabilities:
    native_available: bool
    http_available: bool


@dataclass
class AutoTransportResolution:
    active: DriverTransportMode
    fallback_triggered: bool
    fallback_reason: Optional[str] = None
    notes: Optional[str] = None


def select_transport_from_base_url(base_url: str) -> DriverTransportMode:
    b = base_url.strip().lower()
    return DriverTransportMode.NATIVE if b.startswith("vng://") else DriverTransportMode.HTTP


DEFAULT_HTTP_DISCOVERY_PORT = 8080


def parse_vng_host_for_discovery(vng_url: str) -> str:
    t = vng_url.strip()
    if not t.lower().startswith("vng://"):
        raise ValueError("expected vng:// URL")
    rest = t[6:]
    host_part = rest.split("/", maxsplit=1)[0].split("?", maxsplit=1)[0].strip()
    if not host_part:
        raise ValueError("vng URL host is empty")
    if host_part.startswith("["):
        end = host_part.find("]")
        if end <= 0:
            raise ValueError("invalid IPv6 bracket in vng URL")
        return host_part[1:end]
    last_colon = host_part.rfind(":")
    if last_colon > 0:
        maybe_port = host_part[last_colon + 1 :]
        if maybe_port.isdigit() and ":" not in host_part[:last_colon]:
            return host_part[:last_colon]
    return host_part


def infer_http_base_url_from_vng_url(vng_url: str, http_port: int) -> str:
    if http_port < 1 or http_port > 65535:
        raise ValueError("http discovery port must be 1..65535")
    host = parse_vng_host_for_discovery(vng_url)
    if ":" in host:
        return f"http://[{host}]:{http_port}"
    return f"http://{host}:{http_port}"


@dataclass
class DriverConfig:
    base_url: str
    session_id: str
    mode: str
    admin_api_key: Optional[str] = None
    operator_id: Optional[str] = None
    tenant_id: Optional[str] = None
    user_id: Optional[str] = None
    route_hint: Optional[str] = None
    http_fallback_url: Optional[str] = None
    request_timeout_ms: int = 30000
    max_retries: int = 2


@dataclass
class DriverRequest:
    method: str
    url: str
    headers: Dict[str, str]
    body_json: Optional[str] = None


def validate_config(config: DriverConfig) -> Optional[str]:
    if not config.base_url.strip():
        return "base_url must not be empty"
    if not config.session_id.strip():
        return "session_id must not be empty"

    if config.mode == "admin" and not (config.admin_api_key or "").strip():
        return "admin mode requires adminApiKey"

    if config.mode == "operator":
        if not (config.admin_api_key or "").strip() or not (config.operator_id or "").strip():
            return "operator mode requires adminApiKey and operatorId"

    if config.mode == "tenant" and not (config.tenant_id or "").strip():
        return "tenant mode requires tenantId"

    if config.http_fallback_url is not None:
        h = config.http_fallback_url.strip()
        if not h:
            return "http_fallback_url must not be empty when set"
        hl = h.lower()
        if not (hl.startswith("http://") or hl.startswith("https://")):
            return "http_fallback_url must start with http:// or https://"
        if not config.base_url.strip().lower().startswith("vng://"):
            return "http_fallback_url is only valid when base_url uses the vng:// scheme"

    return None


def http_rest_base_url(config: DriverConfig) -> str:
    b = config.base_url.strip().lower()
    if b.startswith("vng://"):
        h = (config.http_fallback_url or "").strip()
        if not h:
            raise ValueError(
                "http_fallback_url is required when base_url uses vng:// (REST APIs need an http(s) endpoint)"
            )
        return h.rstrip("/")
    return config.base_url.strip().rstrip("/")


def resolve_auto_transport(
    config: DriverConfig, caps: TransportCapabilities
) -> AutoTransportResolution:
    dual = bool((config.http_fallback_url or "").strip())
    if dual:
        if caps.native_available:
            return AutoTransportResolution(
                active=DriverTransportMode.NATIVE,
                fallback_triggered=False,
                notes="auto: dual-endpoint; native available (native-first)",
            )
        if caps.http_available:
            return AutoTransportResolution(
                active=DriverTransportMode.HTTP,
                fallback_triggered=True,
                fallback_reason="native_unavailable",
                notes="auto: dual-endpoint; fell back to http_fallback_url",
            )
        raise ValueError("no available transport: native and http are unavailable")

    base = config.base_url.strip().lower()
    if base.startswith("vng://"):
        if caps.native_available:
            return AutoTransportResolution(
                active=DriverTransportMode.NATIVE,
                fallback_triggered=False,
                notes="auto: single vng URL; native available",
            )
        if caps.http_available:
            raise ValueError(
                "native unavailable and no http_fallback_url is configured for HTTP fallback"
            )
        raise ValueError("no available transport: native and http are unavailable")

    if caps.http_available:
        return AutoTransportResolution(
            active=DriverTransportMode.HTTP,
            fallback_triggered=False,
            notes="auto: single http(s) URL",
        )
    raise ValueError("no available transport: native and http are unavailable")


def parse_discovery_http_port_str(s: str) -> Optional[int]:
    t = s.strip()
    if not t:
        return None
    try:
        p = int(t)
    except ValueError:
        return None
    if p < 1 or p > 65535:
        return None
    return p


def discovery_http_port_from_env() -> Optional[int]:
    raw = os.environ.get("VNG_HTTP_DISCOVERY_PORT")
    if raw is None:
        return None
    return parse_discovery_http_port_str(raw)


def probe_tcp_connect(host_port: str, timeout_ms: float) -> bool:
    hp = host_port.strip()
    if not hp or timeout_ms < 1:
        return False
    try:
        host, port = _parse_host_port(hp)
    except ValueError:
        return False
    try:
        with socket.create_connection((host, port), timeout=timeout_ms / 1000.0):
            return True
    except OSError:
        return False


def _parse_host_port(host_port: str) -> Tuple[str, int]:
    s = host_port.strip()
    if s.startswith("["):
        end = s.find("]")
        if end <= 0:
            raise ValueError("invalid bracket host:port")
        host = s[1:end]
        rest = s[end + 1 :]
        if not rest.startswith(":"):
            raise ValueError("expected ]:port")
        port = int(rest[1:])
        return host, port
    if ":" not in s:
        raise ValueError("invalid host:port")
    host, _, port_s = s.rpartition(":")
    port = int(port_s)
    return host, port


def _http_origin_host_port_for_probe(url: str) -> Optional[str]:
    u = url.strip()
    if u.startswith("http://"):
        rest = u[len("http://") :]
    elif u.startswith("https://"):
        rest = u[len("https://") :]
    else:
        return None
    hostport = rest.split("/")[0].split("?")[0].strip()
    return hostport or None


def _try_http_rest_base_url(config: DriverConfig) -> Optional[str]:
    b = config.base_url.strip().lower()
    if b.startswith("vng://"):
        h = (config.http_fallback_url or "").strip()
        return h.rstrip("/") if h else None
    return config.base_url.strip().rstrip("/")


def infer_transport_capabilities_tcp(
    config: DriverConfig,
    native_connect_timeout_ms: float,
    http_connect_timeout_ms: float,
) -> TransportCapabilities:
    native_available = False
    base = config.base_url.strip()
    if base.lower().startswith("vng://"):
        hp = base[6:].split("/")[0].split("?")[0].strip()
        if hp:
            native_available = probe_tcp_connect(hp, native_connect_timeout_ms)
    http_available = False
    http_base = _try_http_rest_base_url(config)
    if http_base:
        hp2 = _http_origin_host_port_for_probe(http_base)
        if hp2:
            http_available = probe_tcp_connect(hp2, http_connect_timeout_ms)
    return TransportCapabilities(
        native_available=native_available, http_available=http_available
    )


def infer_transport_capabilities_tcp_with_discovery(
    config: DriverConfig,
    native_connect_timeout_ms: float,
    http_connect_timeout_ms: float,
    discovery_http_port: Optional[int],
) -> TransportCapabilities:
    port = discovery_http_port if discovery_http_port is not None else discovery_http_port_from_env()
    effective = config
    if not (config.http_fallback_url or "").strip() and port is not None:
        b = config.base_url.strip()
        if b.lower().startswith("vng://"):
            inferred = infer_http_base_url_from_vng_url(b, port)
            effective = replace(config, http_fallback_url=inferred)
    return infer_transport_capabilities_tcp(
        effective, native_connect_timeout_ms, http_connect_timeout_ms
    )


def resolve_auto_transport_with_discovery(
    config: DriverConfig,
    caps: TransportCapabilities,
    discovery_http_port: Optional[int],
) -> AutoTransportResolution:
    port = (
        discovery_http_port
        if discovery_http_port is not None
        else discovery_http_port_from_env()
    )
    if port is None:
        return resolve_auto_transport(config, caps)
    if (config.http_fallback_url or "").strip():
        return resolve_auto_transport(config, caps)
    base = config.base_url.strip()
    if not base.lower().startswith("vng://"):
        return resolve_auto_transport(config, caps)
    inferred = infer_http_base_url_from_vng_url(base, port)
    merged = replace(config, http_fallback_url=inferred)
    return resolve_auto_transport(merged, caps)


def _build_headers(config: DriverConfig) -> Dict[str, str]:
    headers: Dict[str, str] = {
        "content-type": "application/json",
        "x-vng-session-id": config.session_id,
    }

    if config.mode in ("admin", "operator") and config.admin_api_key:
        headers["x-vng-admin-key"] = config.admin_api_key
    if config.mode == "operator" and config.operator_id:
        headers["x-vng-operator-id"] = config.operator_id
    if config.mode == "tenant" and config.tenant_id:
        headers["x-vng-tenant-id"] = config.tenant_id
    if config.mode == "tenant" and config.user_id:
        headers["x-vng-user-id"] = config.user_id
    if config.route_hint:
        headers["x-vng-route-hint"] = config.route_hint
    return headers


class VoltNueronGridDriver:
    def __init__(self, config: DriverConfig) -> None:
        error = validate_config(config)
        if error:
            raise ValueError(error)
        self._config = config

    def resolve_transport_mode(self, mode: DriverTransportMode) -> TransportResolution:
        if mode in (DriverTransportMode.HTTP, DriverTransportMode.NATIVE):
            return TransportResolution(active=mode, used_auto_resolution=False)
        active = select_transport_from_base_url(self._config.base_url)
        return TransportResolution(
            active=active,
            used_auto_resolution=True,
            notes=f"auto: selected {active.value} from base_url scheme",
        )

    def _build_post(self, path: str, payload: dict) -> DriverRequest:
        base = http_rest_base_url(self._config)
        return DriverRequest(
            method="POST",
            url=f"{base}{path}",
            headers=_build_headers(self._config),
            body_json=json.dumps(payload),
        )

    def build_health_request(self) -> DriverRequest:
        base = http_rest_base_url(self._config)
        return DriverRequest(
            method="GET",
            url=f"{base}/health",
            headers=_build_headers(self._config),
        )

    def build_sql_analyze_request(self, sql_batch: str) -> DriverRequest:
        return self._build_post("/api/v1/sql/analyze", {"sql_batch": sql_batch})

    def build_sql_route_request(self, sql_batch: str) -> DriverRequest:
        return self._build_post("/api/v1/sql/route", {"sql_batch": sql_batch})

    def build_sql_execute_request(self, sql_batch: str, max_rows: Optional[int] = None) -> DriverRequest:
        return self._build_post("/api/v1/sql/execute", {"sql_batch": sql_batch, "max_rows": max_rows})

    def build_sql_transaction_request(self, statements: List[str]) -> DriverRequest:
        return self._build_post("/api/v1/sql/transaction", {"statements": statements})

    def build_schema_registry_request(self) -> DriverRequest:
        base = http_rest_base_url(self._config)
        return DriverRequest(
            method="GET",
            url=f"{base}/api/v1/ingest/schema/registry",
            headers=_build_headers(self._config),
        )

