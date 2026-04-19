from __future__ import annotations

import time
import urllib.error
import urllib.request
from typing import Callable, Optional, Tuple

from .driver import DriverConfig, DriverRequest

DEFAULT_HTTP_REQUEST_TIMEOUT_MS = 30_000
DEFAULT_HTTP_MAX_RETRIES = 2


def is_retryable_http_status(status: int) -> bool:
    """Matches Rust `is_retryable_http_status`."""
    return status in (408, 425, 429, 500, 502, 503, 504)


class DriverError(Exception):
    """Typed error (driver-core-contract §6)."""

    def __init__(self, kind: str, message: str, status_code: Optional[int] = None) -> None:
        super().__init__(message)
        self.kind = kind
        self.status_code = status_code


def perform_driver_http_request(
    request: DriverRequest,
    config: DriverConfig,
    *,
    urlopen: Optional[Callable[..., object]] = None,
) -> Tuple[int, str]:
    """
    HTTP execution with per-attempt timeout and retries (driver-core-contract §5 hooks).
    """
    opener: Callable[..., object] = urlopen or urllib.request.urlopen
    timeout_s = max(config.request_timeout_ms, 100) / 1000.0
    max_retries = max(0, min(config.max_retries, 20))
    data: Optional[bytes] = None
    if request.method == "POST" and request.body_json:
        data = request.body_json.encode("utf-8")

    attempt = 0
    while attempt <= max_retries:
        req = urllib.request.Request(
            request.url,
            data=data,
            headers=request.headers,
            method=request.method,
        )
        try:
            with opener(req, timeout=timeout_s) as resp:  # type: ignore[misc]
                status = int(resp.getcode())
                body = resp.read().decode("utf-8", errors="replace")
        except urllib.error.HTTPError as e:
            status = int(e.code)
            body = e.read().decode("utf-8", errors="replace")
        except OSError as e:
            if attempt < max_retries:
                time.sleep(min(0.25 * (2**attempt), 2.0))
                attempt += 1
                continue
            raise DriverError("transport", f"HTTP request failed: {e}") from e

        if is_retryable_http_status(status) and attempt < max_retries:
            time.sleep(min(0.25 * (2**attempt), 2.0))
            attempt += 1
            continue
        return status, body

    raise DriverError("transport", "exhausted HTTP retries")
