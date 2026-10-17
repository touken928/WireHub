#!/usr/bin/env python3
"""Isolated CLI/API/shutdown/paired backup/migration/restore/rollback acceptance."""

import argparse
import hashlib
import importlib.util
import json
import os
import secrets
import shutil
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

from support import HTTP, ROOT, Hub, check, request, run_cli, write_report

spec = importlib.util.spec_from_file_location(
    "state_backup", ROOT / "scripts/state-backup.py"
)
backup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(backup)


def snapshot(url, token):
    state = {
        path: request(url + "/api/" + path, token)
        for path in ("setup", "groups", "peers", "forwards")
    }
    # Legacy releases express both entry and target with target_port.
    for forward in state["forwards"]:
        forward.setdefault("listen_port", forward["target_port"])
    return state


def expect_failure(operation, message):
    try:
        operation()
    except (ValueError, OSError, sqlite3.Error):
        return
    raise RuntimeError(message)


def run(binary, rollback_binary, output):
    os.umask(0o077)
    binary = binary.resolve()
    subprocess.run(
        ["python3", str(ROOT / "scripts/check-release.py"), "--binary", str(binary)],
        check=True,
    )
    checks = []
    with tempfile.TemporaryDirectory(prefix="wirehub-operations-") as temporary:
        directory = Path(temporary)
        readonly = directory / "readonly"
        readonly.mkdir()
        environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith("WIREHUB_")
        }
        for arguments in (["--version"], ["--help"], ["export-openapi"]):
            subprocess.run(
                [str(binary), *arguments],
                cwd=readonly,
                env=environment,
                stdout=subprocess.DEVNULL,
                check=True,
            )
        check(list(readonly.iterdir()) == [], "Metadata CLI touched state")
        checks.append("version/help/OpenAPI do not open state")
        hub = Hub(binary, directory / "source", http_bind="0.0.0.0")
        try:
            checks.append("non-loopback HTTP starts without an additional opt-in")
            unauth = request(hub.url + "/api/policy", None, "PUT", {}, expected=401)
            check(
                unauth["code"] == "unauthorized",
                "Authentication did not precede JSON validation",
            )
            invalid = request(
                hub.url + "/api/groups", hub.token, "POST", {}, expected=422
            )
            check(
                invalid["code"] == "invalid_request", "JSON rejection contract differs"
            )
            missing = request(hub.url + "/api/not-a-route", hub.token, expected=404)
            check(missing["code"] == "not_found", "API fallback error differs")
            for status, content, content_type, code in (
                (400, b"{", "application/json", "invalid_request"),
                (415, b"{}", "text/plain", "unsupported_media_type"),
                (
                    413,
                    b" " * (2 * 1024 * 1024 + 1),
                    "application/json",
                    "payload_too_large",
                ),
            ):
                req = urllib.request.Request(
                    hub.url + "/api/groups",
                    data=content,
                    headers={
                        "Authorization": "Bearer " + hub.token,
                        "Content-Type": content_type,
                    },
                    method="POST",
                )
                try:
                    HTTP.open(req, timeout=8)
                except urllib.error.HTTPError as error:
                    check(
                        error.code == status
                        and error.headers.get("Cache-Control") == "no-store",
                        "Extractor status/cache contract differs",
                    )
                    body = json.loads(error.read())
                    check(body["code"] == code, "Extractor structured code differs")
                else:
                    raise RuntimeError("Invalid API request unexpectedly succeeded")
            checks.append(
                "authentication precedes extraction; structured 400/401/413/415/422/404 with no-store"
            )
            request(
                hub.url + "/api/setup",
                hub.token,
                "POST",
                {
                    "subnet": "172.27.94.0/24",
                    "endpoint": "127.0.0.1:51820",
                    "persistent_keepalive": 25,
                },
            )
            group = request(
                hub.url + "/api/groups",
                hub.token,
                "POST",
                {"name": "backup-group"},
                expected=201,
            )
            peer = request(
                hub.url + "/api/peers",
                hub.token,
                "POST",
                {"name": "backup-peer", "group_id": group["id"]},
                expected=201,
            )["peer"]
            request(
                hub.url + "/api/forwards",
                hub.token,
                "POST",
                {
                    "name": "backup-service",
                    "protocol": "tcp",
                    "target_port": 8080,
                    "target_peer_id": peer["id"],
                    "allowed_group_ids": [group["id"]],
                },
                expected=201,
            )
            before = snapshot(hub.url, hub.token)
            expect_failure(
                lambda: backup.backup(
                    hub.directory / "wirehub.sqlite3",
                    hub.directory / "wirehub.key",
                    directory / "unsafe-live-backup",
                ),
                "Backup did not honor the live Rust instance lock",
            )
            check(
                not (directory / "unsafe-live-backup").exists(),
                "Live backup created output",
            )
            checks.append("Python flock excludes the running Rust service")
        finally:
            shutdown_seconds = hub.stop()
        checks.append("SIGTERM drains and exits zero within ten seconds")
        # A crash between link(2) and unlink(2) may leave the atomic key
        # publication temporary name behind. A restarted service must retain
        # the original identity, and backup must accept that key's link count.
        key = hub.directory / "wirehub.key"
        residue = hub.directory / (".key." + secrets.token_hex(8) + ".tmp")
        os.link(key, residue)
        linked_hub = Hub(binary, hub.directory)
        try:
            check(
                snapshot(linked_hub.url, linked_hub.token) == before,
                "Restart with a residual key hard link changed service state",
            )
        finally:
            linked_hub.stop()
        checks.append(
            "restart preserves identity with residual key publication hard link"
        )
        current_archive = directory / "schema-6-backup"
        backup.backup(hub.directory / "wirehub.sqlite3", key, current_archive)
        current_metadata = backup.verify(current_archive)
        check(
            current_metadata["schema_version"] == 6,
            "Current backup lost schema version",
        )
        current_restored = directory / "schema-6-restored"
        backup.restore(current_archive, current_restored)
        current_hub = Hub(binary, current_restored)
        try:
            check(
                snapshot(current_hub.url, current_hub.token) == before,
                "Schema 6 backup restore changed inventory",
            )
        finally:
            current_hub.stop()
        checks.append(
            "schema 6 paired backup verifies and restores complete service state"
        )
        # Canonical schema 5 pre-upgrade fixture, using only the temporary pair.
        with sqlite3.connect(hub.directory / "wirehub.sqlite3") as connection:
            connection.executescript(
                "ALTER TABLE forwards RENAME TO forwards_new; CREATE TABLE forwards(id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL,target_peer_id TEXT NOT NULL REFERENCES peers(id) ON DELETE CASCADE,target_port INTEGER NOT NULL,allowed TEXT NOT NULL,UNIQUE(protocol,target_port)); INSERT INTO forwards(id,name,protocol,target_peer_id,target_port,allowed) SELECT id,name,protocol,target_peer_id,target_port,allowed FROM forwards_new; DROP TABLE forwards_new; "
            )
            connection.execute("PRAGMA user_version=5")
            connection.commit()
        archive = directory / "before-upgrade"
        backup.backup(
            hub.directory / "wirehub.sqlite3", hub.directory / "wirehub.key", archive
        )
        metadata = backup.verify(archive)
        archive_hash = {
            name: hashlib.sha256((archive / name).read_bytes()).hexdigest()
            for name in ("wirehub.sqlite3", "wirehub.key", "manifest.json")
        }
        checks.append(
            "canonical schema 5 paired backup with integrity/identity validation despite key hard link"
        )
        # The key exception is intentionally narrow: database hard links,
        # key symlinks, and keys with unsafe permissions/size remain rejected.
        database_alias = directory / "database-hardlink.sqlite3"
        os.link(hub.directory / "wirehub.sqlite3", database_alias)
        expect_failure(
            lambda: backup.backup(
                database_alias, key, directory / "database-hardlink-backup"
            ),
            "Database hard link passed instance locking",
        )
        key_symlink = directory / "key-symlink"
        key_symlink.symlink_to(key)
        expect_failure(
            lambda: backup.secure_read(key_symlink, key=True),
            "Hub key symlink was accepted",
        )
        insecure_key = directory / "insecure.key"
        shutil.copyfile(key, insecure_key)
        insecure_key.chmod(0o644)
        expect_failure(
            lambda: backup.secure_read(insecure_key, key=True),
            "Insecurely permissioned hub key was accepted",
        )
        check(
            key.stat().st_nlink == 2,
            "Residual publication hard link did not remain during backup checks",
        )
        checks.append(
            "database hard links, key symlinks and insecure keys remain rejected"
        )
        upgraded = directory / "upgraded"
        backup.restore(archive, upgraded)
        migrated = Hub(binary, upgraded)
        try:
            check(
                snapshot(migrated.url, migrated.token) == before,
                "Migration lost saved settings, peers, groups or forwards",
            )
            with sqlite3.connect(upgraded / "wirehub.sqlite3") as connection:
                check(
                    connection.execute("PRAGMA user_version").fetchone()[0] == 6,
                    "Upgrade did not install schema 6",
                )
            request(
                migrated.url + "/api/settings",
                migrated.token,
                "PUT",
                {"endpoint": "after-upgrade.example:51820", "persistent_keepalive": 15},
            )
        finally:
            migrated.stop()
        checks.append("schema 5→6 migration preserves complete inventory and identity")
        restored = directory / "restored"
        backup.restore(archive, restored)
        rolled_back = False
        if rollback_binary:
            rollback_binary = rollback_binary.resolve()
            # Older schema 5 binary must reject schema 6 without mutating it.
            digest = hashlib.sha256(
                (upgraded / "wirehub.sqlite3").read_bytes()
            ).digest()
            environment = dict(
                hub.environment,
                WIREHUB_DB=str(upgraded / "wirehub.sqlite3"),
                WIREHUB_HUB_KEY=str(upgraded / "wirehub.key"),
            )
            rejected = subprocess.run(
                [str(rollback_binary)],
                cwd=upgraded,
                env=environment,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=10,
            )
            check(rejected.returncode != 0, "Old binary accepted the upgraded schema")
            check(
                hashlib.sha256((upgraded / "wirehub.sqlite3").read_bytes()).digest()
                == digest,
                "Old binary mutated the upgraded database",
            )
            old = Hub(rollback_binary, restored)
            try:
                check(
                    snapshot(old.url, old.token) == before,
                    "Pre-upgrade pair did not restore old binary behavior",
                )
            finally:
                old.stop(False)
            rolled_back = True
            checks.append(
                "real pre-upgrade binary rejects schema 6 and runs restored schema 5 pair"
            )
        else:
            with sqlite3.connect(restored / "wirehub.sqlite3") as connection:
                check(
                    connection.execute("PRAGMA user_version").fetchone()[0] == 5,
                    "Rollback restore changed the original schema",
                )
            checks.append(
                "rollback pair restores original schema 5; old binary execution not requested"
            )
        check(
            archive_hash
            == {
                name: hashlib.sha256((archive / name).read_bytes()).hexdigest()
                for name in archive_hash
            },
            "Migration or rollback changed the immutable archive",
        )
        tampered = directory / "wrong-key"
        shutil.copytree(archive, tampered)
        (tampered / "wirehub.key").write_bytes(secrets.token_bytes(32))
        manifest = json.loads((tampered / "manifest.json").read_text())
        manifest["sha256"]["wirehub.key"] = hashlib.sha256(
            (tampered / "wirehub.key").read_bytes()
        ).hexdigest()
        (tampered / "manifest.json").write_text(json.dumps(manifest))
        expect_failure(
            lambda: backup.restore(tampered, directory / "bad-restore"),
            "Mismatched identity pair passed checksums",
        )
        check(
            not (directory / "bad-restore").exists(), "Mismatch created restore output"
        )
        expect_failure(
            lambda: backup.restore(archive, restored),
            "Restore overwrote an existing directory",
        )
        expect_failure(
            lambda: backup.backup(
                hub.directory / "wirehub.sqlite3",
                hub.directory / "wirehub.key",
                archive,
            ),
            "Backup overwrote an archive",
        )
        checks.append(
            "mismatched pair rejected before restore; existing destinations never overwritten"
        )
        version = subprocess.check_output([str(binary), "--version"], text=True).strip()
        report = {
            "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "binary": version,
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "checks": checks,
            "shutdown_seconds": shutdown_seconds,
            "backup_schema": metadata["schema_version"],
            "rollback_binary_executed": rolled_back,
        }
        write_report(output, report)
        print("PASS: operations acceptance; report " + str(output))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/wirehub")
    parser.add_argument("--rollback-binary", type=Path)
    parser.add_argument(
        "--output", type=Path, default=Path("/tmp/wirehub-operations.json")
    )
    args = parser.parse_args()
    run(args.binary, args.rollback_binary, args.output)


if __name__ == "__main__":
    run_cli(main, "Operations check failed; temporary credentials withheld")
