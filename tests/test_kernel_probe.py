"""Exercise TCP probe deadlines and failure evidence with real loopback sockets."""

import contextlib
import importlib.util
import socket
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "kernel_client", Path(__file__).parent / "linux-kernel" / "client.py"
)
client = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(client)


@contextlib.contextmanager
def server(handler):
    stopped = threading.Event()
    errors = []
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        listener.settimeout(10)

        def run():
            try:
                with listener.accept()[0] as connection:
                    connection.settimeout(10)
                    handler(connection, stopped)
            except OSError as error:
                if not stopped.is_set():
                    errors.append(error)

        worker = threading.Thread(target=run, daemon=True)
        worker.start()
        try:
            yield listener.getsockname()
        finally:
            stopped.set()
            worker.join(timeout=10)
            if worker.is_alive():
                raise AssertionError("Loopback fixture did not stop")
            if errors:
                raise AssertionError("Loopback fixture failed") from errors[0]


def read_exact(connection, length):
    received = bytearray()
    while len(received) < length:
        part = connection.recv(length - len(received))
        if not part:
            raise OSError("Fixture received premature EOF")
        received.extend(part)
    return bytes(received)


class KernelProbeTests(unittest.TestCase):
    def test_bulk_full_duplex_integrity(self):
        payload = bytes(range(256)) * (4 * 1024 * 1024 // 256)

        def echo(connection, _):
            remaining = len(payload)
            while remaining:
                part = connection.recv(min(65536, remaining))
                if not part:
                    raise OSError("Fixture received premature EOF")
                connection.sendall(part)
                remaining -= len(part)

        with server(echo) as address:
            result = client.tcp_probe(*address, payload, client.TCP_BULK_TIMEOUT)
        self.assertTrue(result["ok"], result)
        self.assertEqual(result["bytes"], len(payload))
        self.assertEqual(result["sent_bytes"], len(payload))

    def test_bulk_tolerates_pause_longer_than_small_probe_budget(self):
        payload = b"delayed echo"

        def delayed(connection, stopped):
            received = read_exact(connection, len(payload))
            if not stopped.wait(client.TCP_PROBE_TIMEOUT + 0.2):
                connection.sendall(received)

        with server(delayed) as address:
            result = client.tcp_probe(*address, payload, client.TCP_BULK_TIMEOUT)
        self.assertTrue(result["ok"], result)
        self.assertGreater(result["seconds"], client.TCP_PROBE_TIMEOUT)

    def test_trickling_progress_does_not_extend_total_deadline(self):
        payload = b"x" * 64

        def trickle(connection, stopped):
            received = read_exact(connection, len(payload))
            for byte in received:
                try:
                    connection.sendall(bytes([byte]))
                except (BrokenPipeError, ConnectionResetError):
                    return
                if stopped.wait(0.04):
                    return

        with server(trickle) as address:
            result = client.tcp_probe(*address, payload, 0.2)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "timeout")
        self.assertEqual(result["phase"], "transfer")
        self.assertLess(result["bytes"], len(payload))

    def test_corruption_fails_without_exposing_payload(self):
        payload = b"private-test-payload"

        def corrupt(connection, _):
            received = read_exact(connection, len(payload))
            connection.sendall(received[:-1] + b"!")

        with server(corrupt) as address:
            result = client.tcp_probe(*address, payload, 2)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "integrity_mismatch")
        self.assertEqual(result["expected_bytes"], len(payload))
        self.assertNotIn(payload.decode(), str(result))

    def test_early_eof_reports_partial_delivery(self):
        payload = b"truncated echo"

        def truncate(connection, _):
            received = read_exact(connection, len(payload))
            connection.sendall(received[:3])
            connection.shutdown(socket.SHUT_WR)

        with server(truncate) as address:
            result = client.tcp_probe(*address, payload, 2)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "premature_eof")
        self.assertEqual(result["bytes"], 3)

    def test_refusal_is_distinct_from_transfer_failure(self):
        # Bound-but-not-listening ports time out on macOS and refuse on Linux.
        # Inject refusal so this error-classification check is platform independent.
        with patch.object(
            client.socket, "create_connection", side_effect=ConnectionRefusedError
        ):
            result = client.tcp_probe("127.0.0.1", 1, b"echo", 2)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "connection_error")
        self.assertEqual(result["phase"], "connect")
        self.assertEqual(result["bytes"], 0)


if __name__ == "__main__":
    unittest.main()
