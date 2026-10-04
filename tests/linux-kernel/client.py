"""Linux kernel WireGuard helper inside a disposable NET_ADMIN container."""

import configparser
import hashlib
import json
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path


def command(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def configure(text):
    config = configparser.ConfigParser(interpolation=None)
    config.optionxform = str
    config.read_string(text)
    address = config["Interface"].pop("Address")
    mtu = config["Interface"].pop("MTU", "1420")
    allowed = config["Peer"]["AllowedIPs"]
    # wg accepts keys/peer settings only. Addresses, MTU and routes belong to iproute2.
    stripped = "[Interface]\n" + "".join(
        f"{key} = {value}\n" for key, value in config["Interface"].items()
    )
    stripped += "[Peer]\n" + "".join(
        f"{key} = {value}\n" for key, value in config["Peer"].items()
    )
    command("ip", "link", "add", "wg0", "type", "wireguard")
    subprocess.run(
        ["wg", "setconf", "wg0", "/dev/stdin"], input=stripped, text=True, check=True
    )
    command("ip", "addr", "add", address, "dev", "wg0")
    command("ip", "link", "set", "wg0", "mtu", mtu, "up")
    command("ip", "route", "add", allowed, "dev", "wg0")
    return {"ok": True}


def record(data):
    with open("/tmp/arrivals.jsonl", "a") as output:
        output.write(
            json.dumps({"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
            + "\n"
        )


def serve(address, port):
    def tcp_client(connection):
        with connection:
            while data := connection.recv(65536):
                record(data)
                connection.sendall(data)

    def tcp_loop():
        with socket.socket() as tcp:
            tcp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            tcp.bind((address, port))
            tcp.listen()
            while True:
                connection, _ = tcp.accept()
                threading.Thread(
                    target=tcp_client, args=(connection,), daemon=True
                ).start()

    threading.Thread(target=tcp_loop, daemon=True).start()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp:
        udp.bind((address, port))
        while True:
            data, sender = udp.recvfrom(65535)
            record(data)
            udp.sendto(data, sender)


def probe(request):
    protocol, target, port = (
        request["protocol"],
        request["target"],
        request.get("port", 18080),
    )
    payload = request.get("payload", "kernel-wireguard").encode()
    started = time.monotonic()
    try:
        if protocol == "icmp":
            subprocess.run(
                ["ping", "-c", "1", "-W", "2", target],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=True,
            )
        elif protocol == "tcp":
            with socket.create_connection((target, port), timeout=5) as connection:
                if request.get("bytes"):
                    payload = bytes(
                        (i * 31 + 17) & 255 for i in range(request["bytes"])
                    )
                writer = threading.Thread(target=connection.sendall, args=(payload,))
                writer.start()
                received = bytearray()
                while len(received) < len(payload):
                    part = connection.recv(min(65536, len(payload) - len(received)))
                    if not part:
                        raise OSError("premature EOF")
                    received.extend(part)
                writer.join(timeout=5)
                if bytes(received) != payload:
                    raise OSError("integrity mismatch")
        else:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp:
                udp.settimeout(2)
                udp.setsockopt(
                    socket.IPPROTO_IP, 10, 0
                )  # IP_MTU_DISCOVER = IP_PMTUDISC_DONT
                udp.sendto(payload, (target, port))
                if udp.recv(65535) != payload:
                    raise OSError("integrity mismatch")
        return {
            "ok": True,
            "bytes": len(payload),
            "seconds": time.monotonic() - started,
        }
    except (OSError, subprocess.CalledProcessError):
        return {"ok": False}


if __name__ == "__main__":
    action = sys.argv[1]
    if action == "wait":
        threading.Event().wait()
    elif action == "configure":
        print(json.dumps(configure(sys.stdin.read())))
    elif action == "serve":
        serve(sys.argv[2], int(sys.argv[3]))
    elif action == "probe":
        print(json.dumps(probe(json.load(sys.stdin))))
    elif action == "status":
        handshakes = command("wg", "show", "wg0", "latest-handshakes")
        rows = (
            [
                json.loads(line)
                for line in Path("/tmp/arrivals.jsonl").read_text().splitlines()
            ]
            if Path("/tmp/arrivals.jsonl").exists()
            else []
        )
        print(
            json.dumps(
                {
                    "latest_handshake": max(
                        (int(line.split()[-1]) for line in handshakes.splitlines()),
                        default=0,
                    ),
                    "arrivals": rows[-256:],
                    "kernel": command("uname", "-r"),
                    "wireguard_tools": command("wg", "--version"),
                }
            )
        )
