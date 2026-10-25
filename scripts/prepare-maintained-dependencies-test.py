#!/usr/bin/env python3
"""Exercise source integrity and failure atomicity of the maintained dependency preparation."""
import contextlib
import importlib.util
import io
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("prepare_base", ROOT / "scripts/prepare-maintained-dependencies.py")
PREPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREPARE)


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for relative in ["patches", "vendor/markraft/scripts", ".build/dependencies"]:
            (self.root / relative).mkdir(parents=True)
        relative = "vendor/markraft/scripts/prepare-dependencies.py"
        shutil.copyfile(ROOT / relative, self.root / relative)
        for name, version, _ in PREPARE.PACKAGES:
            for relative in [f"patches/{name}-{version}.patch",
                             f".build/dependencies/{name}-{version}.crate"]:
                shutil.copyfile(ROOT / relative, self.root / relative)
        self.previous_root = PREPARE.ROOT
        PREPARE.ROOT = self.root
        self.addCleanup(setattr, PREPARE, "ROOT", self.previous_root)
        self.outputs = {name: self.root / ".build/dependencies" / name
                        for name, _, _ in PREPARE.PACKAGES}

    def prepare(self):
        with contextlib.redirect_stdout(io.StringIO()):
            PREPARE.main()

    def test_cached_sources_are_verified_and_tampering_is_repaired(self):
        self.prepare()
        for name, output in self.outputs.items():
            with self.subTest(crate=name):
                manifest = output / "Cargo.toml"
                dependencies = tomllib.loads(manifest.read_text())["dependencies"]
                if name == "gpui-base":
                    self.assertNotIn("instant", dependencies)
                    self.assertIn("web-time", dependencies)
                else:
                    self.assertEqual(dependencies["paste"]["package"], "pastey")
                stamp = manifest.stat().st_mtime_ns
                self.prepare()
                self.assertEqual(manifest.stat().st_mtime_ns, stamp)
                original = manifest.read_bytes()
                manifest.write_bytes(b"tampered")
                self.prepare()
                self.assertEqual(manifest.read_bytes(), original)

    def test_bad_archive_keeps_previous_prepared_sources(self):
        self.prepare()
        for name, version, _ in PREPARE.PACKAGES:
            with self.subTest(crate=name):
                manifest = self.outputs[name] / "Cargo.toml"
                original = manifest.read_bytes()
                archive = self.root / f".build/dependencies/{name}-{version}.crate"
                patch = self.root / f"patches/{name}-{version}.patch"
                archive_bytes, patch_bytes = archive.read_bytes(), patch.read_bytes()
                archive.write_bytes(b"bad archive")
                patch.write_bytes(patch_bytes + b"\n")
                with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                    self.prepare()
                self.assertEqual(manifest.read_bytes(), original)
                archive.write_bytes(archive_bytes)
                patch.write_bytes(patch_bytes)

    def test_bad_patch_keeps_previous_prepared_sources(self):
        self.prepare()
        for name, version, _ in PREPARE.PACKAGES:
            with self.subTest(crate=name):
                manifest = self.outputs[name] / "Cargo.toml"
                original = manifest.read_bytes()
                patch = self.root / f"patches/{name}-{version}.patch"
                patch_bytes = patch.read_bytes()
                patch.write_text("not a patch")
                with self.assertRaises(subprocess.CalledProcessError):
                    self.prepare()
                self.assertEqual(manifest.read_bytes(), original)
                patch.write_bytes(patch_bytes)


if __name__ == "__main__":
    unittest.main()
