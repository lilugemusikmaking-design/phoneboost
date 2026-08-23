#!/usr/bin/env python3
"""C04 product isolation and permission scan."""

from __future__ import annotations

import pathlib
import subprocess
import sys


def fail(message: str) -> None:
    raise SystemExit(f"c04-product-scan: {message}")


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: scan_c04_product.py <worker-so> <phoneboostd>")
    repository = pathlib.Path(__file__).resolve().parents[1]
    worker_so = pathlib.Path(sys.argv[1]).resolve()
    host = pathlib.Path(sys.argv[2]).resolve()
    subprocess.run(
        [str(repository / "scripts/scan_a6_product.py"), str(worker_so)],
        cwd=repository,
        check=True,
    )
    manifest = (repository / "android/app/src/main/AndroidManifest.xml").read_text(
        encoding="utf-8"
    )
    required = (
        "android.permission.INTERNET",
        "android.permission.ACCESS_LOCAL_NETWORK",
    )
    missing = [permission for permission in required if permission not in manifest]
    if missing:
        fail("missing C04 permissions: " + ", ".join(missing))
    forbidden = (
        "android.permission.ACCESS_FINE_LOCATION",
        "android.permission.ACCESS_COARSE_LOCATION",
        "android.permission.NEARBY_WIFI_DEVICES",
        "android.permission.ACCESS_WIFI_STATE",
        "android.permission.CHANGE_WIFI_STATE",
    )
    found = [permission for permission in forbidden if permission in manifest]
    if found:
        fail("unnecessary discovery/Wi-Fi permissions: " + ", ".join(found))
    host_bytes = host.read_bytes()
    if b"PHONEBOOST-C04-D0" in host_bytes:
        fail("TEST-ONLY D0 probe linked into product phoneboostd")
    print("C04 product isolation PASS")


if __name__ == "__main__":
    main()
