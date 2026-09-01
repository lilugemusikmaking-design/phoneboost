#!/usr/bin/env python3
import hashlib
import json
import pathlib
import sys


def fail(message: str) -> None:
    raise SystemExit(f"a5-product-scan: {message}")


def main() -> None:
    if len(sys.argv) != 2:
        fail("usage: scan_a5_product.py <product-so>")
    product = pathlib.Path(sys.argv[1])
    data = product.read_bytes()
    forbidden = {
        "fixture RNG domain": b"PHONEBOOST-FIXTURE-RNG-V1\0",
        "static Linux fixture key": hashlib.sha256(
            b"PHONEBOOST-FIXTURE-STATIC-LINUX-V1\0"
        ).digest(),
        "static Android fixture key": hashlib.sha256(
            b"PHONEBOOST-FIXTURE-STATIC-ANDROID-V1\0"
        ).digest(),
        "FTEST replay package": b"org.phoneboost.ftest02.replay",
        "FTEST replay API": b"FTEST-02",
        "fixture generator": b"pb-fixture-gen",
        "fixture checker": b"pb-fixture-check",
    }
    fixture_root = pathlib.Path("protocol-fixtures/qr01a")
    for path in fixture_root.glob("vector_*.json"):
        value = json.loads(path.read_text(encoding="utf-8"))
        for role in ("linux", "android"):
            key = f"rng_seed_{role}_hex"
            if key in value:
                forbidden[f"{path.name}:{key}"] = bytes.fromhex(value[key])
    found = [label for label, pattern in forbidden.items() if pattern in data]
    if found:
        fail("forbidden TEST-ONLY material linked: " + ", ".join(found))
    print(f"A5 product isolation PASS: {product}")


if __name__ == "__main__":
    main()
