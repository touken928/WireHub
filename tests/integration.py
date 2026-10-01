#!/usr/bin/env python3
"""Build and test userspace WireGuard using temporary local processes (stdlib only)."""

import contextlib
import json
import os
from pathlib import Path
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CLIENT = Path(__file__).resolve().parent / "client"
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


class CheckFailure(RuntimeError):
    pass


def request(url, token=None, method="GET", body=None, expected=200):
    headers = {}
    data = None
    if token:
        headers["Authorization"] = "Bearer " + token
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with HTTP.open(req, timeout=8) as response:
            status, content = response.status, response.read()
    except urllib.error.HTTPError as error:
        status, content = error.code, error.read()
    if status != expected:
        raise CheckFailure(f"{method} {url.rsplit('/', 1)[-1]} returned HTTP {status}, expected {expected}")
    if not content:
        return None
    try:
        return json.loads(content)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return content.decode("utf-8", errors="replace")


def control(base, token, path, payload=None):
    return request(base + path, token, "POST" if payload is not None else "GET", payload)


def control_post(base, token, path, payload):
    return request(base + path, token, "POST", payload)


def check(condition, message):
    if not condition:
        raise CheckFailure(message)


def scenario(hub_url, clients, hub_token, tokens, endpoint):
    a_url, b_url, c_url = clients
    check(request(hub_url + "/api/health")["ok"], "hub health check failed")
    check(request(hub_url + "/api/ready")["ok"], "hub readiness check failed after successful UDP startup")
    request(hub_url + "/api/setup", expected=401)

    setup = request(hub_url + "/api/setup", hub_token, "POST", {
        "subnet": "172.23.45.0/24", "endpoint": endpoint, "persistent_keepalive": 5,
    })
    check(setup["endpoint"] == endpoint, "hub endpoint mismatch")
    groups = [request(hub_url + "/api/groups", hub_token, "POST", {"name": n}, expected=201) for n in ("A", "B", "C")]
    ga, gb, gc = [group["id"] for group in groups]
    for group, allowed in ((ga, [gb]), (gb, []), (gc, [gb])):
        request(hub_url + f"/api/groups/{group}/acl", hub_token, "PUT", {"allowed_groups": allowed})

    provisions = [request(hub_url + "/api/peers", hub_token, "POST", {
        "name": name, "group_id": group,
    }, expected=201) for name, group in zip("ABC", (ga, gb, gc))]
    configs = [provision["config"] for provision in provisions]
    peer_b = provisions[1]["peer"]
    ip_a, ip_b = provisions[0]["peer"]["ipv4"], peer_b["ipv4"]
    check(all("PrivateKey = " in conf for conf in configs), "provisioned client config missing key")
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
        check(values.get(("Peer", "AllowedIPs")) == "172.23.45.0/24"
              and config.count("AllowedIPs = ") == 1,
              "client config must contain exactly the configured single AllowedIPs CIDR")
        check(values.get(("Peer", "Endpoint")) == endpoint
              and values.get(("Peer", "PersistentKeepalive")) == "5",
              "client config endpoint or persistent keepalive mismatch")

    forwards = []
    for proto in ("tcp", "udp"):
        forwards.append(request(hub_url + "/api/forwards", hub_token, "POST", {
            "name": f"integration-{proto}", "protocol": proto,
            "target_peer_id": peer_b["id"], "target_port": 18080,
            "allowed_group_ids": [ga],
        }, expected=201))
        check(not ({"virtual_ip", "listen_port"} & forwards[-1].keys()), "forward has removed response fields")
    duplicate = {"name": "duplicate", "protocol": "tcp", "target_peer_id": peer_b["id"],
                 "target_port": 18080, "allowed_group_ids": [ga]}
    duplicate_response = request(hub_url + "/api/forwards", hub_token, "POST", duplicate, expected=409)
    private_keys = [next(line.split("=", 1)[1].strip() for line in conf.splitlines()
                         if line.startswith("PrivateKey =")) for conf in configs]
    duplicate_text = json.dumps(duplicate_response)
    check(not any(secret in duplicate_text for secret in [hub_token, *private_keys]),
          "duplicate response leaked private data")

    for url, token, config in zip(clients, tokens, configs):
        control_post(url, token, "/configure", {"config": config})
    control_post(b_url, tokens[1], "/serve", {"port": 18080})
    control_post(a_url, tokens[0], "/serve", {"port": 18080})

    def probe(index, protocol, target, payload, timeout=1200):
        return control_post(clients[index], tokens[index], "/probe", {
            "protocol": protocol, "target": target, "payload": payload, "timeout_ms": timeout,
        })

    def persistent_open(index, target):
        return control_post(clients[index], tokens[index], "/tcp/open", {"target": target})

    def persistent_exchange(index, connection_id, payload, timeout=1200):
        return control_post(clients[index], tokens[index], "/tcp/exchange", {
            "id": connection_id, "payload": payload, "timeout_ms": timeout,
        })

    def persistent_close(index, connection_id):
        return control_post(clients[index], tokens[index], "/tcp/close", {"id": connection_id})

    # Trigger encrypted traffic from all clients, including C, whose forwarding
    # policy is intentionally denied. No OS tools or kernel tunnel are involved.
    for index, label in enumerate("ABC"):
        try:
            probe(index, "udp", "172.23.45.1:9", "handshake-" + label)
        except Exception:
            pass  # The hub tunnel address need not answer; the packet still initiates a handshake.

    def handshake_times():
        return {p["id"]: p.get("last_handshake_unix", 0)
                for p in request(hub_url + "/api/peers", hub_token)}

    def wait_handshakes(after=None):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            peers = request(hub_url + "/api/peers", hub_token)
            if len(peers) == 3 and all(
                    p.get("last_handshake_unix", 0)
                    and (after is None or p.get("last_handshake_unix", 0) > after.get(p["id"], 0))
                    for p in peers):
                return
            time.sleep(.25)
        raise CheckFailure("timed out waiting for nonzero handshakes from all three peers")

    wait_handshakes()

    nonce_tcp, nonce_udp = secrets.token_hex(12), secrets.token_hex(12)
    for proto, nonce in (("tcp", nonce_tcp), ("udp", nonce_udp)):
        result = probe(0, proto, "172.23.45.1:18080", nonce)
        check(result.get("ok") and result.get("payload", nonce) == nonce,
              f"authorized A {proto.upper()} forward did not echo")

    # Keep one real forwarded TCP socket alive across an unrelated policy
    # mutation; the exchanges must use the same client-side net.Conn.
    persistent_id = None
    try:
        opened = persistent_open(0, "172.23.45.1:18080")
        check(opened.get("id"), "could not open persistent forwarded TCP connection")
        persistent_id = opened["id"]
        persistent_before = "persistent-before-" + secrets.token_hex(8)
        result = persistent_exchange(0, persistent_id, persistent_before)
        check(result.get("ok"), "persistent TCP connection failed before unrelated reload")
        request(hub_url + "/api/groups", hub_token, "POST", {"name": "unrelated-reload"}, expected=201)
        persistent_after = "persistent-after-" + secrets.token_hex(8)
        result = persistent_exchange(0, persistent_id, persistent_after)
        check(result.get("ok"), "same persistent TCP connection failed after unrelated reload")

        denied = {}
        for proto in ("tcp", "udp"):
            nonce = f"deny-C-{proto}-{secrets.token_hex(8)}"
            denied[("C", proto)] = nonce
            result = probe(2, proto, "172.23.45.1:18080", nonce, 1000)
            check(not result.get("ok"), f"C {proto.upper()} forward unexpectedly succeeded")

        # Group ACLs are directional: A may initiate to B, but B has no reverse
        # group permission. Confirm the existing UDP flow works and a new B->A
        # initiation is denied, while A is listening so a mistaken arrival is seen.
        nonce_direct = "direct-A-B-" + secrets.token_hex(8)
        direct = probe(0, "udp", f"{ip_b}:18080", nonce_direct)
        check(direct.get("ok") and direct.get("payload", nonce_direct) == nonce_direct,
              "one-way A-to-B direct UDP echo did not succeed")
        nonce_direct_tcp = "direct-A-B-tcp-" + secrets.token_hex(8)
        direct_tcp = probe(0, "tcp", f"{ip_b}:18080", nonce_direct_tcp)
        check(direct_tcp.get("ok"), "one-way A-to-B direct TCP echo did not succeed")
        nonce_reverse = "new-B-A-" + secrets.token_hex(8)
        reverse = probe(1, "udp", f"{ip_a}:18080", nonce_reverse, 1000)
        check(not reverse.get("ok"), "new B-to-A direct UDP initiation unexpectedly succeeded")
        nonce_reverse_tcp = "new-B-A-tcp-" + secrets.token_hex(8)
        reverse_tcp = probe(1, "tcp", f"{ip_a}:18080", nonce_reverse_tcp, 1000)
        check(not reverse_tcp.get("ok"), "new B-to-A direct TCP initiation unexpectedly succeeded")

        # ICMP must also be denied in the reverse direction before the ACL changes.
        denied_icmp = "icmp-denied-" + secrets.token_hex(8)
        result = probe(1, "icmp", ip_a, denied_icmp, 1000)
        check(not result.get("ok"), "B-to-A ICMP unexpectedly succeeded without reverse ACL")

        # Capture exact arrival evidence while all event logs are preserved.
        b_events_before_reverse_grant = json.dumps(
            control(b_url, tokens[1], "/status").get("received", []))
        check(nonce_tcp in b_events_before_reverse_grant
              and nonce_udp in b_events_before_reverse_grant
              and nonce_direct in b_events_before_reverse_grant,
              "backend did not observe all authorized TCP/UDP payloads")
        check(not any(nonce in b_events_before_reverse_grant for nonce in denied.values())
              and nonce_reverse not in b_events_before_reverse_grant
              and nonce_reverse_tcp not in b_events_before_reverse_grant,
              "B backend observed a denied forward or reverse-initiation payload")
        a_events_before_reverse_grant = json.dumps(
            control(a_url, tokens[0], "/status").get("received", []))
        check(nonce_reverse not in a_events_before_reverse_grant
              and nonce_reverse_tcp not in a_events_before_reverse_grant,
              "A backend observed denied B-to-A UDP initiation")

        # Mutate ACL in place. No client reconfiguration, listener restart, event
        # reset, or manufactured handshake is allowed to establish the result.
        request(hub_url + f"/api/groups/{gb}/acl", hub_token, "PUT", {"allowed_groups": [ga]})
        nonce_icmp = "icmp-allowed-" + secrets.token_hex(8)
        result = probe(1, "icmp", ip_a, nonce_icmp)
        check(result.get("ok") and result.get("payload", nonce_icmp) == nonce_icmp,
              "B-to-A ICMP echo did not succeed after reverse ACL grant")
        reverse_tcp_nonce = "direct-B-A-tcp-" + secrets.token_hex(8)
        reverse_tcp = probe(1, "tcp", f"{ip_a}:18080", reverse_tcp_nonce)
        check(reverse_tcp.get("ok"), "B-to-A direct TCP did not succeed after reverse ACL grant")

        # C is allowed to reach B directly even though it is not allowed to use
        # the configured forward; this distinguishes forward policy from routing.
        nonce_c_direct = "direct-C-B-" + secrets.token_hex(8)
        direct_c = probe(2, "udp", f"{ip_b}:18080", nonce_c_direct)
        check(direct_c.get("ok") and direct_c.get("payload", nonce_c_direct) == nonce_c_direct,
              "C direct UDP backend path did not succeed")
        c_events = json.dumps(control(b_url, tokens[1], "/status").get("received", []))
        check(nonce_c_direct in c_events,
              "backend did not observe C's authorized direct payload")

        # Revoke reverse initiation; existing sessions stay untouched, but a fresh
        # reverse TCP initiation must fail immediately after API acknowledgement.
        request(hub_url + f"/api/groups/{gb}/acl", hub_token, "PUT", {"allowed_groups": []})
        revoked_reverse_tcp = "revoked-reverse-tcp-" + secrets.token_hex(8)
        reverse_tcp = probe(1, "tcp", f"{ip_a}:18080", revoked_reverse_tcp, 1000)
        check(not reverse_tcp.get("ok"), "B-to-A TCP initiation succeeded after reverse ACL revoke")

        # Revoke A's forward permission while retaining the same devices, existing
        # connection, listeners, and event history.
        persistent_before_revoke = "persistent-before-revoke-" + secrets.token_hex(8)
        still_live = persistent_exchange(0, persistent_id, persistent_before_revoke)
        check(still_live.get("ok"),
              "existing forwarded TCP connection was already broken before A ACL revocation")
        request(hub_url + f"/api/groups/{ga}/acl", hub_token, "PUT", {"allowed_groups": []})
        persistent_revoked = "persistent-revoked-" + secrets.token_hex(8)
        same_connection = persistent_exchange(0, persistent_id, persistent_revoked, 1000)
        check(not same_connection.get("ok"), "persistent forwarded TCP survived ACL revocation")
        revoked_forward = {}
        for proto in ("tcp", "udp"):
            nonce = f"revoked-{proto}-{secrets.token_hex(8)}"
            revoked_forward[proto] = nonce
            result = probe(0, proto, "172.23.45.1:18080", nonce, 1000)
            check(not result.get("ok"), f"A {proto.upper()} forward succeeded after ACL reload")
        nonce_c_direct_after = "direct-C-B-after-revoke-" + secrets.token_hex(8)
        direct_c_after = probe(2, "tcp", f"{ip_b}:18080", nonce_c_direct_after)
        check(direct_c_after.get("ok"), "unaffected C-to-B direct TCP control failed after A forward revoke")
        b_status = control(b_url, tokens[1], "/status")
        events = json.dumps(b_status.get("received", []))
        check(not any(nonce in events for nonce in revoked_forward.values()),
              "backend observed traffic after ACL revoke")
        check(nonce_tcp in events and nonce_udp in events and nonce_direct_tcp in events
              and persistent_before in events
              and persistent_after in events and persistent_before_revoke in events
              and nonce_c_direct_after in events,
              "backend event log lost authorized payload evidence across policy reloads")
        check(revoked_reverse_tcp not in events and persistent_revoked not in events,
              "backend observed denied TCP payload after ACL acknowledgement")
        a_events_after_revoke = json.dumps(control(a_url, tokens[0], "/status").get("received", []))
        check(reverse_tcp_nonce in a_events_after_revoke
              and revoked_reverse_tcp not in a_events_after_revoke,
              "A backend did not distinguish allowed then revoked B-to-A TCP payloads")
        print("PASS: setup/authenticated provisioning, initial fresh handshakes, same-device ACL reloads, persistent forwarded TCP continuity/revocation, directional direct TCP/UDP/ICMP, and precise backend arrival checks")
    finally:
        if persistent_id:
            persistent_close(0, persistent_id)



