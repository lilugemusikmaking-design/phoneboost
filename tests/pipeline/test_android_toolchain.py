"""Isolated installer checks; synthetic CLI payloads are not Android build proof."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[2]
VERSION = "1.0.16500706"


class AndroidToolchain(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="pb-android-pin-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "scripts").mkdir()
        (self.root / ".pipeline").mkdir()
        shutil.copy2(ROOT / "scripts/pipeline-android-setup", self.root / "scripts")
        self.calls = self.root / "calls.jsonl"
        self.assets = []
        for name, directory in [("jdk", "jdk-test"), ("gradle", "gradle-test")]:
            archive = self.root / (name + ".zip")
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr(directory + "/bin/fixture", "#!/bin/sh\nexit 0\n")
            self.add_asset(name, archive, "zip")
        self.payload = self.root / "payload"
        self.payload.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ["PB_ANDROID_TEST_CALLS"], "a") as out:
    out.write(json.dumps(args) + "\\n")
assert args[0] == "--no-metrics"
if args[1:] == ["--version"]:
    print(os.environ.get("PB_ANDROID_TEST_VERSION", "1.0.16500706"))
else:
    assert args[1].startswith("--sdk=") and args[2:4] == ["sdk", "install"]
    sdk = pathlib.Path(args[1].split("=", 1)[1])
    for package in args[4:]:
        name, revision = package.rsplit("@", 1)
        path = sdk / name / "source.properties"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("Pkg.Revision = " + revision + "\\n")
''')
        self.add_asset("android-cli", self.payload, "binary")
        self.lock = dict(assets=self.assets, jdk_directory="jdk-test", gradle_version="test",
                         android_cli_version=VERSION,
                         sdk_revisions={"build-tools;36.0.0": "36.0.0"})
        self.save_lock()

    def add_asset(self, name, path, kind):
        self.assets.append(dict(name=name, url=path.as_uri(), format=kind,
                                sha256=hashlib.sha256(path.read_bytes()).hexdigest()))

    def save_lock(self):
        (self.root / ".pipeline/android-toolchain.json").write_text(json.dumps(self.lock))

    def run_setup(self, version=VERSION):
        env = dict(os.environ, PB_ANDROID_TEST_CALLS=str(self.calls),
                   PB_ANDROID_TEST_VERSION=version)
        return subprocess.run(["python3", str(self.root / "scripts/pipeline-android-setup")],
                              cwd=self.root, env=env, text=True, capture_output=True, timeout=10)

    def test_verified_payload_runs_directly_with_exact_package_revisions(self):
        # A legacy launcher must never be used, even when left in the cache.
        old = self.root / ".tooling/pipeline-android/cmdline-tools/cmdline-tools/bin/android"
        old.parent.mkdir(parents=True)
        old.write_text("#!/bin/sh\nexit 91\n")
        old.chmod(0o755)
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = [json.loads(line) for line in self.calls.read_text().splitlines()]
        self.assertEqual(calls[0], ["--no-metrics", "--version"])
        self.assertEqual(calls[1][2:], ["sdk", "install", "build-tools/36.0.0@36.0.0"])
        env = (self.root / ".tooling/pipeline-android/env.sh").read_text()
        self.assertIn("/android-cli/bin", env)
        self.assertNotIn("cmdline-tools", env)

    def test_checksum_mismatch_never_executes_cli(self):
        self.payload.write_bytes(self.payload.read_bytes() + b"# changed\n")
        result = self.run_setup()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Checksum mismatch: android-cli", result.stderr)
        self.assertFalse(self.calls.exists())

    def test_version_drift_still_blocks_sdk_installation(self):
        result = self.run_setup("1.1.16919734")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Android CLI version drift: 1.1.16919734", result.stderr)
        self.assertEqual(len(self.calls.read_text().splitlines()), 1)
        self.assertFalse((self.root / ".tooling/pipeline-android/env.sh").exists())

    def test_cached_executable_is_rechecked_against_verified_archive(self):
        self.assertEqual(self.run_setup().returncode, 0)
        binary = self.root / ".tooling/pipeline-android/android-cli/bin/android"
        binary.write_text("#!/bin/sh\nexit 92\n")
        # A warm cache must recover the pinned bytes, not trust a stale stamp.
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(binary.read_bytes(), self.payload.read_bytes())


if __name__ == "__main__":
    unittest.main()
