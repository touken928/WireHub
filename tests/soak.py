#!/usr/bin/env python3
"""Isolated encrypted RC endurance run; no production state or interfaces used."""

import argparse
import contextlib
import hashlib
import os
import platform
import secrets
import subprocess
import tempfile
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


def run(binary, duration, output):
    check(
        duration >= 300,
        "Endurance runs must span at least 300 seconds for automatic rekey",
    )
    os.umask(0o077)
    binary = binary.resolve()
    with (
        tempfile.TemporaryDirectory(prefix="wirehub-soak-") as temporary,
        contextlib.ExitStack() as resources,
    ):
        directory = Path(temporary)
        client_binary = build_client(directory)
        hub = Hub(binary, directory / "hub")
        resources.callback(hub.stop, False)
        fixtures = [
            launch_client(client_binary, directory, label, resources) for label in "AB"
        ]
        clients = [client.url for client in fixtures]
        tokens = [client.token for client in fixtures]
        processes = [hub.process, *(client.process for client in fixtures)]
        request(
            hub.url + "/api/setup",
            hub.token,
            "POST",
            {
                "subnet": "172.27.92.0/24",
                "endpoint": hub.url.removeprefix("http://"),
                "persistent_keepalive": 5,
            },
        )
        groups = [
            request(
                hub.url + "/api/groups",
                hub.token,
                "POST",
                {"name": label},
                expected=201,
            )
            for label in "AB"
        ]
        ga, gb = [group["id"] for group in groups]

        def policy(allowed):
            revision = request(hub.url + "/api/config", hub.token)["revision"]
            result = request(
                hub.url + "/api/policy",
                hub.token,
                "PUT",
                {
                    "expected_revision": revision,
                    "changes": [{"group_id": ga, "allowed_groups": allowed}],
                },
            )
            check(
                result["applied_revision"] >= result["revision"],
                "Policy acknowledgement lagged",
            )
            return revision

        policy([gb])
        provisions = [
            request(
                hub.url + "/api/peers",
                hub.token,
                "POST",
                {"name": label, "group_id": group},
                expected=201,
            )
            for label, group in zip("AB", (ga, gb))
        ]
        for client, token, provision in zip(clients, tokens, provisions):
            request(
                client + "/configure", token, "POST", {"config": provision["config"]}
            )
        request(clients[1] + "/serve", tokens[1], "POST", {"port": 18080})
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

        def call(path, body):
            return request(clients[0] + path, tokens[0], "POST", body, timeout=65)

        def opened(target):
            result = call("/tcp/open", {"target": target})
            check(result.get("id"), "TCP open failed")
            return result["id"]

        targets = ["172.27.92.1:18080", provisions[1]["peer"]["ipv4"] + ":18080"]
        sockets = [opened(target) for target in targets]

        def exchange(connection, nonce, expected=True):
            result = call(
                "/tcp/exchange",
                {"id": connection, "payload": nonce, "timeout_ms": 1200},
            )
            check(
                bool(result.get("ok")) == expected,
                "Same-socket TCP result differs from active policy",
            )

        exchange(sockets[0], "initial")
        time.sleep(1.1)
        initial = request(hub.url + "/api/peers", hub.token)
        first_handshakes = {peer["id"]: peer["last_handshake_unix"] for peer in initial}
        check(all(first_handshakes.values()), "Initial encrypted handshakes missing")
        started = time.monotonic()
        next_reload = 60
        next_revoke = 180
        next_progress = 30
        latencies = []
        samples = []
        bulk_bytes = 0
        reloads = 0
        revocations = 0
        iterations = 0
        previous = {}
        while time.monotonic() - started < duration:
            iteration_start = time.monotonic()
            elapsed = iteration_start - started
            for connection in sockets:
                exchange(connection, "tick-" + secrets.token_hex(12))
            for target in targets:
                check(
                    call(
                        "/probe",
                        {
                            "protocol": "udp",
                            "target": target,
                            "payload": "udp-" + secrets.token_hex(12),
                            "timeout_ms": 1200,
                        },
                    ).get("ok"),
                    "UDP echo failed",
                )
            latencies.append((time.monotonic() - iteration_start) * 1000)
            if elapsed >= next_reload:
                unrelated = request(
                    hub.url + "/api/groups",
                    hub.token,
                    "POST",
                    {"name": "reload-" + str(reloads)},
                    expected=201,
                )
                request(
                    hub.url + "/api/groups/" + unrelated["id"],
                    hub.token,
                    "DELETE",
                    expected=204,
                )
                transfer = call(
                    "/tcp/transfer",
                    {"id": sockets[0], "bytes": 1 << 20, "timeout_ms": 60000},
                )
                check(
                    transfer.get("ok") and transfer["bytes"] == 1 << 20,
                    "Endurance bulk SHA256 mismatch",
                )
                bulk_bytes += transfer["bytes"]
                reloads += 2
                next_reload += 60
            if elapsed >= next_revoke:
                stale = policy([])
                for connection in sockets:
                    exchange(connection, "revoked-" + secrets.token_hex(8), False)
                check(
                    not call(
                        "/probe",
                        {
                            "protocol": "udp",
                            "target": targets[0],
                            "payload": "revoked",
                            "timeout_ms": 1200,
                        },
                    ).get("ok"),
                    "UDP survived acknowledged revocation",
                )
                request(
                    hub.url + "/api/policy",
                    hub.token,
                    "PUT",
                    {
                        "expected_revision": stale,
                        "changes": [{"group_id": ga, "allowed_groups": [gb]}],
                    },
                    expected=409,
                )
                for connection in sockets:
                    call("/tcp/close", {"id": connection})
                policy([gb])
                sockets = [opened(target) for target in targets]
                revocations += 1
                next_revoke += 180
            status = request(hub.url + "/api/status", hub.token)
            check(
                status["ready"]
                and status["applied_revision"] == status["persisted_revision"],
                "Runtime not ready or activation diverged",
            )
            peers = request(hub.url + "/api/peers", hub.token)
            for peer in peers:
                counts = (peer["received_bytes"], peer["sent_bytes"])
                check(
                    all(
                        current >= old
                        for current, old in zip(
                            counts, previous.get(peer["id"], (0, 0))
                        )
                    ),
                    "Sampled counters decreased",
                )
                previous[peer["id"]] = counts
            check(
                all(process.poll() is None for process in processes),
                "Endurance process exited",
            )
            resource = subprocess.check_output(
                [
                    "ps",
                    "-p",
                    ",".join(str(process.pid) for process in processes),
                    "-o",
                    "pid=,rss=,time=",
                ],
                text=True,
            )
            samples.append(
                {
                    "elapsed_seconds": round(elapsed, 3),
                    "processes": [
                        {
                            "pid": int(line.split()[0]),
                            "rss_kib": int(line.split()[1]),
                            "cpu_time": line.split()[2],
                        }
                        for line in resource.splitlines()
                    ],
                }
            )
            iterations += 1
            if elapsed >= next_progress:
                print(
                    f"PASS so far: {elapsed:.0f}s, {iterations} TCP/UDP rounds, {reloads} reloads, {revocations} live revocations",
                    flush=True,
                )
                next_progress += 30
            time.sleep(max(0, 5 - (time.monotonic() - iteration_start)))
        final = request(hub.url + "/api/peers", hub.token)
        check(
            all(
                peer["last_handshake_unix"] > first_handshakes[peer["id"]]
                for peer in final
            ),
            "Automatic rekey was not observed on both peers",
        )
        ordered = sorted(latencies)
        report = {
            "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "binary": subprocess.check_output(
                [str(binary), "--version"], text=True
            ).strip(),
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "platform": platform.platform(),
            "duration_seconds": round(time.monotonic() - started, 3),
            "iterations": iterations,
            "unrelated_reloads": reloads,
            "live_revocations": revocations,
            "bulk_integrity_bytes": bulk_bytes,
            "all_operations_passed": True,
            "automatic_rekey_both_peers": True,
            "tcp_udp_round_latency_ms": {
                "p50": ordered[len(ordered) // 2],
                "p99": ordered[min(len(ordered) - 1, int(len(ordered) * 0.99))],
                "max": max(ordered),
            },
            "hub_pid": hub.process.pid,
            "resource_samples": samples,
            "final_peer_counters": [
                {"rx_bytes": peer["received_bytes"], "tx_bytes": peer["sent_bytes"]}
                for peer in final
            ],
            "scope": "Two real wireguard-go userspace peers, direct and forwarded TCP/UDP, loopback; round latency includes four HTTP-controlled exchanges, not single-packet RTT; excludes desktop and production hardware.",
        }
        write_report(output, report)
        print("PASS: RC endurance report " + str(output), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/wirehub")
    parser.add_argument("--seconds", type=int, default=1800)
    parser.add_argument("--output", type=Path, default=Path("/tmp/wirehub-soak.json"))
    args = parser.parse_args()
    run(args.binary, args.seconds, args.output)


if __name__ == "__main__":
    run_cli(main, "RC endurance check failed; temporary credentials withheld")
