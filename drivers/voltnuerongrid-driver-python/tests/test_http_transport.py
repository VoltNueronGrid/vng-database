import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from threading import Thread

from voltnuerongrid_driver_python import (
    DriverConfig,
    DriverError,
    VoltNueronGridDriver,
    is_retryable_http_status,
    perform_driver_http_request,
    validate_config,
)


class TestHttpPolicy(unittest.TestCase):
    def test_is_retryable_matches_rust(self) -> None:
        self.assertTrue(is_retryable_http_status(503))
        self.assertFalse(is_retryable_http_status(404))


class TestValidateConfigTimeouts(unittest.TestCase):
    def test_request_timeout_floor(self) -> None:
        c = DriverConfig(
            base_url="http://127.0.0.1:8080",
            session_id="s",
            mode="admin",
            admin_api_key="k",
            request_timeout_ms=50,
        )
        self.assertEqual(validate_config(c), "request_timeout_ms must be >= 100")

    def test_max_retries_bounds(self) -> None:
        c = DriverConfig(
            base_url="http://127.0.0.1:8080",
            session_id="s",
            mode="admin",
            admin_api_key="k",
            max_retries=25,
        )
        self.assertEqual(validate_config(c), "max_retries must be from 0 to 20")


class TestPerformDriverHttpRequest(unittest.TestCase):
    def test_retries_503_then_200(self) -> None:
        hits = {"n": 0}

        class H(BaseHTTPRequestHandler):
            def log_message(self, *_args: object) -> None:
                return

            def do_GET(self) -> None:
                hits["n"] += 1
                if hits["n"] == 1:
                    self.send_response(503)
                    self.end_headers()
                    return
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b"ok")

        server = HTTPServer(("127.0.0.1", 0), H)
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        port = server.server_port
        try:
            cfg = DriverConfig(
                base_url=f"http://127.0.0.1:{port}",
                session_id="s",
                mode="admin",
                admin_api_key="k",
                request_timeout_ms=5000,
                max_retries=2,
            )
            driver = VoltNueronGridDriver(cfg)
            req = driver.build_health_request()
            status, body = perform_driver_http_request(req, cfg)
            self.assertEqual(status, 200)
            self.assertEqual(body, "ok")
            self.assertEqual(hits["n"], 2)
        finally:
            server.shutdown()
            thread.join(timeout=2)


class TestDriverErrorType(unittest.TestCase):
    def test_driver_error_attrs(self) -> None:
        err = DriverError("timeout", "deadline")
        self.assertEqual(err.kind, "timeout")
        self.assertEqual(str(err), "deadline")
