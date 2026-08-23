#!/usr/bin/env python3
"""Verify that imported A1-A3 zones match the authorized frozen archive."""

from __future__ import annotations

import hashlib
import json
import sys
import zipfile
from pathlib import Path


EXPECTED_ARCHIVE_SHA256 = (
    "ae2a116e7a1b94ba069f53585b9efdc68258470ded73641f06db1c51b2fffbc1"
)
EXPECTED_MANIFEST_SHA256 = (
    "98f6741cf1db27c1aee946ae2edc67a955682cf2e51110e58d201e2ad5689762"
)
ARCHIVE_PREFIX = "phoneboost-fixture-bootstrap/"
MANIFEST_MEMBER = f"{ARCHIVE_PREFIX}protocol-fixtures/MANIFEST.json"
FROZEN_ROOTS = (
    "crates/pb-types",
    "crates/pb-secure",
    "crates/pb-pbmux",
    "protocol-fixtures",
    "tools/pb-fixture-gen",
    "tools/pb-fixture-check",
    "tests/linux-replay",
)


class VerificationError(RuntimeError):
    """Raised when the imported baseline differs from the archive."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def is_frozen(relative: str) -> bool:
    return any(
        relative == root or relative.startswith(f"{root}/") for root in FROZEN_ROOTS
    )


def archive_inventory(archive: Path) -> dict[str, tuple[int, str]]:
    if sha256_file(archive) != EXPECTED_ARCHIVE_SHA256:
        raise VerificationError("frozen archive SHA-256 mismatch")

    inventory: dict[str, tuple[int, str]] = {}
    with zipfile.ZipFile(archive) as bundle:
        corrupt = bundle.testzip()
        if corrupt is not None:
            raise VerificationError(f"corrupt archive member: {corrupt}")
        if sha256_bytes(bundle.read(MANIFEST_MEMBER)) != EXPECTED_MANIFEST_SHA256:
            raise VerificationError("frozen MANIFEST SHA-256 mismatch")
        for info in bundle.infolist():
            if info.is_dir() or not info.filename.startswith(ARCHIVE_PREFIX):
                continue
            relative = info.filename[len(ARCHIVE_PREFIX) :]
            if is_frozen(relative):
                data = bundle.read(info)
                inventory[relative] = (len(data), sha256_bytes(data))
    return inventory


def imported_inventory(repository: Path) -> dict[str, tuple[int, str]]:
    inventory: dict[str, tuple[int, str]] = {}
    for root in FROZEN_ROOTS:
        frozen_root = repository / root
        if not frozen_root.is_dir():
            raise VerificationError(f"missing frozen root: {root}")
        for path in frozen_root.rglob("*"):
            relative = path.relative_to(repository).as_posix()
            if path.is_symlink():
                raise VerificationError(f"symlink in frozen zone: {relative}")
            if path.is_file():
                inventory[relative] = (path.stat().st_size, sha256_file(path))
    return inventory


def verify(repository: Path) -> dict[str, object]:
    archive = repository / "inputs" / "phoneboost_fixture_bootstrap_v1_1.zip"
    expected = archive_inventory(archive)
    actual = imported_inventory(repository)

    missing = sorted(set(expected) - set(actual))
    extra = sorted(set(actual) - set(expected))
    changed = sorted(
        path for path in set(expected) & set(actual) if expected[path] != actual[path]
    )
    if missing or extra or changed:
        raise VerificationError(
            f"baseline differs: missing={missing}, extra={extra}, changed={changed}"
        )

    return {
        "schema": "phoneboost-frozen-baseline-verification/1",
        "status": "PASS",
        "archive_sha256": EXPECTED_ARCHIVE_SHA256,
        "manifest_sha256": EXPECTED_MANIFEST_SHA256,
        "frozen_roots": list(FROZEN_ROOTS),
        "files_verified": len(actual),
    }


def main() -> int:
    repository = Path(__file__).resolve().parents[1]
    try:
        result = verify(repository)
    except (OSError, KeyError, RuntimeError, zipfile.BadZipFile) as error:
        print(json.dumps({"status": "FAIL", "detail": str(error)}, sort_keys=True))
        return 1
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
