import unittest
import json
from pathlib import Path

from voltnuerongrid_driver_python import (
    DriverConfig,
    DriverTransportMode,
    TransportCapabilities,
    VoltNueronGridDriver,
    encode_framed_json,
    infer_http_base_url_from_vng_url,
    native_command_roundtrip,
    native_schema_registry_command_roundtrip,
    native_sql_execute_command_roundtrip,
    parse_discovery_http_port_str,
    resolve_auto_transport,
    resolve_auto_transport_with_discovery,
    validate_config,
)
from voltnuerongrid_driver_python.native_wire import encode_framed_json as _encode_framed_json


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


class NativeCommandFrameTests(unittest.TestCase):
    """Unit tests for native command frame construction logic — no live socket required."""

    def _decode_framed(self, data: bytes) -> dict:
        import struct

        (length,) = struct.unpack(">I", data[:4])
        return json.loads(data[4 : 4 + length].decode("utf-8"))

    def test_native_command_roundtrip_builds_correct_frame_type(self) -> None:
        """Verify that encode_framed_json produces a Command frame with correct frame_type."""
        frame = {
            "frame_type": "Command",
            "protocol_version": "v1",
            "request_id": "py-native-cmd",
            "session_id": "test-session",
            "payload": {"command": "health", "body": {}},
        }
        encoded = encode_framed_json(frame)
        decoded = self._decode_framed(encoded)
        self.assertEqual(decoded["frame_type"], "Command")
        self.assertEqual(decoded["protocol_version"], "v1")
        self.assertEqual(decoded["payload"]["command"], "health")

    def test_native_sql_execute_command_frame(self) -> None:
        """Verify sql.execute command frame has correct command name and body fields."""
        sql_batch = "SELECT 1;"
        max_rows = 100
        body: dict = {"sql_batch": sql_batch, "max_rows": max_rows}
        frame = {
            "frame_type": "Command",
            "protocol_version": "v1",
            "request_id": "py-native-sql-execute-cmd",
            "session_id": "test-session",
            "payload": {"command": "sql.execute", "body": body},
        }
        encoded = encode_framed_json(frame)
        decoded = self._decode_framed(encoded)
        self.assertEqual(decoded["frame_type"], "Command")
        self.assertEqual(decoded["payload"]["command"], "sql.execute")
        self.assertEqual(decoded["payload"]["body"]["sql_batch"], "SELECT 1;")
        self.assertEqual(decoded["payload"]["body"]["max_rows"], 100)

    def test_native_sql_execute_command_frame_without_max_rows(self) -> None:
        """Verify sql.execute body omits max_rows when not provided."""
        body: dict = {"sql_batch": "SELECT 2;"}
        frame = {
            "frame_type": "Command",
            "protocol_version": "v1",
            "request_id": "py-native-sql-execute-cmd",
            "session_id": "test-session",
            "payload": {"command": "sql.execute", "body": body},
        }
        encoded = encode_framed_json(frame)
        decoded = self._decode_framed(encoded)
        self.assertNotIn("max_rows", decoded["payload"]["body"])

    def test_native_schema_registry_command_name(self) -> None:
        """Verify ingest.schema.registry command frame has correct command name."""
        frame = {
            "frame_type": "Command",
            "protocol_version": "v1",
            "request_id": "py-native-schema-registry-cmd",
            "session_id": "test-session",
            "payload": {"command": "ingest.schema.registry", "body": {}},
        }
        encoded = encode_framed_json(frame)
        decoded = self._decode_framed(encoded)
        self.assertEqual(decoded["payload"]["command"], "ingest.schema.registry")
        self.assertEqual(decoded["frame_type"], "Command")

    def test_native_command_functions_are_importable(self) -> None:
        """Smoke-test that all new S2 command parity functions are importable from the package."""
        from voltnuerongrid_driver_python import (
            native_command_roundtrip,
            native_health_command_roundtrip,
            native_schema_registry_command_roundtrip,
            native_sql_analyze_command_roundtrip,
            native_sql_execute_command_roundtrip,
            native_sql_route_command_roundtrip,
            native_sql_transaction_command_roundtrip,
        )

        self.assertTrue(callable(native_command_roundtrip))
        self.assertTrue(callable(native_health_command_roundtrip))
        self.assertTrue(callable(native_sql_execute_command_roundtrip))
        self.assertTrue(callable(native_sql_analyze_command_roundtrip))
        self.assertTrue(callable(native_sql_route_command_roundtrip))
        self.assertTrue(callable(native_sql_transaction_command_roundtrip))
        self.assertTrue(callable(native_schema_registry_command_roundtrip))

    def test_hello_frame_structure(self) -> None:
        """Verify Hello frame structure used in session handshake."""
        frame = {
            "frame_type": "Hello",
            "protocol_version": "v1",
            "request_id": "py-native-hello",
            "session_id": None,
            "payload": {
                "session_id": "test-session",
                "protocol": "vng-native",
                "version": "v1",
            },
        }
        encoded = encode_framed_json(frame)
        decoded = self._decode_framed(encoded)
        self.assertEqual(decoded["frame_type"], "Hello")
        self.assertIsNone(decoded["session_id"])
        self.assertEqual(decoded["payload"]["protocol"], "vng-native")


if __name__ == "__main__":
    unittest.main()

