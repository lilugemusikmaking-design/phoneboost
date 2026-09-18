#!/usr/bin/env python3
"""Offline structural validation for Maestro YAML when no device is present."""
from pathlib import Path
import sys
import yaml


def main() -> int:
    files = sorted(Path(".maestro").glob("*.yaml"))
    if not files:
        raise RuntimeError("no Maestro flows found")
    for path in files:
        documents = list(yaml.safe_load_all(path.read_text()))
        if len(documents) != 2 or not isinstance(documents[0], dict) or not isinstance(documents[1], list):
            raise RuntimeError(f"{path}: expected config document then command list")
        if documents[0].get("appId") != "org.phoneboost.app":
            raise RuntimeError(f"{path}: unexpected appId")
        if not documents[1] or not all(isinstance(command, dict) for command in documents[1]):
            raise RuntimeError(f"{path}: commands must be non-empty mappings")
        print(f"{path}: YAML structure PASS ({len(documents[1])} commands)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, yaml.YAMLError) as error:
        print(f"Maestro flow validation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