def wait_ready(process, label, url=None, address_file=None):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"{label} exited during startup (status {process.returncode})")
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


def run_scenario(directory, client_binary):
    processes = []
    with contextlib.ExitStack() as resources:
        def launch(label, command, environment):
            log = resources.enter_context((directory / f"{label}.log").open("wb"))
            process = subprocess.Popen(command, cwd=directory, env=environment,
                                       stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            processes.append(process)
            resources.callback(stop_processes, [process])
            return process

        # Do not inherit runtime settings from a developer's running Hub.
        hub_environment = {key: value for key, value in os.environ.items()
                           if not key.startswith("WIREHUB_")}
        hub_token = secrets.token_urlsafe(32)
        with reserve_hub_port() as port:
            hub_environment.update(WIREHUB_PORT=str(port), WIREHUB_HTTP_BIND="127.0.0.1",
                                   WIREHUB_ADMIN_TOKEN=hub_token,
                                   WIREHUB_DB=str(directory / "hub.sqlite3"),
                                   WIREHUB_HUB_KEY=str(directory / "hub.key"))
        hub = launch("hub", [str(ROOT / "target/debug/wirehub")], hub_environment)
        hub_url = f"http://127.0.0.1:{port}"
        wait_ready(hub, "Hub", url=hub_url + "/api/health")
        tokens, client_urls = [], []
        for label in "ABC":
            token = secrets.token_urlsafe(32)
            tokens.append(token)
            address_file = directory / f"client-{label}.address"
            environment = dict(os.environ, TEST_CONTROL_TOKEN=token)
            process = launch(f"client-{label}", [str(client_binary), "-listen", "127.0.0.1:0",
                                                "-ready-file", str(address_file)], environment)
            client_urls.append(wait_ready(process, f"Client {label}", address_file=address_file))
        scenario(hub_url, client_urls, hub_token, tokens, f"127.0.0.1:{port}")
        if any(process.poll() is not None for process in processes):
            raise RuntimeError("A test service exited unexpectedly")


def main():
    for command in ("pnpm", "cargo", "go"):
        if shutil.which(command) is None:
            raise RuntimeError(f"Missing required command: {command}")
    os.umask(0o077)
    # Surface interrupted runs as failures while allowing finally blocks to clean up.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    with tempfile.TemporaryDirectory(prefix="wirehub-integration-") as temporary:
        directory = Path(temporary)
        client_binary = directory / "client"
        for command, cwd in (
            (["pnpm", "--dir", "frontend", "install", "--frozen-lockfile"], ROOT),
            (["pnpm", "--dir", "frontend", "build"], ROOT),
            (["cargo", "build", "--locked", "--target-dir", str(ROOT / "target")], ROOT),
            (["go", "test", "-mod=readonly", "./..."], CLIENT),
            (["go", "build", "-mod=readonly", "-trimpath", "-o", str(client_binary), "."], CLIENT),
        ):
            subprocess.run(command, cwd=cwd, check=True)
        run_scenario(directory, client_binary)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        raise SystemExit(130)
    except (CheckFailure, OSError, RuntimeError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        # Service logs stay private and are removed; never print tokens or configs.
        print(f"FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
