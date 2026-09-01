#!/usr/bin/env python3
"""Verify A5 fixture isolation and absence of A6 TEST authority JNI exports."""

from __future__ import annotations

import pathlib
import subprocess
import sys


def fail(message: str) -> None:
    raise SystemExit(f"a6-product-scan: {message}")


def main() -> None:
    if len(sys.argv) != 2:
        fail("usage: scan_a6_product.py <product-so>")
    repository = pathlib.Path(__file__).resolve().parents[1]
    product = pathlib.Path(sys.argv[1]).resolve()
    subprocess.run(
        [sys.executable, str(repository / "scripts/scan_a5_product.py"), str(product)],
        cwd=repository,
        check=True,
    )
    symbols = subprocess.run(
        ["nm", "-D", "--defined-only", str(product)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    forbidden_jni = (
        "workerAcquire",
        "workerAuthenticate",
        "workerAuthenticatedSession",
        "workerReserve",
        "workerCommit",
        "workerReleaseReservation",
    )
    found = [name for name in forbidden_jni if name in symbols]
    if found:
        fail("forbidden authority JNI exports: " + ", ".join(found))
    print(f"A6 product authority isolation PASS: {product}")


if __name__ == "__main__":
    main()
