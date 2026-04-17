from dataclasses import dataclass
from typing import Dict, List, Optional
import json


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

    return None


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

    def _build_post(self, path: str, payload: dict) -> DriverRequest:
        return DriverRequest(
            method="POST",
            url=f"{self._config.base_url}{path}",
            headers=_build_headers(self._config),
            body_json=json.dumps(payload),
        )

    def build_health_request(self) -> DriverRequest:
        return DriverRequest(
            method="GET",
            url=f"{self._config.base_url}/health",
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
        return DriverRequest(
            method="GET",
            url=f"{self._config.base_url}/api/v1/ingest/schema/registry",
            headers=_build_headers(self._config),
        )

