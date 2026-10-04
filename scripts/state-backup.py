#!/usr/bin/env python3
"""Unix-only offline paired backup/restore. Requires Python 3.11+ and OpenSSL X25519."""

import argparse
import contextlib
import datetime
import fcntl
import hashlib
import json
import os
import shutil
import sqlite3
import stat
import subprocess
from pathlib import Path

DB_NAME, KEY_NAME = "wirehub.sqlite3", "wirehub.key"


def secure_read(path, key=False):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError("Backup inputs must be regular files with one hard link")
        if key and (stat.S_IMODE(metadata.st_mode) != 0o600 or metadata.st_size != 32):
            raise ValueError("Hub key must contain exactly 32 bytes with mode 0600")
        return source.read()


@contextlib.contextmanager
def instance_lock(database):
    if database.is_symlink():
        raise ValueError("Use the canonical regular database path")
    database = database.resolve(strict=True)
    metadata = database.stat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
        raise ValueError("Database must be a regular file with one hard link")
    descriptor = os.open(
        str(database) + ".wirehub.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600
    )
    with os.fdopen(descriptor, "r+b") as lock:
        metadata = os.fstat(lock.fileno())
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_nlink != 1
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            raise ValueError(
                "Instance lock must be a regular 0600 file with one hard link"
            )
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError(
                "Stop WireHub before backing up this identity pair"
            ) from None
        yield database


def public_identity(private):
    # RFC 8410 PKCS#8 X25519 DER; key bytes stay on stdin, outside argv/logs/files.
    encoded = bytes.fromhex("302e020100300506032b656e04220420") + private
    result = subprocess.run(
        ["openssl", "pkey", "-inform", "DER", "-pubout", "-outform", "DER"],
        input=encoded,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    prefix = bytes.fromhex("302a300506032b656e032100")
    if (
        result.returncode
        or not result.stdout.startswith(prefix)
        or len(result.stdout) != 44
    ):
        raise ValueError("OpenSSL with X25519 support is required to validate the pair")
    return result.stdout[-32:]


def inspect(database, private):
    with sqlite3.connect(
        database.resolve().as_uri() + "?mode=ro", uri=True
    ) as connection:
        if connection.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise ValueError("SQLite integrity check failed")
        version = connection.execute("PRAGMA user_version").fetchone()[0]
        if version not in (3, 4, 5):
            raise ValueError(
                "Backup supports canonical schema 3, 4 or 5 identity pairs"
            )
        identities = connection.execute(
            "SELECT public_key FROM hub_identity WHERE id=1"
        ).fetchall()
        if len(identities) != 1 or identities[0][0] != public_identity(private):
            raise ValueError("Database and hub key identities do not match")
        revision = (
            connection.execute(
                "SELECT revision FROM config_state WHERE id=1"
            ).fetchone()[0]
            if version == 5
            else None
        )
        return {
            "schema_version": version,
            "config_revision": revision,
            "hub_public_key": identities[0][0].hex(),
        }


def sync_file(path):
    with path.open("rb") as source:
        os.fsync(source.fileno())


def sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def write_private(path, contents):
    with path.open("xb") as output:
        os.fchmod(output.fileno(), 0o600)
        output.write(contents)
        output.flush()
        os.fsync(output.fileno())


def backup(database, key, output):
    with instance_lock(database) as database:
        private = secure_read(key, key=True)
        inspect(database, private)  # Validate before creating any output.
        output.mkdir(mode=0o700)  # Exclusive new destination; never overwrite a backup.
        try:
            with (
                sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as source,
                sqlite3.connect(output / DB_NAME) as destination,
            ):
                source.backup(destination)
            os.chmod(output / DB_NAME, 0o600)
            sync_file(output / DB_NAME)
            write_private(output / KEY_NAME, private)
            metadata = {
                "format": 1,
                "created_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                **inspect(output / DB_NAME, private),
            }
            metadata["sha256"] = {
                name: hashlib.sha256(
                    secure_read(output / name, key=name == KEY_NAME)
                ).hexdigest()
                for name in (DB_NAME, KEY_NAME)
            }
            # A backup is complete only when this final durable manifest exists.
            write_private(
                output / "manifest.json",
                (json.dumps(metadata, indent=2) + "\n").encode(),
            )
            sync_directory(output)
            sync_directory(output.parent)
        except Exception:
            shutil.rmtree(
                output
            )  # Only the exclusively created output belongs to this operation.
            raise


def verify(directory):
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError("Backup must be a regular directory")
    manifest = json.loads(secure_read(directory / "manifest.json"))
    if manifest.get("format") != 1 or set(manifest.get("sha256", {})) != {
        DB_NAME,
        KEY_NAME,
    }:
        raise ValueError("Invalid backup manifest")
    for name in (DB_NAME, KEY_NAME):
        if (
            hashlib.sha256(
                secure_read(directory / name, key=name == KEY_NAME)
            ).hexdigest()
            != manifest["sha256"][name]
        ):
            raise ValueError("Backup checksum mismatch")
    actual = inspect(directory / DB_NAME, secure_read(directory / KEY_NAME, key=True))
    if any(manifest.get(name) != value for name, value in actual.items()):
        raise ValueError("Backup identity/schema metadata mismatch")
    return manifest


def restore(source, destination):
    verify(source)
    destination.mkdir(
        mode=0o700
    )  # Restore to a new directory; switch service paths only after verification.
    try:
        for name in (DB_NAME, KEY_NAME):
            write_private(
                destination / name, secure_read(source / name, key=name == KEY_NAME)
            )
        inspect(destination / DB_NAME, secure_read(destination / KEY_NAME, key=True))
        sync_directory(destination)
        sync_directory(destination.parent)
    except Exception:
        shutil.rmtree(destination)
        raise


if __name__ == "__main__":
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("backup")
    create.add_argument("--database", type=Path, required=True)
    create.add_argument("--key", type=Path, required=True)
    create.add_argument("--output", type=Path, required=True)
    check = commands.add_parser("verify")
    check.add_argument("directory", type=Path)
    recover = commands.add_parser("restore")
    recover.add_argument("--source", type=Path, required=True)
    recover.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "backup":
            backup(args.database, args.key, args.output)
        elif args.command == "verify":
            verify(args.directory)
        else:
            restore(args.source, args.output)
        print("PASS: paired state " + args.command)
    except (OSError, ValueError, sqlite3.Error, subprocess.SubprocessError) as error:
        parser.exit(1, "FAIL: " + str(error) + "\n")
