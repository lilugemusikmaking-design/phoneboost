#!/usr/bin/env python3
"""Fail before packaging if external flags silently replace the qualified cfg."""
import os
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
EXPECTED = ['--cfg', 'chacha20_force_neon']
OVERRIDES = ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS',
             'CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS')


def check_discovered_configs(root, cargo_home):
    """Reject additional flag sources rather than approximating Cargo's merge.

    Packaging runs from root and does not pass --config. Other config settings
    (registry/cache/network) are allowed and never printed, including secrets.
    """
    canonical = (root / '.cargo/config.toml').resolve()
    directories = [p / '.cargo' for p in (root, *root.parents)] + [cargo_home]
    seen = set()
    for directory in directories:
        for name in ('config', 'config.toml'):
            path = (directory / name).resolve()
            if path in seen or not path.exists():
                continue
            seen.add(path)
            data = tomllib.loads(path.read_text())
            if 'include' in data:
                raise ValueError('Included Cargo config is not qualified for Android packaging: ' + str(path))
            if path == canonical:
                continue
            shadows_root = path == (root / '.cargo/config').resolve()
            has_flags = ('rustflags' in data.get('build', {})
                         or any('rustflags' in target for target in data.get('target', {}).values()))
            if shadows_root or has_flags:
                raise ValueError('Additional Cargo flag configuration is not qualified: ' + str(path))


def validate(environment, configuration, target_cfg):
    # Even an explicitly empty global RUSTFLAGS can override Cargo target config.
    present = [name for name in OVERRIDES if name in environment]
    if present:
        raise ValueError('Android crypto build flags overridden: ' + ', '.join(present)
                         + '; unset them for the qualified packaging path')
    if configuration.get('target', {}).get('aarch64-linux-android', {}).get('rustflags') != EXPECTED:
        raise ValueError('Android target flags differ from the qualified NEON build')
    if 'target_feature="neon"' not in target_cfg.splitlines():
        raise ValueError('Android target does not advertise NEON; refusing silent soft fallback')


def main():
    cargo_home = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo'))).absolute()
    check_discovered_configs(ROOT, cargo_home)
    configuration = tomllib.loads((ROOT / '.cargo/config.toml').read_text())
    cfg = subprocess.check_output(['rustc', '--print', 'cfg', '--target', 'aarch64-linux-android'], text=True)
    validate(os.environ, configuration, cfg)
    print('Android crypto build: aarch64 NEON, pinned backend, no external flag override')


if __name__ == '__main__':
    main()
