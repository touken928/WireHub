# Routing baseline

Run from the repository root:

```sh
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend build
python3 tests/performance.py --iterations 50000 --output /tmp/performance.json
```

The runner builds the release test executable, then measures only the benchmark
process using `wait4` resource usage. It records CPU model, architecture, OS,
compiler, wall time, CPU time and peak RSS. The 84 measurements cover 1/32/128/253
peers; 64/512/1280/1420-byte inner packets; TCP/UDP/ICMP direct routes; TCP/UDP
forward routes with 1,024 configured services; and per-peer/global capacity.
Each routing case warms 1,000 deliveries and measures 50,000 more. p50/p95/p99
measure synchronous ingress validation, checksum, routing preparation and
successful-delivery accounting, including allocation and latency instrumentation.

This is a single-thread plaintext routing baseline. It excludes WireGuard
cryptography, socket IO, client behavior, congestion and network RTT. Its inner
Mbit/s figures must not be used as encrypted network throughput claims. CI
uploads its own measurements; hardware differences make absolute timing gates
inappropriate. Capacity correctness is asserted and fails the test if violated.

The local baseline summarized below was obtained on
2026-10-04 (Asia/Shanghai), Apple M5, 10 logical CPUs, macOS 26.6.2 arm64,
Rust 1.97.1. The benchmark used 99.9% of one CPU, 27.2 MiB peak RSS, and 2.37 s
wall time. Across all packet sizes, protocols and modes:

| Peers | Routing throughput range (million packets/s) | Worst measured p99 (µs) |
| --- | --- | --- |
| 1 | 1.38–3.79 | 0.792 |
| 32 | 1.32–3.89 | 0.917 |
| 128 | 1.38–3.89 | 0.792 |
| 253 | 1.36–3.92 | 0.916 |

Capacity accepted 256, 8,192, 16,384 and 16,384 unique UDP flows respectively;
the remaining attempts were rejected. This confirms the shared 256-per-initiator
and 16,384-global limits under this matrix. Network compatibility and encrypted
end-to-end acceptance is described in [release verification](release.md).

Raw JSON measurements are generated test artifacts. CI uploads its measurements;
the original local report is archived under the ignored
`artifacts/acceptance/2026-10-04/` directory. Retain full measurements when
comparing runs on the same hardware and configuration.
