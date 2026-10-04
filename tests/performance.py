#!/usr/bin/env python3
"""Release routing baseline and isolated process resource usage; never opens hub state."""

import argparse
import json
import os
import platform
import subprocess
import sys
import time
from pathlib import Path

from support import ROOT, run_cli, stop_processes, write_report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output", type=Path, default=Path("/tmp/wirehub-performance.json")
    )
    parser.add_argument("--iterations", type=int, default=20000)
    args = parser.parse_args()
    if args.iterations < 1000:
        parser.error("iterations must be at least 1000")
    build = subprocess.run(
        ["cargo", "test", "--release", "--locked", "--no-run", "--message-format=json"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        check=True,
    )
    executables = [
        item["executable"]
        for line in build.stdout.splitlines()
        if (item := json.loads(line)).get("executable")
        and item.get("profile", {}).get("test")
    ]
    if len(executables) != 1:
        raise RuntimeError("Expected exactly one benchmark test executable")
    import tempfile

    with tempfile.TemporaryFile(mode="w+") as output:
        env = {**os.environ, "WIREHUB_BENCH_ITERATIONS": str(args.iterations)}
        started = time.monotonic()
        process = subprocess.Popen(
            [
                executables[0],
                "--ignored",
                "--exact",
                "kernel::benchmark::baseline",
                "--nocapture",
            ],
            cwd=ROOT,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
        )
        try:
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
        finally:
            if process.returncode is None:
                stop_processes([process])
        elapsed = time.monotonic() - started
        output.seek(0)
        log = output.read()
    print(log, end="")
    if process.returncode:
        raise subprocess.CalledProcessError(process.returncode, process.args)
    rows = [
        json.loads(line.removeprefix("BENCH "))
        for line in log.splitlines()
        if line.startswith("BENCH ")
    ]
    if len(rows) != 84:
        raise RuntimeError(f"Incomplete benchmark matrix: {len(rows)}/84")
    cpu = platform.processor()
    if sys.platform == "darwin":
        try:
            cpu = subprocess.check_output(
                ["sysctl", "-n", "machdep.cpu.brand_string"],
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
        except (OSError, subprocess.CalledProcessError):
            cpu += " (model unavailable)"
    report = {
        "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "cpu": cpu,
        "logical_cpus": os.cpu_count(),
        "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
        "scope": "Single-thread in-process plaintext routing, including validation, checksum, prepare and delivered accounting. Excludes WireGuard crypto, sockets and network RTT; latency is per-packet routing processing, not client latency.",
        "wall_seconds": elapsed,
        "user_cpu_seconds": usage.ru_utime,
        "system_cpu_seconds": usage.ru_stime,
        "cpu_percent_one_core": (usage.ru_utime + usage.ru_stime) / elapsed * 100,
        "peak_rss_bytes": usage.ru_maxrss * (1 if sys.platform == "darwin" else 1024),
        "iterations_per_case": args.iterations,
        "results": rows,
    }
    write_report(args.output, report)
    print(f"Report: {args.output}")


if __name__ == "__main__":
    run_cli(main, "Performance check failed; benchmark report was not completed")
