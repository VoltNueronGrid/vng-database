"""
P9: Python Driver Conformance Skeleton
=======================================
Validates that the Python driver satisfies the VoltNueronGrid driver
conformance test suite (drivers/conformance/conformance-test-suite.md).

Requires:
    pytest >= 7.0

Run with:
    pytest tests/conformance_stub.py -v

Note: These tests do NOT require a live server — they validate driver
configuration and request-building behaviour in isolation using mocks.
"""

import pytest
from unittest.mock import patch, MagicMock


# ---------------------------------------------------------------------------
# Helpers — lazy import so the test file doesn't fail if the package is not
# installed yet.  When running the gate, install the package first:
#   pip install -e drivers/voltnuerongrid-driver-python
# ---------------------------------------------------------------------------

def _driver():
    """Lazy import of the VoltNueronGrid Python driver module."""
    try:
        import voltnuerongrid_driver  # type: ignore[import]
        return voltnuerongrid_driver
    except ImportError:
        pytest.skip("voltnuerongrid_driver package not installed — skeleton only")


# ---------------------------------------------------------------------------
# C1: Configuration Validation (cases 1-7)
# ---------------------------------------------------------------------------

class TestC1ConfigValidation:
    """C1 — Configuration must be validated on driver construction."""

    def test_c1_case1_admin_mode_requires_api_key(self):
        """admin mode without adminApiKey must raise a configuration error."""
        drv = _driver()
        with pytest.raises((ValueError, drv.ConfigurationError, Exception)) as exc_info:
            drv.connect(mode="admin", base_url="http://127.0.0.1:8080")
        assert "adminApiKey" in str(exc_info.value).lower() or "admin" in str(exc_info.value).lower()

    def test_c1_case2_operator_mode_requires_operator_id(self):
        """operator mode without operatorId must raise a configuration error."""
        drv = _driver()
        with pytest.raises(Exception):
            drv.connect(mode="operator", base_url="http://127.0.0.1:8080", admin_api_key="key")

    def test_c1_case3_tenant_mode_requires_tenant_id(self):
        """tenant mode without tenantId must raise a configuration error."""
        drv = _driver()
        with pytest.raises(Exception):
            drv.connect(mode="tenant", base_url="http://127.0.0.1:8080")

    def test_c1_case4_valid_admin_config_no_error(self):
        """A fully-specified admin config must not raise on construction."""
        drv = _driver()
        # No exception expected — driver object created successfully.
        conn = drv.connect(mode="admin", base_url="http://127.0.0.1:8080", admin_api_key="test-key")
        assert conn is not None

    def test_c1_case5_valid_tenant_config_no_error(self):
        """A fully-specified tenant config must not raise on construction."""
        drv = _driver()
        conn = drv.connect(
            mode="tenant",
            base_url="http://127.0.0.1:8080",
            tenant_id="acme",
            user_id="analyst",
        )
        assert conn is not None

    def test_c1_case6_empty_base_url_raises(self):
        """An empty baseUrl must raise a configuration error."""
        drv = _driver()
        with pytest.raises(Exception) as exc_info:
            drv.connect(mode="admin", base_url="", admin_api_key="key")
        assert "url" in str(exc_info.value).lower() or "base" in str(exc_info.value).lower()

    def test_c1_case7_trailing_slash_normalised(self):
        """Trailing slash on baseUrl must be stripped during construction."""
        drv = _driver()
        conn = drv.connect(mode="admin", base_url="http://127.0.0.1:8080/", admin_api_key="key")
        # The stored base_url must not end with /
        url = getattr(conn, "base_url", getattr(conn, "baseUrl", None))
        if url is not None:
            assert not url.endswith("/"), f"baseUrl should not end with /: {url!r}"


# ---------------------------------------------------------------------------
# C3: Request Building (cases 11-16) — verified via mock HTTP layer
# ---------------------------------------------------------------------------

