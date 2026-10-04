"""Standard-library fixtures shared by the isolated acceptance runners."""

import contextlib
import json
import os
import secrets
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLIENT = ROOT / "tests/client"
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


class CheckFailure(RuntimeError):
    pass


def request(url, token=None, method="GET", body=None, expected=200, timeout=8):
    headers = {}
    data = None
    if token:
        headers["Authorization"] = "Bearer " + token
    if method == "PUT" and url.endswith("/acl") and "/api/groups/" in url:
        base = url.split("/api/groups/", 1)[0]
        revision = request(base + "/api/config", token)["revision"]
        headers["If-Match"] = f'"{revision}"'
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with HTTP.open(req, timeout=timeout) as response:
            status, content = response.status, response.read()
    except urllib.error.HTTPError as error:
        status, content = error.code, error.read()
    if status != expected:
        raise CheckFailure(
            f"{method} {url.rsplit('/', 1)[-1]} returned HTTP {status}, expected {expected}"
        )
    if not content:
        return None
    try:
        return json.loads(content)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return content.decode("utf-8", errors="replace")


def control(base, token, path, payload=None):
    return request(
        base + path, token, "POST" if payload is not None else "GET", payload
    )


def control_post(base, token, path, payload):
    return request(base + path, token, "POST", payload)


def check(condition, message):
    if not condition:
        raise CheckFailure(message)


def wait_ready(process, label, url=None, address_file=None):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(
                f"{label} exited during startup (status {process.returncode})"
            )
        if address_file is not None and address_file.exists():
            address = address_file.read_text().strip()
            if address:
                url = f"http://{address}/health"
        if url is not None:
            try:
                with HTTP.open(url, timeout=1) as response:
                    if response.status == 200 and json.load(response).get("ok"):
                        return url.rsplit("/", 1)[0]
            except (OSError, urllib.error.URLError, ValueError):
                pass
        time.sleep(0.1)
    raise RuntimeError(f"Timed out waiting for {label}")


@contextlib.contextmanager
def reserve_hub_port():
    # The Hub needs the same numeric port free for both TCP and UDP.
    for _ in range(100):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as tcp:
            tcp.bind(("127.0.0.1", 0))
            port = tcp.getsockname()[1]
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp:
                try:
                    udp.bind(("0.0.0.0", port))
                except OSError:
                    continue
                yield port
                return
    raise RuntimeError("Could not allocate a TCP/UDP port for the Hub")


def stop_processes(processes):
    for process in reversed(processes):
        if process.poll() is None:
            process.terminate()
    deadline = time.monotonic() + 5
    for process in reversed(processes):
        try:
            process.wait(timeout=max(0.01, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


class Hub:
    def __init__(self, binary, directory):
        directory.mkdir(mode=0o700, exist_ok=True)
        self.directory = directory
        self.token = secrets.token_urlsafe(32)
        self.environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith("WIREHUB_")
        }
        with reserve_hub_port() as port:
            self.environment.update(
                WIREHUB_PORT=str(port),
                WIREHUB_ADMIN_TOKEN=self.token,
                WIREHUB_HTTP_BIND="127.0.0.1",
                WIREHUB_DB=str(directory / "wirehub.sqlite3"),
                WIREHUB_HUB_KEY=str(directory / "wirehub.key"),
            )
        self.url = f"http://127.0.0.1:{port}"
        self.log = (directory / "process.log").open("wb")
        try:
            self.process = subprocess.Popen(
                [str(binary)],
                cwd=directory,
                env=self.environment,
                stdin=subprocess.DEVNULL,
                stdout=self.log,
                stderr=self.log,
            )
        except BaseException:
            self.log.close()
            raise
        try:
            wait_ready(self.process, "Hub", url=self.url + "/api/health")
        except BaseException:
            self.stop(False)
            raise

    def stop(self, graceful=True):
        started = time.monotonic()
        if self.process.poll() is None:
            self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
            raise RuntimeError("SIGTERM exceeded the bounded shutdown deadline")
        finally:
            self.log.close()
        if graceful:
            check(
                self.process.returncode == 0,
                "SIGTERM did not drain and exit successfully",
            )
        return time.monotonic() - started


@dataclass(frozen=True)
class TestClient:
    url: str
    token: str
    process: subprocess.Popen


def build_client(directory):
    binary = directory / "client"
    subprocess.run(
        ["go", "build", "-mod=readonly", "-trimpath", "-o", str(binary), "."],
        cwd=CLIENT,
        check=True,
    )
    return binary


def launch_client(binary, directory, label, resources):
    token = secrets.token_urlsafe(32)
    address = directory / f"client-{label}.address"
    log = resources.enter_context((directory / f"client-{label}.log").open("wb"))
    process = subprocess.Popen(
        [str(binary), "-listen", "127.0.0.1:0", "-ready-file", str(address)],
        env=dict(os.environ, TEST_CONTROL_TOKEN=token),
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=log,
    )
    resources.callback(stop_processes, [process])
    url = wait_ready(process, f"Client {label}", address_file=address)
    return TestClient(url, token, process)


def write_report(output, report):
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


def run_cli(operation, failure_message):
    """Keep temporary state private and unwind resource contexts on interruption."""
    os.umask(0o077)
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    try:
        operation()
    except KeyboardInterrupt:
        raise SystemExit(130) from None
    except Exception as error:
        # Only intentional check failures may carry detail. External process and
        # parser errors can include temporary credentials/configuration.
        detail = str(error) if isinstance(error, RuntimeError) else failure_message
        print(f"FAIL: {detail}", file=sys.stderr, flush=True)
        raise SystemExit(1) from None
