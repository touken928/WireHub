#!/usr/bin/env python3
"""Render the current release's documented changes and versioned download links."""

import argparse
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def render(version, repository, document):
    section = re.search(
        rf"^## {re.escape(version)} release\n(.*?)(?=^## |\Z)",
        document,
        re.MULTILINE | re.DOTALL,
    )
    if section is None:
        raise ValueError(f"Missing release notes for {version} in docs/release.md")
    tag = f"v{version}"
    base = f"https://github.com/{repository}/blob/{tag}"
    return f"""## Changes

{section[1].strip()}

## Acceptance and upgrade

See [release verification]({base}/docs/release.md) and the
[supported network behavior and backup procedure]({base}/docs/operations.md).
Before upgrading, stop the service and preserve a verified database/Hub-key pair.

## Downloads

| Platform | Binary |
| --- | --- |
| Linux amd64 | `wirehub-{tag}-linux-amd64` |
| Linux arm64 | `wirehub-{tag}-linux-arm64` |

The web UI is included. `SHA256SUMS` contains binary checksums.
See the [README]({base}/README.md) for startup commands.

## Docker

```sh
docker pull ghcr.io/{repository.lower()}:{version}
```

Supports Linux amd64 and arm64.
"""


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    args = parser.parse_args()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    print(
        render(version, args.repository, (ROOT / "docs/release.md").read_text()), end=""
    )
