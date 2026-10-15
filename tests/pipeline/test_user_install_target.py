"""Isolated packaging regression tests; synthetic payloads are not runtime proof."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


REPOSITORY = Path(__file__).resolve().parents[2]
BINARIES = ("phoneboostd", "phoneboostctl", "phoneboost-web-bridge")


class UserInstallTarget(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="pb-install-target-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.checkout = self.root / "checkout"
        self.prefix = self.root / "installed"
        for relative in (
            "scripts/install_phoneboost_control_center_user.sh",
            "scripts/run_phoneboost_control_center.sh",
            "packaging/linux/phoneboost-control-center",
            "packaging/linux/org.phoneboost.ControlCenter.desktop.in",
        ):
            destination = self.checkout / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPOSITORY / relative, destination)
        frontend = self.checkout / "frontend/build"
        frontend.mkdir(parents=True)
        (frontend / "index.html").write_text("<!doctype html><title>fixture</title>")

    def payload(self, directory, marker):
        release = directory / "release"
        release.mkdir(parents=True, exist_ok=True)
        for name in BINARIES:
            path = release / name
            path.write_text(f"#!/bin/sh\n# {marker}: {name}\nexit 99\n")
            path.chmod(0o755)

    def install(self, target=None):
        environment = dict(os.environ)
        environment.pop("CARGO_TARGET_DIR", None)
        environment["PHONEBOOST_INSTALL_PREFIX"] = str(self.prefix)
        if target is not None:
            environment["CARGO_TARGET_DIR"] = str(target)
        return subprocess.run(
            ["bash", str(self.checkout / "scripts/install_phoneboost_control_center_user.sh"),
             "--no-build"], cwd=self.root, env=environment,
            capture_output=True, text=True, timeout=10,
        )

    def assert_installed(self, target):
        for name in BINARIES:
            self.assertEqual(
                (self.prefix / "share/phoneboost/libexec" / name).read_bytes(),
                (target / "release" / name).read_bytes(),
            )

    def test_default_target_from_another_working_directory(self):
        target = self.checkout / "target"
        self.payload(target, "default")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_installed(target)

    def test_absolute_shared_target_wins_over_stale_default(self):
        self.payload(self.checkout / "target", "stale")
        target = self.root / "shared cache"
        self.payload(target, "current")
        result = self.install(target)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_installed(target)

    def test_relative_target_is_relative_to_checkout(self):
        target = self.checkout / "custom-target"
        self.payload(target, "relative")
        result = self.install("custom-target")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_installed(target)

    def test_missing_selected_artifact_preserves_existing_install(self):
        self.payload(self.checkout / "target", "old")
        self.assertEqual(self.install().returncode, 0)
        previous = {p: p.read_bytes() for p in self.prefix.rglob("*") if p.is_file()}
        target = self.root / "incomplete-target"
        self.payload(target, "new")
        (target / "release/phoneboost-web-bridge").unlink()
        result = self.install(target)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("production binary is unavailable", result.stderr)
        self.assertEqual(previous, {p: p.read_bytes() for p in self.prefix.rglob("*") if p.is_file()})
        self.assertFalse(list((self.prefix / "share").glob(".phoneboost-*")))


if __name__ == "__main__":
    unittest.main()
