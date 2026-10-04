#!/usr/bin/env python3
"""Prepare a temporary macOS WireGuard profile, then verify native tunnel traffic.

Import/activation/removal is deliberately left to the local WireGuard UI operator.
The profile routes only 172.27.93.0/24; never modify an existing tunnel.
"""

import argparse
import contextlib
import hashlib
import os
import platform
import secrets
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path

from support import (
    ROOT,
    Hub,
    build_client,
    check,
    launch_client,
    request,
    run_cli,
    write_report,
)


def native_echo(target, protocol, payload):
    with socket.socket(
        socket.AF_INET, socket.SOCK_STREAM if protocol == "tcp" else socket.SOCK_DGRAM
    ) as connection:
        connection.settimeout(3)
        connection.bind(("172.27.93.2", 0))
        connection.connect(target)
        connection.sendall(payload)
        reply = b""
        while len(reply) < len(payload):
            chunk = connection.recv(len(payload) - len(reply))
            if not chunk:
                break
            reply += chunk
        check(reply == payload, "Desktop echo integrity differs")


def run(binary, wait_seconds, output):
    check(
        platform.system() == "Darwin", "This harness targets the installed macOS client"
    )
    os.umask(0o077)
    with (
        tempfile.TemporaryDirectory(prefix="wirehub-desktop-") as temporary,
        contextlib.ExitStack() as resources,
    ):
        directory = Path(temporary)
        client_binary = build_client(directory)
        hub = Hub(binary.resolve(), directory / "hub")
        resources.callback(hub.stop, False)
        backend = launch_client(client_binary, directory, "backend", resources)
        url, token = backend.url, backend.token
        request(
            hub.url + "/api/setup",
            hub.token,
            "POST",
            {
                "subnet": "172.27.93.0/24",
                "endpoint": hub.url.removeprefix("http://"),
                "persistent_keepalive": 5,
            },
        )
        groups = [
            request(
                hub.url + "/api/groups", hub.token, "POST", {"name": name}, expected=201
            )
            for name in ("Desktop", "Backend")
        ]
        ga, gb = [group["id"] for group in groups]

        def policy(allowed):
            revision = request(hub.url + "/api/config", hub.token)["revision"]
            return request(
                hub.url + "/api/policy",
                hub.token,
                "PUT",
                {
                    "expected_revision": revision,
                    "changes": [
                        {"group_id": ga, "allowed_groups": allowed},
                        {"group_id": gb, "allowed_groups": [ga]},
                    ],
                },
            )

        policy([gb])
        provisions = [
            request(
                hub.url + "/api/peers",
                hub.token,
                "POST",
                {"name": name, "group_id": group},
                expected=201,
            )
            for name, group in zip(("Desktop", "Backend"), (ga, gb))
        ]
        check(
            provisions[0]["peer"]["ipv4"] == "172.27.93.2", "Unexpected desktop address"
        )
        profile = directory / "wirehub-rc-desktop.conf"
        profile.write_text(provisions[0]["config"])
        profile.chmod(0o600)
        request(url + "/configure", token, "POST", {"config": provisions[1]["config"]})
        request(url + "/serve", token, "POST", {"port": 18080})
        for protocol in ("tcp", "udp"):
            request(
                hub.url + "/api/forwards",
                hub.token,
                "POST",
                {
                    "name": protocol,
                    "protocol": protocol,
                    "target_port": 18080,
                    "target_peer_id": provisions[1]["peer"]["id"],
                    "allowed_group_ids": [ga],
                },
                expected=201,
            )
        print("READY: import and activate " + str(profile), flush=True)
        print(
            "Only 172.27.93.0/24 is routed. Deactivate and remove this temporary profile after verification.",
            flush=True,
        )
        started = time.monotonic()
        deadline = started + wait_seconds
        while time.monotonic() < deadline:
            peers = request(hub.url + "/api/peers", hub.token)
            desktop = next(
                peer for peer in peers if peer["id"] == provisions[0]["peer"]["id"]
            )
            if desktop["last_handshake_unix"]:
                try:
                    native_echo(("172.27.93.3", 18080), "tcp", b"desktop-ready")
                    break
                except OSError:
                    pass
            time.sleep(2)
        else:
            raise RuntimeError(
                "Desktop profile was not activated before timeout; interoperability is unverified"
            )
        checks = []
        for target in ("172.27.93.3", "172.27.93.1"):
            for protocol in ("tcp", "udp"):
                native_echo((target, 18080), protocol, secrets.token_bytes(1200))
                checks.append(
                    "native "
                    + protocol
                    + " "
                    + ("forwarded" if target.endswith(".1") else "direct")
                    + " with 1200-byte payload"
                )
        native_echo(("172.27.93.1", 18080), "udp", secrets.token_bytes(1392))
        checks.append("1420-byte inner IPv4 UDP packet")
        ping = subprocess.run(
            ["/sbin/ping", "-S", "172.27.93.2", "-c", "3", "-W", "2000", "172.27.93.3"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=15,
        )
        check(ping.returncode == 0, "Native ICMP failed")
        checks.append("native ICMP with bidirectional ACL")
        # The socket remains open through unrelated reload, bulk transfer and revoke.
        with socket.socket() as connection:
            connection.settimeout(15)
            connection.bind(("172.27.93.2", 0))
            connection.connect(("172.27.93.1", 18080))
            group = request(
                hub.url + "/api/groups",
                hub.token,
                "POST",
                {"name": "unrelated"},
                expected=201,
            )
            request(
                hub.url + "/api/groups/" + group["id"],
                hub.token,
                "DELETE",
                expected=204,
            )
            payload = bytes(range(256)) * (4 * 1024 * 1024 // 256)
            errors = []

            def write():
                try:
                    connection.sendall(payload)
                except OSError:
                    errors.append(True)

            writer = threading.Thread(target=write)
            writer.start()
            digest = hashlib.sha256()
            received = 0
            try:
                while received < len(payload):
                    chunk = connection.recv(min(65536, len(payload) - received))
                    check(bool(chunk), "Bulk transfer truncated")
                    digest.update(chunk)
                    received += len(chunk)
            finally:
                if received < len(payload):
                    connection.shutdown(socket.SHUT_RDWR)
                writer.join(timeout=20)
            check(
                not writer.is_alive()
                and not errors
                and digest.digest() == hashlib.sha256(payload).digest(),
                "Desktop bulk integrity differs",
            )
            checks.append("same-socket unrelated reload and 4 MiB SHA256 integrity")
            policy([])
            connection.settimeout(2)
            try:
                connection.sendall(b"revoked")
                reply = connection.recv(7)
            except OSError:
                reply = b""
            check(not reply, "Desktop TCP survived acknowledged revoke")
            checks.append(
                "same-socket acknowledged revocation without reconfiguring tunnel"
            )
        for protocol in ("tcp", "udp"):
            try:
                native_echo(("172.27.93.1", 18080), protocol, b"denied")
            except OSError:
                continue
            raise RuntimeError("New desktop flow survived revoke")
        checks.append("new forwarded TCP and UDP denied after revoke")
        version = Path("/Applications/WireGuard.app/Contents/Info.plist")
        import plistlib

        client_version = plistlib.loads(version.read_bytes())[
            "CFBundleShortVersionString"
        ]
        report = {
            "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "desktop_client": "WireGuard " + client_version,
            "platform": platform.platform(),
            "binary": subprocess.check_output(
                [str(binary.resolve()), "--version"], text=True
            ).strip(),
            "checks": checks,
            "all_checks_passed": True,
        }
        write_report(output, report)
        print(
            "PASS: desktop interoperability; deactivate and remove wirehub-rc-desktop. Report "
            + str(output),
            flush=True,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/wirehub")
    parser.add_argument("--wait-seconds", type=int, default=3600)
    parser.add_argument(
        "--output", type=Path, default=Path("/tmp/wirehub-desktop.json")
    )
    args = parser.parse_args()
    run(args.binary, args.wait_seconds, args.output)


if __name__ == "__main__":
    run_cli(main, "Desktop verification failed; temporary credentials withheld")
