#!/usr/bin/env python3
import unittest
import tempfile
from pathlib import Path
from check_android_crypto_build import EXPECTED, OVERRIDES, validate, check_discovered_configs


class AndroidBuildGuard(unittest.TestCase):
    def setUp(self):
        self.config = {'target': {'aarch64-linux-android': {'rustflags': EXPECTED}}}
        self.cfg = 'target_arch="aarch64"\ntarget_feature="neon"\n'

    def test_qualified_target_and_unrelated_environment(self):
        validate({'RUSTC_WRAPPER': 'sccache', 'CARGO_BUILD_JOBS': '1'}, self.config, self.cfg)

    def test_empty_and_nonempty_overrides_refused(self):
        for name in OVERRIDES:
            for value in ('', '--cfg chacha20_force_soft'):
                with self.subTest(name=name, value=value), self.assertRaises(ValueError):
                    validate({name: value}, self.config, self.cfg)

    def test_missing_neon_or_changed_config_refused(self):
        with self.assertRaises(ValueError):
            validate({}, self.config, 'target_arch="aarch64"')
        with self.assertRaises(ValueError):
            validate({}, {'target': {}}, self.cfg)

    def test_merged_flags_and_shadowed_root_refused(self):
        with tempfile.TemporaryDirectory() as name:
            base = Path(name)
            root, home = base / 'repo', base / 'cargo-home'
            (root / '.cargo').mkdir(parents=True)
            home.mkdir()
            (root / '.cargo/config.toml').write_text('[target.aarch64-linux-android]\nrustflags=["--cfg","chacha20_force_neon"]\n')
            check_discovered_configs(root, home)
            (home / 'config.toml').write_text('[net]\noffline=true\n')
            check_discovered_configs(root, home)
            for content in ('[target.aarch64-linux-android]\nrustflags=["--cfg","chacha20_force_soft"]',
                            '[target.\'cfg(target_arch="aarch64")\']\nrustflags=["--cfg","chacha20_force_soft"]',
                            'include="other.toml"'):
                (home / 'config.toml').write_text(content)
                with self.subTest(content=content), self.assertRaises(ValueError):
                    check_discovered_configs(root, home)
            (home / 'config.toml').unlink()
            (root / '.cargo/config').write_text('[net]\noffline=true\n')
            with self.assertRaises(ValueError):
                check_discovered_configs(root, home)


if __name__ == '__main__':
    unittest.main()
