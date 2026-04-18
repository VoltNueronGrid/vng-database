import unittest
import json
from pathlib import Path

from voltnuerongrid_driver_python import (
    DriverConfig,
    DriverTransportMode,
    TransportCapabilities,
    VoltNueronGridDriver,
    infer_http_base_url_from_vng_url,
    parse_discovery_http_port_str,
    resolve_auto_transport,
    resolve_auto_transport_with_discovery,
    validate_config,
)


class DriverTests(unittest.TestCase):
    @staticmethod
    def _fixtures_dir() -> Path:
        return Path(__file__).resolve().parent.parent.parent / "conformance" / "fixtures"

    def test_validate_config_from_shared_conformance_fixtures(self) -> None:
        fixture_path = self._fixtures_dir() / "config-validation-cases.json"
        data = json.loads(fixture_path.read_text(encoding="utf-8"))

        for case in data["cases"]:
            config = DriverConfig(
                base_url=case["config"]["baseUrl"],
                session_id=case["config"]["sessionId"],
                mode=case["config"]["mode"],
                admin_api_key=case["config"].get("adminApiKey"),
                operator_id=case["config"].get("operatorId"),
                tenant_id=case["config"].get("tenantId"),
            )
            self.assertEqual(validate_config(config), case["expectError"], case["name"])

    def test_build_execute_request_from_shared_fixture(self) -> None:
        fixture_path = self._fixtures_dir() / "request-building-cases.json"
        data = json.loads(fixture_path.read_text(encoding="utf-8"))
        use_case = data["operatorExecuteCase"]

        driver = VoltNueronGridDriver(
            DriverConfig(
                base_url=use_case["config"]["baseUrl"],
                session_id=use_case["config"]["sessionId"],
                mode=use_case["config"]["mode"],
                admin_api_key=use_case["config"]["adminApiKey"],
                operator_id=use_case["config"]["operatorId"],
            )
        )
        request = driver.build_sql_execute_request(use_case["sqlBatch"], max_rows=use_case["maxRows"])
        self.assertEqual(request.method, use_case["expect"]["method"])
        self.assertEqual(request.url, use_case["expect"]["url"])
        self.assertEqual(request.headers["x-vng-admin-key"], use_case["expect"]["headers"]["x-vng-admin-key"])
        self.assertEqual(request.headers["x-vng-operator-id"], use_case["expect"]["headers"]["x-vng-operator-id"])

    def test_transport_mode_fixture_is_consumed_for_dual_transport_gate(self) -> None:
        fixture_path = self._fixtures_dir() / "transport-mode-cases.json"
        data = json.loads(fixture_path.read_text(encoding="utf-8"))

        self.assertEqual(data["defaults"]["fallbackPolicy"], "native_primary_http_fallback")
        self.assertEqual(data["defaults"]["transportAutoOrder"], ["native", "http"])
        self.assertGreaterEqual(len(data["cases"]), 5)

        http_case = next(case for case in data["cases"] if case["id"] == "tm-http-execute-operator")
        self.assertEqual(http_case["transportMode"], "http")
        self.assertEqual(http_case["expect"]["activeTransport"], "http")
        driver = VoltNueronGridDriver(
            DriverConfig(
                base_url=http_case["config"]["baseUrl"],
                session_id=http_case["config"]["sessionId"],
                mode=http_case["config"]["mode"],
                admin_api_key=http_case["config"]["adminApiKey"],
                operator_id=http_case["config"]["operatorId"],
            )
        )
        request = driver.build_sql_execute_request("SELECT 1;", max_rows=100)
        self.assertEqual(request.method, "POST")
        self.assertIn("/api/v1/sql/execute", request.url)

        auto_fallback_case = next(case for case in data["cases"] if case["id"] == "tm-auto-fallback-http")
        self.assertEqual(auto_fallback_case["transportMode"], "auto")
        self.assertFalse(auto_fallback_case["runtimeCapabilities"]["nativeAvailable"])
        self.assertTrue(auto_fallback_case["runtimeCapabilities"]["httpAvailable"])
        self.assertEqual(auto_fallback_case["expect"]["activeTransport"], "http")
        self.assertTrue(auto_fallback_case["expect"]["fallbackTriggered"])

        no_transport_case = next(case for case in data["cases"] if case["id"] == "tm-auto-no-transports")
        self.assertEqual(no_transport_case["expectError"]["kind"], "transport")

    def test_resolve_transport_mode_auto_uses_base_url_scheme(self) -> None:
        d1 = VoltNueronGridDriver(
            DriverConfig(
                base_url="vng://127.0.0.1:7542",
                session_id="s",
                mode="admin",
                admin_api_key="k",
            )
        )
        r1 = d1.resolve_transport_mode(DriverTransportMode.AUTO)
        self.assertEqual(r1.active, DriverTransportMode.NATIVE)
        self.assertTrue(r1.used_auto_resolution)

        d2 = VoltNueronGridDriver(
            DriverConfig(
                base_url="http://127.0.0.1:8080",
                session_id="s",
                mode="admin",
                admin_api_key="k",
            )
        )
        r2 = d2.resolve_transport_mode(DriverTransportMode.AUTO)
        self.assertEqual(r2.active, DriverTransportMode.HTTP)

    def test_resolve_auto_dual_endpoint_matches_fixture_semantics(self) -> None:
        dual = DriverConfig(
            base_url="vng://127.0.0.1:7542",
            session_id="s",
            mode="admin",
            admin_api_key="secret",
            http_fallback_url="http://127.0.0.1:8080",
        )
        a = resolve_auto_transport(
            dual, TransportCapabilities(native_available=True, http_available=True)
        )
        self.assertEqual(a.active, DriverTransportMode.NATIVE)
        self.assertFalse(a.fallback_triggered)
        b = resolve_auto_transport(
            dual, TransportCapabilities(native_available=False, http_available=True)
        )
        self.assertEqual(b.active, DriverTransportMode.HTTP)
        self.assertTrue(b.fallback_triggered)
        self.assertEqual(b.fallback_reason, "native_unavailable")
        with self.assertRaises(ValueError) as ctx:
            resolve_auto_transport(
                dual, TransportCapabilities(native_available=False, http_available=False)
            )
        self.assertIn("no available transport", str(ctx.exception))

    def test_parse_discovery_http_port_str(self) -> None:
        self.assertEqual(parse_discovery_http_port_str("8080"), 8080)
        self.assertIsNone(parse_discovery_http_port_str("0"))
        self.assertIsNone(parse_discovery_http_port_str(""))

    def test_infer_http_and_resolve_auto_with_discovery(self) -> None:
        self.assertEqual(
            infer_http_base_url_from_vng_url("vng://127.0.0.1:7542", 8080),
            "http://127.0.0.1:8080",
        )
        cfg = DriverConfig(
            base_url="vng://127.0.0.1:7542",
            session_id="s",
            mode="admin",
            admin_api_key="k",
        )
        r = resolve_auto_transport_with_discovery(
            cfg,
            TransportCapabilities(native_available=True, http_available=True),
            8080,
        )
        self.assertEqual(r.active, DriverTransportMode.NATIVE)
        self.assertIn("dual-endpoint", r.notes or "")


if __name__ == "__main__":
    unittest.main()

