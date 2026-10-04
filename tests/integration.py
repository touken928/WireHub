#!/usr/bin/env python3
"""Build and test userspace WireGuard using temporary local processes (stdlib only)."""

import argparse
import contextlib
import json
import os
import secrets
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

from network_relay import Relay
from support import (
    CLIENT,
    HTTP,
    ROOT,
    CheckFailure,
    Hub,
    build_client,
    check,
    control,
    control_post,
    launch_client,
    request,
    reserve_hub_port,
    run_cli,
    stop_processes,
    wait_ready,
)


def scenario(hub_url, clients, hub_token, tokens, endpoint, relays=None):
    a_url, b_url, c_url = clients
    check(request(hub_url + "/api/health")["ok"], "hub health check failed")
    check(
        request(hub_url + "/api/ready")["ok"],
        "hub readiness check failed after successful UDP startup",
    )
    request(hub_url + "/api/setup", expected=401)

    setup = request(
        hub_url + "/api/setup",
        hub_token,
        "POST",
        {
            "subnet": "172.23.45.0/24",
            "endpoint": endpoint,
            "persistent_keepalive": 5,
        },
    )
    check(setup["endpoint"] == endpoint, "hub endpoint mismatch")
    groups = [
        request(hub_url + "/api/groups", hub_token, "POST", {"name": n}, expected=201)
        for n in ("A", "B", "C")
    ]
    ga, gb, gc = [group["id"] for group in groups]
    before = request(hub_url + "/api/config", hub_token)["revision"]
    batch = {
        "expected_revision": before,
        "changes": [
            {"group_id": group, "allowed_groups": allowed}
            for group, allowed in ((ga, [gb]), (gb, []), (gc, [gb]))
        ],
    }
    applied = request(hub_url + "/api/policy", hub_token, "PUT", batch)
    check(
        applied["revision"] == before + 1 and applied["applied_revision"] >= before + 1,
        "batch ACL did not commit and activate one revision",
    )
    request(hub_url + "/api/policy", hub_token, "PUT", batch, expected=409)
    check(
        request(hub_url + "/api/config", hub_token)["revision"] == before + 1,
        "stale batch changed the configuration",
    )
    status = request(hub_url + "/api/status", hub_token)
    check(
        status["ready"] and status["applied_revision"] == status["persisted_revision"],
        "persisted and active revisions differ after acknowledged batch",
    )

    provisions = [
        request(
            hub_url + "/api/peers",
            hub_token,
            "POST",
            {
                "name": name,
                "group_id": group,
            },
            expected=201,
        )
        for name, group in zip("ABC", (ga, gb, gc))
    ]
    configs = [provision["config"] for provision in provisions]
    peer_b = provisions[1]["peer"]
    ip_a, ip_b = provisions[0]["peer"]["ipv4"], peer_b["ipv4"]
    check(
        all("PrivateKey = " in conf for conf in configs),
        "provisioned client config missing key",
    )
    for config in configs:
        values = {}
        section = None
        for line in config.splitlines():
            line = line.strip()
            if line.startswith("[") and line.endswith("]"):
                section = line[1:-1]
            elif "=" in line:
                key, value = (part.strip() for part in line.split("=", 1))
                values[(section, key)] = value
        check(
            values.get(("Peer", "AllowedIPs")) == "172.23.45.0/24"
            and config.count("AllowedIPs = ") == 1,
            "client config must contain exactly the configured single AllowedIPs CIDR",
        )
        check(
            values.get(("Interface", "MTU")) == "1420",
            "client config missing explicit MTU",
        )
        check(
            values.get(("Peer", "Endpoint")) == endpoint
            and values.get(("Peer", "PersistentKeepalive")) == "5",
            "client config endpoint or persistent keepalive mismatch",
        )

    forwards = []
    for proto in ("tcp", "udp"):
        forwards.append(
            request(
                hub_url + "/api/forwards",
                hub_token,
                "POST",
                {
                    "name": f"integration-{proto}",
                    "protocol": proto,
                    "target_peer_id": peer_b["id"],
                    "target_port": 18080,
                    "allowed_group_ids": [ga],
                },
                expected=201,
            )
        )
        check(
            not ({"virtual_ip", "listen_port"} & forwards[-1].keys()),
            "forward has removed response fields",
        )
    duplicate = {
        "name": "duplicate",
        "protocol": "tcp",
        "target_peer_id": peer_b["id"],
        "target_port": 18080,
        "allowed_group_ids": [ga],
    }
    duplicate_response = request(
        hub_url + "/api/forwards", hub_token, "POST", duplicate, expected=409
    )
    private_keys = [
        next(
            line.split("=", 1)[1].strip()
            for line in conf.splitlines()
            if line.startswith("PrivateKey =")
        )
        for conf in configs
    ]
    duplicate_text = json.dumps(duplicate_response)
    check(
        not any(secret in duplicate_text for secret in [hub_token, *private_keys]),
        "duplicate response leaked private data",
    )

    for index, (url, token, config) in enumerate(zip(clients, tokens, configs)):
        if relays:
            config = config.replace(
                "Endpoint = " + endpoint, "Endpoint = " + relays[index].endpoint
            )
        control_post(url, token, "/configure", {"config": config})
    control_post(b_url, tokens[1], "/serve", {"port": 18080})
    control_post(a_url, tokens[0], "/serve", {"port": 18080})

    def probe(index, protocol, target, payload, timeout=1200):
        return control_post(
            clients[index],
            tokens[index],
            "/probe",
            {
                "protocol": protocol,
                "target": target,
                "payload": payload,
                "timeout_ms": timeout,
            },
        )

    def persistent_open(index, target):
        return control_post(
            clients[index], tokens[index], "/tcp/open", {"target": target}
        )

    def persistent_exchange(index, connection_id, payload, timeout=1200):
        return control_post(
            clients[index],
            tokens[index],
            "/tcp/exchange",
            {
                "id": connection_id,
                "payload": payload,
                "timeout_ms": timeout,
            },
        )

    def persistent_close(index, connection_id):
        return control_post(
            clients[index], tokens[index], "/tcp/close", {"id": connection_id}
        )

    # Trigger encrypted traffic from all clients, including C, whose forwarding
    # policy is intentionally denied. No OS tools or kernel tunnel are involved.
    for index, label in enumerate("ABC"):
        try:
            probe(index, "udp", "172.23.45.1:9", "handshake-" + label)
        except Exception:
            pass  # The hub tunnel address need not answer; the packet still initiates a handshake.

    def handshake_times():
        return {
            p["id"]: p.get("last_handshake_unix", 0)
            for p in request(hub_url + "/api/peers", hub_token)
        }

    def wait_handshakes(after=None):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            peers = request(hub_url + "/api/peers", hub_token)
            if len(peers) == 3 and all(
                p.get("last_handshake_unix", 0)
                and (
                    after is None
                    or p.get("last_handshake_unix", 0) > after.get(p["id"], 0)
                )
                for p in peers
            ):
                return
            time.sleep(0.25)
        raise CheckFailure(
            "timed out waiting for nonzero handshakes from all three peers"
        )

    wait_handshakes()

    nonce_tcp, nonce_udp = secrets.token_hex(12), secrets.token_hex(12)
    for proto, nonce in (("tcp", nonce_tcp), ("udp", nonce_udp)):
        result = probe(0, proto, "172.23.45.1:18080", nonce)
        check(
            result.get("ok") and result.get("payload", nonce) == nonce,
            f"authorized A {proto.upper()} forward did not echo",
        )

    # Keep one real forwarded TCP socket alive across an unrelated policy
    # mutation; the exchanges must use the same client-side net.Conn.
    persistent_id = None
    try:
        opened = persistent_open(0, "172.23.45.1:18080")
        check(opened.get("id"), "could not open persistent forwarded TCP connection")
        persistent_id = opened["id"]
        initial_arrivals = json.dumps(
            control(b_url, tokens[1], "/status").get("received", [])
        )
        check(
            nonce_tcp in initial_arrivals and nonce_udp in initial_arrivals,
            "Initial forwarded payload evidence missing",
        )
        transferred = request(
            a_url + "/tcp/transfer",
            tokens[0],
            "POST",
            {"id": persistent_id, "bytes": 4 << 20, "timeout_ms": 60000},
            timeout=65,
        )
        check(
            transferred.get("ok") and transferred["bytes"] == 4 << 20,
            "4 MiB TCP transfer failed integrity validation",
        )
        for protocol in ("tcp", "udp"):
            mtu_payload = "mtu-safe-" + "x" * 1200
            check(
                probe(0, protocol, "172.23.45.1:18080", mtu_payload, 5000).get("ok"),
                f"MTU-safe {protocol} payload failed",
            )
        print(
            f"PASS: 4 MiB same-socket TCP integrity transfer ({transferred['seconds']:.3f}s) and MTU-safe TCP/UDP payloads"
        )
        if relays:
            relay = relays[0]
            relay.enabled = True
            impaired = request(
                a_url + "/tcp/transfer",
                tokens[0],
                "POST",
                {"id": persistent_id, "bytes": 1 << 20, "timeout_ms": 60000},
                timeout=65,
            )
            check(
                impaired.get("ok"),
                "Loss/delay/reordering TCP integrity transfer failed",
            )
            relay.enabled = False
            check(
                relay.dropped > 0 and relay.reordered > 0,
                "Impairment gates did not exercise loss and reordering",
            )
            relay.migrate()
            migrated = persistent_exchange(
                0, persistent_id, "nat-migrated-" + secrets.token_hex(8), 5000
            )
            check(
                migrated.get("ok"),
                "Same TCP socket did not recover after NAT endpoint migration",
            )
            # UDP flow timeout is tested deterministically in Rust; this checks actual
            # idle encrypted-device recovery without restarting or reconfiguring it.
            time.sleep(float(os.environ.get("WIREHUB_IDLE_SECONDS", "65")))
            resumed = persistent_exchange(
                0, persistent_id, "idle-resumed-" + secrets.token_hex(8), 5000
            )
            check(
                resumed.get("ok"),
                "Same established TCP socket did not resume after idle",
            )
            recovered_udp = probe(
                0, "udp", "172.23.45.1:18080", "idle-udp-recovered", 5000
            )
            check(recovered_udp.get("ok"), "Fresh UDP flow did not recover after idle")
            print(
                f"PASS: deterministic 5% encrypted data loss, 12 ms delay and 30 ms reordering (drops={relay.dropped}, reordered={relay.reordered}), 1 MiB integrity, NAT migration, idle same-socket TCP and fresh UDP recovery"
            )
            # Re-establish event evidence after the long transfer evicted old records.
            check(
                probe(0, "tcp", "172.23.45.1:18080", nonce_tcp, 5000).get("ok"),
                "TCP warm recovery failed",
            )
            check(
                probe(0, "udp", "172.23.45.1:18080", nonce_udp, 5000).get("ok"),
                "UDP warm recovery failed",
            )
        persistent_before = "persistent-before-" + secrets.token_hex(8)
        result = persistent_exchange(0, persistent_id, persistent_before)
        check(
            result.get("ok"), "persistent TCP connection failed before unrelated reload"
        )
        request(
            hub_url + "/api/groups",
            hub_token,
            "POST",
            {"name": "unrelated-reload"},
            expected=201,
        )
        persistent_after = "persistent-after-" + secrets.token_hex(8)
        result = persistent_exchange(0, persistent_id, persistent_after)
        check(
            result.get("ok"),
            "same persistent TCP connection failed after unrelated reload",
        )

        denied = {}
        for proto in ("tcp", "udp"):
            nonce = f"deny-C-{proto}-{secrets.token_hex(8)}"
            denied[("C", proto)] = nonce
            result = probe(2, proto, "172.23.45.1:18080", nonce, 1000)
            check(
                not result.get("ok"),
                f"C {proto.upper()} forward unexpectedly succeeded",
            )

        # Group ACLs are directional: A may initiate to B, but B has no reverse
        # group permission. Confirm the existing UDP flow works and a new B->A
        # initiation is denied, while A is listening so a mistaken arrival is seen.
        nonce_direct = "direct-A-B-" + secrets.token_hex(8)
        direct = probe(0, "udp", f"{ip_b}:18080", nonce_direct)
        check(
            direct.get("ok") and direct.get("payload", nonce_direct) == nonce_direct,
            "one-way A-to-B direct UDP echo did not succeed",
        )
        nonce_direct_tcp = "direct-A-B-tcp-" + secrets.token_hex(8)
        direct_tcp = probe(0, "tcp", f"{ip_b}:18080", nonce_direct_tcp)
        check(direct_tcp.get("ok"), "one-way A-to-B direct TCP echo did not succeed")
        nonce_reverse = "new-B-A-" + secrets.token_hex(8)
        reverse = probe(1, "udp", f"{ip_a}:18080", nonce_reverse, 1000)
        check(
            not reverse.get("ok"),
            "new B-to-A direct UDP initiation unexpectedly succeeded",
        )
        nonce_reverse_tcp = "new-B-A-tcp-" + secrets.token_hex(8)
        reverse_tcp = probe(1, "tcp", f"{ip_a}:18080", nonce_reverse_tcp, 1000)
        check(
            not reverse_tcp.get("ok"),
            "new B-to-A direct TCP initiation unexpectedly succeeded",
        )

        # ICMP must also be denied in the reverse direction before the ACL changes.
        denied_icmp = "icmp-denied-" + secrets.token_hex(8)
        result = probe(1, "icmp", ip_a, denied_icmp, 1000)
        check(
            not result.get("ok"),
            "B-to-A ICMP unexpectedly succeeded without reverse ACL",
        )

        # Capture exact arrival evidence while all event logs are preserved.
        b_events_before_reverse_grant = json.dumps(
            control(b_url, tokens[1], "/status").get("received", [])
        )
        check(
            nonce_direct in b_events_before_reverse_grant,
            "backend did not observe all authorized TCP/UDP payloads",
        )
        check(
            not any(nonce in b_events_before_reverse_grant for nonce in denied.values())
            and nonce_reverse not in b_events_before_reverse_grant
            and nonce_reverse_tcp not in b_events_before_reverse_grant,
            "B backend observed a denied forward or reverse-initiation payload",
        )
        a_events_before_reverse_grant = json.dumps(
            control(a_url, tokens[0], "/status").get("received", [])
        )
        check(
            nonce_reverse not in a_events_before_reverse_grant
            and nonce_reverse_tcp not in a_events_before_reverse_grant,
            "A backend observed denied B-to-A UDP initiation",
        )

        # Mutate ACL in place. No client reconfiguration, listener restart, event
        # reset, or manufactured handshake is allowed to establish the result.
        request(
            hub_url + f"/api/groups/{gb}/acl",
            hub_token,
            "PUT",
            {"allowed_groups": [ga]},
        )
        nonce_icmp = "icmp-allowed-" + secrets.token_hex(8)
        result = probe(1, "icmp", ip_a, nonce_icmp)
        check(
            result.get("ok") and result.get("payload", nonce_icmp) == nonce_icmp,
            "B-to-A ICMP echo did not succeed after reverse ACL grant",
        )
        reverse_tcp_nonce = "direct-B-A-tcp-" + secrets.token_hex(8)
        reverse_tcp = probe(1, "tcp", f"{ip_a}:18080", reverse_tcp_nonce)
        check(
            reverse_tcp.get("ok"),
            "B-to-A direct TCP did not succeed after reverse ACL grant",
        )

        # C is allowed to reach B directly even though it is not allowed to use
        # the configured forward; this distinguishes forward policy from routing.
        nonce_c_direct = "direct-C-B-" + secrets.token_hex(8)
        direct_c = probe(2, "udp", f"{ip_b}:18080", nonce_c_direct)
        check(
            direct_c.get("ok")
            and direct_c.get("payload", nonce_c_direct) == nonce_c_direct,
            "C direct UDP backend path did not succeed",
        )
        c_events = json.dumps(control(b_url, tokens[1], "/status").get("received", []))
        check(
            nonce_c_direct in c_events,
            "backend did not observe C's authorized direct payload",
        )

        # Revoke reverse initiation; existing sessions stay untouched, but a fresh
        # reverse TCP initiation must fail immediately after API acknowledgement.
        request(
            hub_url + f"/api/groups/{gb}/acl", hub_token, "PUT", {"allowed_groups": []}
        )
        revoked_reverse_tcp = "revoked-reverse-tcp-" + secrets.token_hex(8)
        reverse_tcp = probe(1, "tcp", f"{ip_a}:18080", revoked_reverse_tcp, 1000)
        check(
            not reverse_tcp.get("ok"),
            "B-to-A TCP initiation succeeded after reverse ACL revoke",
        )

        # Revoke A's forward permission while retaining the same devices, existing
        # connection, listeners, and event history.
        persistent_before_revoke = "persistent-before-revoke-" + secrets.token_hex(8)
        still_live = persistent_exchange(0, persistent_id, persistent_before_revoke)
        check(
            still_live.get("ok"),
            "existing forwarded TCP connection was already broken before A ACL revocation",
        )
        request(
            hub_url + f"/api/groups/{ga}/acl", hub_token, "PUT", {"allowed_groups": []}
        )
        persistent_revoked = "persistent-revoked-" + secrets.token_hex(8)
        same_connection = persistent_exchange(
            0, persistent_id, persistent_revoked, 1000
        )
        check(
            not same_connection.get("ok"),
            "persistent forwarded TCP survived ACL revocation",
        )
        revoked_forward = {}
        for proto in ("tcp", "udp"):
            nonce = f"revoked-{proto}-{secrets.token_hex(8)}"
            revoked_forward[proto] = nonce
            result = probe(0, proto, "172.23.45.1:18080", nonce, 1000)
            check(
                not result.get("ok"),
                f"A {proto.upper()} forward succeeded after ACL reload",
            )
        nonce_c_direct_after = "direct-C-B-after-revoke-" + secrets.token_hex(8)
        direct_c_after = probe(2, "tcp", f"{ip_b}:18080", nonce_c_direct_after)
        check(
            direct_c_after.get("ok"),
            "unaffected C-to-B direct TCP control failed after A forward revoke",
        )
        b_status = control(b_url, tokens[1], "/status")
        events = json.dumps(b_status.get("received", []))
        check(
            not any(nonce in events for nonce in revoked_forward.values()),
            "backend observed traffic after ACL revoke",
        )
        check(
            nonce_direct_tcp in events
            and persistent_before in events
            and persistent_after in events
            and persistent_before_revoke in events
            and nonce_c_direct_after in events,
            "backend event log lost authorized payload evidence across policy reloads",
        )
        check(
            revoked_reverse_tcp not in events and persistent_revoked not in events,
            "backend observed denied TCP payload after ACL acknowledgement",
        )
        a_events_after_revoke = json.dumps(
            control(a_url, tokens[0], "/status").get("received", [])
        )
        check(
            reverse_tcp_nonce in a_events_after_revoke
            and revoked_reverse_tcp not in a_events_after_revoke,
            "A backend did not distinguish allowed then revoked B-to-A TCP payloads",
        )
        print(
            "PASS: setup/authenticated provisioning, initial fresh handshakes, same-device ACL reloads, persistent forwarded TCP continuity/revocation, directional direct TCP/UDP/ICMP, and precise backend arrival checks"
        )
    finally:
        if persistent_id:
            persistent_close(0, persistent_id)