class TestC3RequestBuilding:
    """C3 — Driver must produce correct HTTP request headers."""

    def _mock_execute(self, drv, conn):
        """Return a MagicMock that captures the last request headers."""
        captured = {}

        def fake_post(url, json, headers, **kwargs):
            captured.update(headers)
            resp = MagicMock()
            resp.status_code = 200
            resp.json.return_value = {"status": "ok", "route_path": "oltp"}
            return resp

        return captured, fake_post

    def test_c3_case11_admin_query_includes_admin_key(self):
        """Admin mode query must include x-vng-admin-key header."""
        drv = _driver()
        conn = drv.connect(mode="admin", base_url="http://127.0.0.1:8080", admin_api_key="secret-key")
        captured, fake_post = self._mock_execute(drv, conn)
        with patch("requests.post", side_effect=fake_post):
            try:
                conn.execute("SELECT 1")
            except Exception:
                pass
        assert "x-vng-admin-key" in captured, "admin mode must set x-vng-admin-key header"
        assert captured["x-vng-admin-key"] == "secret-key"

    def test_c3_case12_tenant_query_includes_tenant_headers(self):
        """Tenant mode query must include x-vng-tenant-id and x-vng-user-id headers."""
        drv = _driver()
        conn = drv.connect(
            mode="tenant",
            base_url="http://127.0.0.1:8080",
            tenant_id="acme",
            user_id="analyst",
        )
        captured, fake_post = self._mock_execute(drv, conn)
        with patch("requests.post", side_effect=fake_post):
            try:
                conn.execute("SELECT 1")
            except Exception:
                pass
        assert "x-vng-tenant-id" in captured, "tenant mode must set x-vng-tenant-id header"
        assert "x-vng-user-id" in captured, "tenant mode must set x-vng-user-id header"

    def test_c3_case15_database_scoped_query_includes_database_header(self):
        """When database is set in config, x-vng-database header must be present."""
        drv = _driver()
        conn = drv.connect(
            mode="admin",
            base_url="http://127.0.0.1:8080",
            admin_api_key="key",
            database="myapp",
        )
        captured, fake_post = self._mock_execute(drv, conn)
        with patch("requests.post", side_effect=fake_post):
            try:
                conn.execute("SELECT 1")
            except Exception:
                pass
        assert "x-vng-database" in captured, "database-scoped connection must set x-vng-database header"
        assert captured["x-vng-database"] == "myapp"

    def test_c3_case16_no_database_header_absent(self):
        """When no database is set, x-vng-database header must be absent."""
        drv = _driver()
        conn = drv.connect(mode="admin", base_url="http://127.0.0.1:8080", admin_api_key="key")
        captured, fake_post = self._mock_execute(drv, conn)
        with patch("requests.post", side_effect=fake_post):
            try:
                conn.execute("SELECT 1")
            except Exception:
                pass
        assert "x-vng-database" not in captured, "x-vng-database must be absent when no database is configured"


# ---------------------------------------------------------------------------
# C5: Error Propagation (cases 20-23)
# ---------------------------------------------------------------------------

class TestC5ErrorPropagation:
    """C5 — Server error responses must be surfaced to callers."""

    def _conn(self):
        drv = _driver()
        return drv.connect(mode="admin", base_url="http://127.0.0.1:8080", admin_api_key="key")

    def _mock_status(self, status_code, body=None):
        resp = MagicMock()
        resp.status_code = status_code
        resp.json.return_value = body or {"status": "error", "reason": "test"}
        return resp

    def test_c5_case20_401_raises_auth_error(self):
        """A 401 response must raise an auth-flavoured error."""
        conn = self._conn()
        with patch("requests.post", return_value=self._mock_status(401)):
            with pytest.raises(Exception) as exc_info:
                conn.execute("SELECT 1")
        assert "401" in str(exc_info.value) or "auth" in str(exc_info.value).lower()

    def test_c5_case21_403_raises_permission_error(self):
        """A 403 response must raise a permission-flavoured error."""
        conn = self._conn()
        with patch("requests.post", return_value=self._mock_status(403)):
            with pytest.raises(Exception) as exc_info:
                conn.execute("SELECT 1")
        assert "403" in str(exc_info.value) or "permission" in str(exc_info.value).lower() or "forbid" in str(exc_info.value).lower()

    def test_c5_case22_503_raises_server_unavailable(self):
        """A 503 response must raise a server-unavailable-flavoured error."""
        conn = self._conn()
        with patch("requests.post", return_value=self._mock_status(503)):
            with pytest.raises(Exception) as exc_info:
                conn.execute("SELECT 1")
        assert "503" in str(exc_info.value) or "unavailable" in str(exc_info.value).lower()

    def test_c5_case23_malformed_json_raises_protocol_error(self):
        """A malformed JSON response must raise a protocol error."""
        conn = self._conn()
        resp = MagicMock()
        resp.status_code = 200
        resp.json.side_effect = ValueError("not valid json")
        with patch("requests.post", return_value=resp):
            with pytest.raises(Exception):
                conn.execute("SELECT 1")
