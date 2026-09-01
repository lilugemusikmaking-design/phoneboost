#!/usr/bin/env python3
"""C05 product secret and TEST-only crypto isolation scan."""

from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import sys


def fail(message: str) -> None:
    raise SystemExit(f"c05-product-scan: {message}")


def main() -> None:
    if len(sys.argv) != 4:
        fail("usage: scan_c05_product.py <worker-so> <phoneboostd> <phoneboostctl>")
    repository = pathlib.Path(__file__).resolve().parents[1]
    worker = pathlib.Path(sys.argv[1]).resolve()
    daemon = pathlib.Path(sys.argv[2]).resolve()
    cli = pathlib.Path(sys.argv[3]).resolve()
    subprocess.run(
        [
            str(repository / "scripts/scan_c04_product.py"),
            str(worker),
            str(daemon),
        ],
        cwd=repository,
        check=True,
    )
    lock = (repository / "Cargo.lock").read_text(encoding="utf-8")
    if 'name = "snow"\nversion = "0.9.6"' not in lock:
        fail("snow is not exactly locked to 0.9.6")

    forbidden: dict[str, bytes] = {
        "fixture RNG domain": b"PHONEBOOST-FIXTURE-RNG-V1\0",
        "fixture Linux static key": hashlib.sha256(
            b"PHONEBOOST-FIXTURE-STATIC-LINUX-V1\0"
        ).digest(),
        "fixture Android static key": hashlib.sha256(
            b"PHONEBOOST-FIXTURE-STATIC-ANDROID-V1\0"
        ).digest(),
        "fixture diagnostics feature": b"fixture-diagnostics",
        "C04 D0 probe": b"PHONEBOOST-C04-D0",
        "FTEST replay": b"FTEST-02",
    }
    for path in (repository / "protocol-fixtures/qr01a").glob("vector_*.json"):
        value = json.loads(path.read_text(encoding="utf-8"))
        for role in ("linux", "android"):
            seed = value.get(f"rng_seed_{role}_hex")
            if seed:
                forbidden[f"{path.name}:{role} RNG seed"] = bytes.fromhex(seed)

    for artifact in (worker, daemon, cli):
        data = artifact.read_bytes()
        found = [label for label, pattern in forbidden.items() if pattern in data]
        if found:
            fail(f"{artifact.name} contains forbidden material: {', '.join(found)}")
    print("C05 product secret isolation PASS")


if __name__ == "__main__":
    main()
