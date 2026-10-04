#!/usr/bin/env python3
"""Reject mismatched package, lockfile, frontend and release-tag versions."""

import argparse
import json
import re
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def check(tag=None, binary=None, build_id=None):
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    packages = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    locked = next(
        package["version"] for package in packages if package["name"] == "wirehub"
    )
    frontend = json.loads((ROOT / "frontend/package.json").read_text())["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", cargo):
        raise ValueError("Package version is not supported SemVer")
    if not cargo == locked == frontend:
        raise ValueError("Cargo, lockfile and frontend package versions differ")
    if tag is not None and tag != "v" + cargo:
        raise ValueError("Release tag must exactly match the package version")
    if binary:
        version = subprocess.check_output(
            [str(binary.resolve()), "--version"], text=True
        ).strip()
        expected = f"wirehub {cargo} ("
        if not version.startswith(expected) or not version.endswith(")"):
            raise ValueError("Compiled binary version differs from packages")
        if build_id and version != f"wirehub {cargo} ({build_id})":
            raise ValueError("Compiled binary build ID differs from release commit")
    print("PASS: release version agreement " + cargo)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--build-id")
    args = parser.parse_args()
    check(args.tag, args.binary, args.build_id)
