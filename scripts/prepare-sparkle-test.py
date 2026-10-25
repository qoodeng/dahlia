#!/usr/bin/env python3
"""Sparkle preparation tests. A local file:// fixture stands in for the release; no network."""
import importlib.util
import io
import os
from pathlib import Path
import re
import stat
import sys
import tarfile
import tempfile
import unittest

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("prepare_sparkle", Path(__file__).with_name("prepare-sparkle.py"))
sp = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sp)


def add(tar, name, data=b"", mode=0o644, kind=tarfile.REGTYPE, link=""):
    info = tarfile.TarInfo(name)
    info.mode, info.type, info.linkname = mode, kind, link
    info.size = len(data) if kind == tarfile.REGTYPE else 0
    tar.addfile(info, io.BytesIO(data) if kind == tarfile.REGTYPE else None)


def release_archive(path, extra=(), skip=()):
    """A small archive laid out like Sparkle's release tarball."""
    with tarfile.open(path, "w:xz") as tar:
        entries = [
            ("./Sparkle.framework", b"", 0o755, tarfile.DIRTYPE, ""),
            ("./Sparkle.framework/Versions", b"", 0o755, tarfile.DIRTYPE, ""),
            ("./Sparkle.framework/Versions/B", b"", 0o755, tarfile.DIRTYPE, ""),
            ("./Sparkle.framework/Versions/B/Sparkle", b"framework", 0o755, tarfile.REGTYPE, ""),
            ("./Sparkle.framework/Versions/B/Autoupdate", b"autoupdate", 0o755, tarfile.REGTYPE, ""),
            ("./Sparkle.framework/Versions/B/Updater.app", b"", 0o755, tarfile.DIRTYPE, ""),
            ("./Sparkle.framework/Versions/B/Updater.app/Contents", b"", 0o755, tarfile.DIRTYPE, ""),
            ("./Sparkle.framework/Versions/B/XPCServices", b"", 0o755, tarfile.DIRTYPE, ""),
            ("./Sparkle.framework/Versions/Current", b"", 0o755, tarfile.SYMTYPE, "B"),
            ("./Sparkle.framework/Sparkle", b"", 0o755, tarfile.SYMTYPE, "Versions/Current/Sparkle"),
            ("./bin/sign_update", b"sign", 0o755, tarfile.REGTYPE, ""),
            ("./bin/generate_appcast", b"appcast", 0o755, tarfile.REGTYPE, ""),
            ("./bin/generate_keys", b"keys", 0o755, tarfile.REGTYPE, ""),
            ("./bin/BinaryDelta", b"delta", 0o755, tarfile.REGTYPE, ""),
            ("./LICENSE", b"Sparkle license", 0o644, tarfile.REGTYPE, ""),
            ("./Sparkle Test App.app/Contents/Info.plist", b"test app", 0o644, tarfile.REGTYPE, ""),
            ("./Symbols/Sparkle.framework.dSYM/x", b"symbols", 0o644, tarfile.REGTYPE, ""),
            *extra]
        for name, data, mode, kind, link in entries:
            if name not in skip:
                add(tar, name, data, mode, kind, link)


class SparkleTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.base = Path(self.scratch.name)
        self.root = self.base / "checkout"
        self.root.mkdir()
        self.source = self.base / "Sparkle-9.9.9.tar.xz"
        release_archive(self.source)
        self.sha = sp.digest(self.source)
        self.url = self.source.as_uri()

    def prepare(self, **overrides):
        options = {"root": self.root, "version": "9.9.9", "url": self.url, "sha256": self.sha}
        options.update(overrides)
        return sp.prepare(**options)

    def test_pinned_release_is_exact_and_official(self):
        self.assertRegex(sp.VERSION, r"^2\.\d+\.\d+$")
        self.assertRegex(sp.SHA256, r"^[0-9a-f]{64}$")
        self.assertTrue(sp.URL.startswith("https://github.com/sparkle-project/Sparkle/releases/download/"))
        self.assertTrue(sp.URL.endswith(f"/{sp.VERSION}/Sparkle-{sp.VERSION}.tar.xz"))

    def test_verified_archive_yields_framework_and_tools_in_the_cache(self):
        sparkle = self.prepare()
        self.assertEqual(sparkle.directory, self.root / ".build/sparkle/9.9.9/payload")
        self.assertTrue(sparkle.framework.joinpath("Versions/B/Sparkle").is_file())
        for tool in (sparkle.sign_update, sparkle.generate_appcast, sparkle.generate_keys):
            self.assertTrue(tool.is_file() and os.access(tool, os.X_OK), tool)
        self.assertEqual(sparkle.license.read_text(), "Sparkle license")
        self.assertEqual(sparkle.sha256, self.sha)
        # Links survive, so the framework keeps its versioned layout.
        self.assertTrue((sparkle.framework / "Versions/Current").is_symlink())
        self.assertEqual(os.readlink(sparkle.framework / "Sparkle"), "Versions/Current/Sparkle")
        self.assertTrue(stat.S_IMODE((sparkle.framework / "Versions/B/Autoupdate").stat().st_mode) & 0o100)
        # Test apps, symbols and unrelated tools stay out.
        names = {p.name for p in sparkle.directory.rglob("*")}
        self.assertNotIn("Sparkle Test App.app", names)
        self.assertNotIn("Symbols", names)
        self.assertNotIn("BinaryDelta", names)

    def test_a_prepared_cache_is_reused_without_the_archive_or_network(self):
        first = self.prepare()
        stamp = (first.directory / "prepared.json").stat().st_mtime_ns
        (self.root / ".build/sparkle/9.9.9/Sparkle-9.9.9.tar.xz").unlink()
        self.source.unlink()
        second = self.prepare()
        self.assertEqual(second.directory, first.directory)
        self.assertEqual((second.directory / "prepared.json").stat().st_mtime_ns, stamp)

    def test_checksum_failure_downloads_nothing_usable(self):
        with self.assertRaisesRegex(sp.SparkleError, "checksum mismatch"):
            self.prepare(sha256="0" * 64)
        base = self.root / ".build/sparkle/9.9.9"
        self.assertFalse((base / "payload").exists())
        self.assertFalse((base / "Sparkle-9.9.9.tar.xz").exists())
        self.assertEqual([p.name for p in base.iterdir()], [])

    def test_checksum_failure_leaves_a_previously_prepared_payload_alone(self):
        good = self.prepare()
        (good.directory / "marker").write_text("keep")
        tampered = self.base / "tampered.tar.xz"
        release_archive(tampered, extra=[("./bin/evil", b"x", 0o755, tarfile.REGTYPE, "")])
        # The cached archive is damaged and the fetch returns different bytes than the pin.
        (self.root / ".build/sparkle/9.9.9/Sparkle-9.9.9.tar.xz").write_bytes(b"corrupt")
        (good.directory / "prepared.json").unlink()
        with self.assertRaisesRegex(sp.SparkleError, "checksum mismatch"):
            self.prepare(url=tampered.as_uri())
        self.assertEqual((good.directory / "marker").read_text(), "keep")
        self.assertFalse((good.directory / "bin/evil").exists())

    def test_corrupt_cached_archive_is_replaced_by_a_verified_download(self):
        self.prepare()
        base = self.root / ".build/sparkle/9.9.9"
        (base / "Sparkle-9.9.9.tar.xz").write_bytes(b"corrupt")
        (base / "payload/prepared.json").unlink()
        sparkle = self.prepare()
        self.assertEqual(sp.digest(base / "Sparkle-9.9.9.tar.xz"), self.sha)
        self.assertTrue(sparkle.sign_update.is_file())

    def test_a_tampered_extraction_is_rebuilt_from_the_verified_archive(self):
        sparkle = self.prepare()
        sparkle.sign_update.unlink()
        self.assertTrue(self.prepare().sign_update.is_file())

    def test_a_different_pinned_checksum_never_reuses_the_old_extraction(self):
        first = self.prepare()
        other = self.base / "other.tar.xz"
        release_archive(other, extra=[("./bin/sign_update2", b"x", 0o755, tarfile.REGTYPE, "")])
        (self.root / ".build/sparkle/9.9.9/Sparkle-9.9.9.tar.xz").unlink()
        second = self.prepare(url=other.as_uri(), sha256=sp.digest(other))
        self.assertEqual(second.sha256, sp.digest(other))
        self.assertNotEqual(first.sha256, second.sha256)

    def test_archive_missing_the_tools_is_rejected(self):
        for index, missing in enumerate(("./bin/sign_update", "./bin/generate_appcast",
                                          "./Sparkle.framework/Versions/B/Autoupdate", "./LICENSE")):
            with self.subTest(missing=missing):
                broken = self.base / "broken.tar.xz"
                release_archive(broken, skip=(missing,))
                with self.assertRaisesRegex(sp.SparkleError, "lacks the framework or tools"):
                    self.prepare(url=broken.as_uri(), sha256=sp.digest(broken), root=self.base / f"missing-{index}")

    def test_paths_that_escape_the_cache_are_rejected(self):
        for index, name in enumerate(("../escape", "./../escape", "/etc/escape")):
            with self.subTest(name=name):
                evil = self.base / "evil.tar.xz"
                release_archive(evil, extra=[(name, b"x", 0o644, tarfile.REGTYPE, "")])
                root = self.base / f"escape-{index}"
                with self.assertRaisesRegex(sp.SparkleError, "outside itself"):
                    self.prepare(url=evil.as_uri(), sha256=sp.digest(evil), root=root)
                self.assertFalse((self.base / "escape").exists())

    def test_links_that_point_outside_the_cache_are_rejected(self):
        evil = self.base / "link.tar.xz"
        release_archive(evil, extra=[("./Sparkle.framework/escape", b"", 0o755, tarfile.SYMTYPE, "../../../../outside")])
        with self.assertRaisesRegex(sp.SparkleError, "unpack"):
            self.prepare(url=evil.as_uri(), sha256=sp.digest(evil))

    def test_only_https_or_local_files_are_downloaded(self):
        with self.assertRaisesRegex(sp.SparkleError, "Refusing"):
            self.prepare(url="http://example.invalid/Sparkle-9.9.9.tar.xz")
        with self.assertRaisesRegex(sp.SparkleError, "Refusing"):
            self.prepare(url="ftp://example.invalid/Sparkle-9.9.9.tar.xz")

    def test_missing_download_is_a_clear_error(self):
        with self.assertRaisesRegex(sp.SparkleError, "Couldn’t download"):
            self.prepare(url=(self.base / "absent.tar.xz").as_uri())
        self.assertFalse(list((self.root / ".build/sparkle/9.9.9").glob("*.partial")))

    def test_cache_stays_out_of_git(self):
        ignored = (Path(__file__).resolve().parent.parent / ".gitignore").read_text().split()
        self.assertIn("/.build/", ignored)


if __name__ == "__main__":
    unittest.main(verbosity=2)
