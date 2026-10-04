"""Isolated UDP NAT relay with deterministic impairment; stdlib only."""

import heapq
import select
import socket
import threading
import time


class Relay:
    def __init__(self, hub):
        self.hub = hub
        self.front = self._socket()
        self.back = self._socket()
        self.endpoint = f"127.0.0.1:{self.front.getsockname()[1]}"
        self.client = None
        self.enabled = False
        self.stopped = threading.Event()
        self.lock = threading.Lock()
        self.packets = self.dropped = self.delayed = self.reordered = 0
        self.error = None
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    @staticmethod
    def _socket():
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind(("127.0.0.1", 0))
        sock.setblocking(False)
        return sock

    def migrate(self):
        with self.lock:
            old = self.back
            self.back = self._socket()
            port = self.back.getsockname()[1]
            old.close()
            return port

    def _run(self):
        pending = []
        serial = 0
        try:
            while not self.stopped.is_set():
                with self.lock:
                    sockets = [self.front, self.back]
                try:
                    readable, _, _ = select.select(sockets, [], [], 0.005)
                except (OSError, ValueError):
                    continue  # A deliberate NAT migration closed the old outlet.
                for sock in readable:
                    try:
                        data, sender = sock.recvfrom(65535)
                    except (OSError, BlockingIOError):
                        continue
                    outbound = sock is self.front
                    if outbound:
                        self.client = sender
                    elif sender != self.hub:
                        continue
                    serial += 1
                    delay = 0
                    if self.enabled and data[:4] == b"\x04\x00\x00\x00":
                        self.packets += 1
                        if self.packets % 20 == 0:
                            self.dropped += 1
                            continue
                        delay = 0.012
                        self.delayed += 1
                        if self.packets % 7 == 0:
                            delay += 0.030
                            self.reordered += 1
                    heapq.heappush(
                        pending, (time.monotonic() + delay, serial, outbound, data)
                    )
                while pending and pending[0][0] <= time.monotonic():
                    _, _, outbound, data = heapq.heappop(pending)
                    with self.lock:
                        if outbound:
                            self.back.sendto(data, self.hub)
                        elif self.client:
                            self.front.sendto(data, self.client)
        except Exception as error:
            self.error = error

    def close(self):
        self.stopped.set()
        self.thread.join(timeout=2)
        self.front.close()
        self.back.close()
        if self.thread.is_alive():
            raise RuntimeError("Relay did not stop")
        if self.error:
            raise RuntimeError("Relay failed") from self.error