def initial_invalid_snapshot_fails_before_http(directory):
    """Exercise the real binary's startup ordering with a persisted bad snapshot."""
    database = directory / "invalid-startup.sqlite3"
    key = directory / "invalid-startup.key"
    token = secrets.token_urlsafe(32)
    environment = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("WIREHUB_")
    }
    with reserve_hub_port() as port:
        environment.update(
            WIREHUB_PORT=str(port),
            WIREHUB_HTTP_BIND="127.0.0.1",
            WIREHUB_ADMIN_TOKEN=token,
            WIREHUB_DB=str(database),
            WIREHUB_HUB_KEY=str(key),
        )

    seed_log_path = directory / "invalid-startup-seed.log"
    with seed_log_path.open("wb") as seed_log:
        seed = subprocess.Popen(
            [str(ROOT / "target/debug/wirehub")],
            cwd=directory,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=seed_log,
            stderr=seed_log,
        )
        try:
            base = wait_ready(
                seed,
                "invalid-snapshot database seeder",
                url=f"http://127.0.0.1:{port}/api/health",
            )
            request(
                base + "/setup",
                token,
                "POST",
                {
                    "subnet": "172.23.45.0/24",
                    "endpoint": f"127.0.0.1:{port}",
                    "persistent_keepalive": 5,
                },
            )
            group = request(
                base + "/groups",
                token,
                "POST",
                {"name": "invalid-startup"},
                expected=201,
            )
            request(
                base + "/peers",
                token,
                "POST",
                {
                    "name": "invalid-key",
                    "group_id": group["id"],
                },
                expected=201,
            )
        finally:
            stop_processes([seed])

    # Store::open intentionally rejects malformed persisted settings. A peer key
    # value is accepted by the current SQL schema/open validation, but rejected
    # while compiling the initial runtime snapshot.
    with sqlite3.connect(database) as db:
        db.execute(
            "UPDATE peers SET public_key = ? WHERE id = (SELECT id FROM peers LIMIT 1)",
            ("not-base64!",),
        )
        check(
            db.execute("SELECT public_key FROM peers LIMIT 1").fetchone()[0]
            == "not-base64!",
            "failed to persist invalid runtime peer key",
        )

    process_log_path = directory / "invalid-startup.log"
    with process_log_path.open("wb") as process_log:
        process = subprocess.Popen(
            [str(ROOT / "target/debug/wirehub")],
            cwd=directory,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=process_log,
            stderr=process_log,
        )
        try:
            deadline = time.monotonic() + 10
            while process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.05)
            check(
                process.poll() is not None,
                "invalid initial snapshot process did not exit",
            )
            check(
                process.returncode != 0,
                "invalid initial snapshot process exited successfully",
            )
            with process_log_path.open("rb") as log:
                output = log.read().decode("utf-8", errors="replace")
            check(
                "Snapshot(SnapshotLoadError)" in output,
                f"startup did not report initial snapshot failure: {output.strip()}",
            )
            try:
                with HTTP.open(
                    f"http://127.0.0.1:{port}/api/health", timeout=1
                ) as response:
                    check(
                        response.status != 200,
                        "invalid initial snapshot process served HTTP 200",
                    )
            except (OSError, urllib.error.URLError):
                pass
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
                check(
                    probe.connect_ex(("127.0.0.1", port)) != 0,
                    "HTTP port remained listening after initial snapshot failure",
                )
        finally:
            stop_processes([process])


