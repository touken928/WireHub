#!/usr/bin/env python3
"""Real Linux-kernel WireGuard interop in disposable Docker network/containers."""

import argparse
import hashlib
import json
import secrets
import subprocess
import time
from pathlib import Path

from support import ROOT, check, request, run_cli, write_report


def docker(*args, input=None):
    result = subprocess.run(
        ["docker", *args],
        input=input,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode:
        # Arguments/configuration may contain private temporary credentials.
        raise RuntimeError(f"Docker {args[0]} failed; test resources are being removed")
    return result.stdout.strip()


def helper(name, action, body=None):
    return json.loads(
        docker(
            "exec",
            "-i",
            name,
            "python3",
            "/client.py",
            action,
            input=json.dumps(body) if body is not None else None,
        )
    )


def run(output, build):
    if build:
        docker(
            "build", "-t", "wirehub:interop-hub", "-f", "docker/Dockerfile", str(ROOT)
        )
        docker(
            "build", "-t", "wirehub:interop-client", str(ROOT / "tests/linux-kernel")
        )
    suffix = secrets.token_hex(6)
    network = "wirehub-interop-" + suffix
    hub = "wirehub-hub-" + suffix
    containers = []
    token = secrets.token_urlsafe(32)
    docker("network", "create", network)
    started = time.monotonic()
    try:
        docker(
            "run",
            "-d",
            "--rm",
            "--name",
            hub,
            "--network",
            network,
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            "--tmpfs",
            "/data:rw,mode=0700,uid=65532,gid=65532",
            "-p",
            "127.0.0.1::51820/tcp",
            "-e",
            "WIREHUB_ADMIN_TOKEN=" + token,
            "-e",
            "WIREHUB_TRUSTED_PROXY_MODE=1",
            "wirehub:interop-hub",
        )
        containers.append(hub)
        url = "http://" + docker("port", hub, "51820/tcp")
        for _ in range(100):
            try:
                if request(url + "/api/ready")["ok"]:
                    break
            except OSError:
                pass
            time.sleep(0.1)
        else:
            raise RuntimeError("Kernel interop hub startup timed out")
        request(
            url + "/api/setup",
            token,
            "POST",
            {
                "subnet": "172.27.91.0/24",
                "endpoint": hub + ":51820",
                "persistent_keepalive": 5,
            },
        )
        groups = [
            request(url + "/api/groups", token, "POST", {"name": label}, expected=201)
            for label in ("kernel-A", "kernel-B")
        ]
        ga, gb = (group["id"] for group in groups)
        revision = request(url + "/api/config", token)["revision"]
        request(
            url + "/api/policy",
            token,
            "PUT",
            {
                "expected_revision": revision,
                "changes": [{"group_id": ga, "allowed_groups": [gb]}],
            },
        )
        provisions = [
            request(
                url + "/api/peers",
                token,
                "POST",
                {"name": label, "group_id": group["id"]},
                expected=201,
            )
            for label, group in zip(("kernel-A", "kernel-B"), groups)
        ]
        clients = []
        for index, provision in enumerate(provisions):
            name = "wirehub-client-" + str(index) + "-" + suffix
            docker(
                "run",
                "-d",
                "--rm",
                "--name",
                name,
                "--network",
                network,
                "--cap-drop",
                "ALL",
                "--cap-add",
                "NET_ADMIN",
                "--cap-add",
                "NET_RAW",
                "--security-opt",
                "no-new-privileges",
                "wirehub:interop-client",
            )
            containers.append(name)
            clients.append(name)
            config = provision["config"]
            response = json.loads(
                docker(
                    "exec",
                    "-i",
                    name,
                    "python3",
                    "/client.py",
                    "configure",
                    input=config,
                )
            )
            check(response["ok"], "Kernel client setup failed")
        a, b = clients
        ip_a, ip_b = (provision["peer"]["ipv4"] for provision in provisions)
        for name, ip in zip(clients, (ip_a, ip_b)):
            docker("exec", "-d", name, "python3", "/client.py", "serve", ip, "18080")
        for protocol in ("tcp", "udp"):
            request(
                url + "/api/forwards",
                token,
                "POST",
                {
                    "name": "kernel-" + protocol,
                    "protocol": protocol,
                    "target_port": 18080,
                    "target_peer_id": provisions[1]["peer"]["id"],
                    "allowed_group_ids": [ga],
                },
                expected=201,
            )
        time.sleep(0.2)
        measurements = []
        for target in (ip_b, "172.27.91.1"):
            for protocol in ("tcp", "udp"):
                result = helper(
                    a,
                    "probe",
                    {
                        "protocol": protocol,
                        "target": target,
                        "payload": "kernel-" + protocol + "-" + secrets.token_hex(8),
                    },
                )
                check(result["ok"], f"Kernel {protocol} route failed")
                measurements.append(
                    {
                        "protocol": protocol,
                        "mode": "direct" if target == ip_b else "forward",
                        **result,
                    }
                )
        check(
            not helper(a, "probe", {"protocol": "icmp", "target": ip_b})["ok"],
            "Stateless ICMP reply unexpectedly bypassed reverse ACL",
        )
        for protocol in ("tcp", "udp", "icmp"):
            check(
                not helper(b, "probe", {"protocol": protocol, "target": ip_a})["ok"],
                "Kernel reverse ACL denied path succeeded",
            )
        revision = request(url + "/api/config", token)["revision"]
        request(
            url + "/api/policy",
            token,
            "PUT",
            {
                "expected_revision": revision,
                "changes": [{"group_id": gb, "allowed_groups": [ga]}],
            },
        )
        check(
            helper(a, "probe", {"protocol": "icmp", "target": ip_b})["ok"],
            "Kernel direct ICMP failed with both directions permitted",
        )
        check(
            helper(b, "probe", {"protocol": "icmp", "target": ip_a})["ok"],
            "Kernel reverse ICMP failed after grant",
        )
        revision = request(url + "/api/config", token)["revision"]
        request(
            url + "/api/policy",
            token,
            "PUT",
            {
                "expected_revision": revision,
                "changes": [{"group_id": gb, "allowed_groups": []}],
            },
        )
        for size in (1200, 1392):
            check(
                helper(
                    a,
                    "probe",
                    {"protocol": "udp", "target": "172.27.91.1", "payload": "x" * size},
                )["ok"],
                "Kernel MTU-safe UDP failed",
            )
        check(
            not helper(
                a,
                "probe",
                {
                    "protocol": "udp",
                    "target": "172.27.91.1",
                    "payload": "fragmented-" + "x" * 1400,
                },
            )["ok"],
            "Fragmented UDP unexpectedly traversed router",
        )
        transfer = helper(
            a, "probe", {"protocol": "tcp", "target": "172.27.91.1", "bytes": 4 << 20}
        )
        check(
            transfer["ok"] and transfer["bytes"] == 4 << 20,
            "Kernel 4 MiB TCP integrity failed",
        )
        measurements.append({"protocol": "tcp", "mode": "forward-bulk", **transfer})
        # Revoke while the same kernel interfaces, keys and listeners remain up.
        revision = request(url + "/api/config", token)["revision"]
        result = request(
            url + "/api/policy",
            token,
            "PUT",
            {
                "expected_revision": revision,
                "changes": [{"group_id": ga, "allowed_groups": []}],
            },
        )
        check(
            result["applied_revision"] >= result["revision"],
            "Kernel policy revocation not activated",
        )
        denied = "revoked-kernel-" + secrets.token_hex(16)
        for protocol in ("tcp", "udp"):
            check(
                not helper(
                    a,
                    "probe",
                    {"protocol": protocol, "target": "172.27.91.1", "payload": denied},
                )["ok"],
                "Kernel forward survived revocation",
            )
        check(
            not helper(a, "probe", {"protocol": "icmp", "target": ip_b})["ok"],
            "Kernel ICMP survived revocation",
        )
        status = [helper(name, "status") for name in clients]
        check(
            all(item["latest_handshake"] > 0 for item in status),
            "Real kernel handshake evidence missing",
        )
        denied_hash = hashlib.sha256(denied.encode()).hexdigest()
        check(
            all(item["sha256"] != denied_hash for item in status[1]["arrivals"]),
            "Kernel backend received revoked payload",
        )
        report = {
            "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "client": "Linux kernel WireGuard",
            "hub_binary": docker("run", "--rm", "wirehub:interop-hub", "--version"),
            "hub_image_id": docker(
                "image", "inspect", "--format", "{{.Id}}", "wirehub:interop-hub"
            ),
            "kernel": status[0]["kernel"],
            "wireguard_tools": status[0]["wireguard_tools"],
            "checks": [
                "direct TCP/UDP/ICMP",
                "forward TCP/UDP",
                "directional denial",
                "1420 MTU boundaries",
                "fragmented UDP rejection",
                "4 MiB TCP integrity",
                "same-interface live revocation",
                "no revoked backend arrival",
            ],
            "measurements": measurements,
            "elapsed_seconds": time.monotonic() - started,
        }
        write_report(output, report)
        print("PASS: real Linux kernel WireGuard interop; report " + str(output))
    finally:
        for name in reversed(containers):
            subprocess.run(
                ["docker", "rm", "-f", name],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        subprocess.run(
            ["docker", "network", "rm", network],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output", type=Path, default=Path("/tmp/wirehub-linux-kernel.json")
    )
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    run(args.output, not args.skip_build)


if __name__ == "__main__":
    run_cli(main, "Kernel interop check failed; temporary credentials withheld")