def run_scenario(directory, client_binary):
    initial_invalid_snapshot_fails_before_http(directory)
    with contextlib.ExitStack() as resources:
        hub = Hub(ROOT / "target/debug/wirehub", directory / "hub")
        resources.callback(hub.stop, False)
        clients = [
            launch_client(client_binary, directory, label, resources) for label in "ABC"
        ]
        relays = None
        if os.environ.get("WIREHUB_NETWORK_REGRESSIONS") == "all":
            port = int(hub.environment["WIREHUB_PORT"])
            relays = [Relay(("127.0.0.1", port)) for _ in clients]
            for relay in relays:
                resources.callback(relay.close)
        scenario(
            hub.url,
            [client.url for client in clients],
            hub.token,
            [client.token for client in clients],
            hub.url.removeprefix("http://"),
            relays,
        )
        check(
            all(
                process.poll() is None
                for process in [hub.process, *(client.process for client in clients)]
            ),
            "A test service exited unexpectedly",
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    for command in ("pnpm", "cargo", "go"):
        if shutil.which(command) is None:
            raise RuntimeError(f"Missing required command: {command}")
    with tempfile.TemporaryDirectory(prefix="wirehub-integration-") as temporary:
        directory = Path(temporary)
        for command, cwd in (
            (["pnpm", "--dir", "frontend", "install", "--frozen-lockfile"], ROOT),
            (["pnpm", "--dir", "frontend", "build"], ROOT),
            (
                ["cargo", "build", "--locked", "--target-dir", str(ROOT / "target")],
                ROOT,
            ),
            (["go", "test", "-mod=readonly", "./..."], CLIENT),
        ):
            subprocess.run(command, cwd=cwd, check=True)
        run_scenario(directory, build_client(directory))


if __name__ == "__main__":
    run_cli(main, "Integration check failed; temporary credentials withheld")
