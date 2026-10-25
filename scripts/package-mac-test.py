#!/usr/bin/env python3
"""Packaging regression tests; no real signing, notarization, build or mount."""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("package_mac", Path(__file__).with_name("package-mac.py"))
pkg = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pkg)
SHA = "a" * 40
IDENTITY = "Developer ID Application: Test Owner (A123456789)"
VERSION = "2026.10.2501"
CARGO_VERSION = "1.0.0"
PUBLIC_KEY = base64.b64encode(bytes(range(1, 33))).decode()


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.base = Path(self.scratch.name)
        self.root = self.base / "repo"
        self.root.mkdir()
        for name, content in {"Cargo.toml": b"package", "Cargo.lock": b"locked", "LICENSE": b"MIT",
                              "vendor/markraft/LICENSE": b"MIT", "assets/brand/dahlia.icns": b"icon",
                              "assets/fonts/PaperMono-Regular.otf": b"regular",
                              "assets/fonts/PaperMono-Bold.otf": b"bold", "assets/fonts/LICENSE.txt": b"OFL"}.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        (self.root / "assets/fonts/provenance.json").write_text(json.dumps({"sha256": {
            name: hashlib.sha256(content).hexdigest() for name, content in
            (("PaperMono-Regular.otf", b"regular"), ("PaperMono-Bold.otf", b"bold"))}}))
        licenses = self.root / "assets/licenses"
        licenses.mkdir()
        (licenses / "THIRD-PARTY-NOTICES.txt").write_text("Fixture license notices")
        (licenses / "upstream.json").write_text("[]")
        (licenses / "manifest.json").write_text(json.dumps({
            "generator": "cargo-about 0.9.2", "target": "aarch64-apple-darwin",
            "cargo_lock_sha256": pkg.sha256(self.root / "Cargo.lock"),
            "notices_sha256": pkg.sha256(licenses / "THIRD-PARTY-NOTICES.txt"),
            "upstream_sha256": hashlib.sha256(b"[]").hexdigest(),
            "unresolved_attribution": []}))
        (self.root / "scripts").mkdir()
        (self.root / "scripts/sparkle-public-key.txt").write_text("# Sparkle EdDSA public key\n" + PUBLIC_KEY + "\n")
        self.sparkle = self.fake_sparkle()
        self.sparkle_roots = []
        self.args = argparse.Namespace(identity=IDENTITY, notary_profile="profile", expected_commit=SHA,
                                       output=str(self.base / "distribution"), version=VERSION, dry_run=False)
        self.calls = []
        self.app_status = "Accepted"
        self.dmg_status = "Accepted"
        self.tamper = False
        self.binary_name = "ortensia-rust"
        self.profile_available = True
        self.valid_signature = True
        self.external_dylib = False
        self.bad_nested_signature = False
        self.sparkle_archs = b"x86_64 arm64"

    def fake_sparkle(self, services=("Installer.xpc", "Downloader.xpc"), name="sparkle"):
        """A stand-in for the prepared Sparkle release, with its versioned layout and links."""
        base = self.base / name
        framework = base / "Sparkle.framework"
        version = framework / "Versions/B"
        for name in services:
            (version / "XPCServices" / name / "Contents/MacOS").mkdir(parents=True)
            (version / "XPCServices" / name / "Contents/MacOS" / name[:-4]).write_bytes(b"service")
        (version / "XPCServices").mkdir(parents=True, exist_ok=True)
        (version / "Updater.app/Contents/MacOS").mkdir(parents=True)
        (version / "Updater.app/Contents/MacOS/Updater").write_bytes(b"updater")
        (version / "Resources").mkdir()
        (version / "Resources/Info.plist").write_bytes(b"info")
        (version / "Sparkle").write_bytes(b"sparkle")
        (version / "Autoupdate").write_bytes(b"autoupdate")
        (framework / "Versions/Current").symlink_to("B")
        (framework / "Sparkle").symlink_to("Versions/Current/Sparkle")
        (base / "LICENSE").write_text("Sparkle license")
        return types.SimpleNamespace(framework=framework, license=base / "LICENSE", version="2.10.1",
                                     sha256="b" * 64)

    def fake_run(self, *arguments, cwd=None, env=None, evidence=None, combined=False):
        args = tuple(str(a) for a in arguments)
        self.calls.append(args)
        if evidence is not None:
            with evidence.open("a") as stream:
                stream.write("mock verified: " + " ".join(args) + "\n")
        if args[:3] == ("git", "rev-parse", "HEAD"):
            return SHA.encode()
        if args[:2] == ("git", "status"):
            return b""
        if args[:2] == ("git", "clone"):
            shutil.copytree(args[-2], args[-1])
        if args[:3] == ("xcrun", "notarytool", "history") and not self.profile_available:
            raise pkg.PackagingError("No Keychain password item found for profile")
        if args[0] == "security":
            return f'1) ABC "{IDENTITY}"'.encode()
        if args[:3] == ("cargo", "metadata", "--locked"):
            return json.dumps({"packages": [{"name": self.binary_name, "version": CARGO_VERSION,
                "manifest_path": str(cwd / "Cargo.toml"), "targets": [{"name": self.binary_name,
                "kind": ["bin"]}]}]}).encode()
        if args[:2] == ("cargo", "build"):
            binary = Path(env["CARGO_TARGET_DIR"]) / "aarch64-apple-darwin/release" / self.binary_name
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"built-from-locked-source")
            binary.chmod(0o755)
        if args[0] == "rustc":
            return b"rustc 1.95.0\nhost: aarch64-apple-darwin\n"
        if args[:2] == ("cargo", "-V"):
            return b"cargo 1.95.0"
        if args[0] == "lipo" and args[-1].endswith("Sparkle"):
            return self.sparkle_archs
        if args[0] == "lipo":
            return b"arm64"
        if args[0] == "otool" and self.external_dylib:
            return b"binary:\n\t/opt/homebrew/lib/not-bundled.dylib (compatibility version 1.0.0)\n"
        if args[0] == "otool":
            return b"binary:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n"
        if args[:2] == ("codesign", "--display") and (
                not self.valid_signature or (self.bad_nested_signature and not args[-1].endswith("Dahlia.app"))):
            return b"Signature=adhoc\n"
        if args[:2] == ("codesign", "--display"):
            return f"Authority={IDENTITY}\nTeamIdentifier=A123456789\nCodeDirectory flags=0x10000(runtime)\n".encode()
        if args[:3] == ("xcrun", "notarytool", "submit"):
            status = self.app_status if args[3].endswith(".zip") else self.dmg_status
            return json.dumps({"status": status, "id": "app-id" if args[3].endswith(".zip") else "dmg-id"}).encode()
        if args[:3] == ("xcrun", "notarytool", "log"):
            return b'{"issues": null}'
        if args[:2] == ("hdiutil", "create"):
            self.stage = Path(args[args.index("-srcfolder") + 1])
            self.info = plistlib.loads((self.stage / "Dahlia.app/Contents/Info.plist").read_bytes())
            Path(args[-1]).write_bytes(b"signed-and-stapled-image")
        if args[:2] == ("hdiutil", "attach"):
            mount = Path(args[args.index("-mountpoint") + 1])
            shutil.copytree(self.stage, mount, symlinks=True, dirs_exist_ok=True)
            if self.tamper:
                (mount / "Dahlia.app/Contents/Resources/build-commit.txt").write_text("wrong")
        if args[0] == "xcodebuild":
            return b"Xcode 26.0"
        if args[:3] == ("xcrun", "--sdk", "macosx"):
            return b"26.0"
        return b""

    def execute(self):
        def prepared(root):
            self.sparkle_roots.append(root)
            return self.sparkle

        with patch.object(pkg, "run", side_effect=self.fake_run), \
                patch.object(pkg, "prepare_sparkle", side_effect=prepared), \
                patch.object(pkg.platform, "system", return_value="Darwin"):
            pkg.package(self.args, self.root)

    def test_complete_workflow_qualifies_exact_mounted_bytes(self):
        self.execute()
        output = Path(self.args.output)
        receipt = json.loads((output / "receipt.json").read_text())
        self.assertEqual(receipt["source_revision"], SHA)
        self.assertEqual(receipt["version"], VERSION)
        self.assertNotIn("build_number", receipt)
        self.assertEqual(receipt["dmg"], f"Dahlia-{VERSION}.dmg")
        self.assertEqual(receipt["app_submission_id"], "app-id")
        self.assertEqual(receipt["dmg_submission_id"], "dmg-id")
        self.assertEqual(receipt["dmg_sha256"], pkg.sha256(output / receipt["dmg"]))
        self.assertIn("Contents/Resources/PaperMono-LICENSE.txt", receipt["app_manifest"])
        self.assertIn("Contents/Resources/THIRD-PARTY-NOTICES.txt", receipt["app_manifest"])
        self.assertEqual(receipt["third_party_notices_sha256"], pkg.sha256(self.root / "assets/licenses/THIRD-PARTY-NOTICES.txt"))
        build = next(c for c in self.calls if c[:2] == ("cargo", "build"))
        self.assertIn("--locked", build)
        self.assertIn("--release", build)
        self.assertLess(next(i for i, c in enumerate(self.calls) if c[:3] == ("xcrun", "notarytool", "history")),
                        self.calls.index(build))
        self.assertTrue(any(c[:2] == ("hdiutil", "detach") for c in self.calls))
        self.assertFalse(any(c[0] == "open" for c in self.calls))
        self.assertFalse(list(output.glob(".work-*")))

    def test_dated_version_is_both_bundle_versions_and_never_cargo_s(self):
        self.execute()
        self.assertEqual(self.info["CFBundleShortVersionString"], VERSION)
        self.assertEqual(self.info["CFBundleVersion"], VERSION)
        self.assertNotIn(CARGO_VERSION, plistlib.dumps(self.info).decode())
        output = Path(self.args.output)
        self.assertEqual([p.name for p in output.glob("*.dmg")], [f"Dahlia-{VERSION}.dmg"])
        self.assertEqual((output / "SHA256SUMS").read_text().split()[1], f"Dahlia-{VERSION}.dmg")

    def test_version_comes_from_the_argument_not_the_template_or_cargo(self):
        (self.root / "assets/Info.plist").write_bytes(plistlib.dumps({
            "CFBundleIdentifier": "dev.dahlia.app", "CFBundleShortVersionString": "0.0.0",
            "CFBundleVersion": "0.0.0"}))
        self.args.version = "2027.01.0107"
        self.execute()
        self.assertEqual((self.info["CFBundleShortVersionString"], self.info["CFBundleVersion"]),
                         ("2027.01.0107", "2027.01.0107"))

    def test_renamed_binary_and_plist_are_supported(self):
        self.binary_name = "dahlia"
        (self.root / "assets/Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": "dev.dahlia.app", "LSMinimumSystemVersion": "13.0"}))
        self.execute()
        receipt = json.loads((Path(self.args.output) / "receipt.json").read_text())
        self.assertEqual(receipt["bundle_identifier"], "dev.dahlia.app")
        self.assertIn("Contents/MacOS/dahlia", receipt["app_manifest"])

    def test_rejected_app_stops_before_dmg(self):
        self.app_status = "Invalid"
        with self.assertRaisesRegex(pkg.PackagingError, "did not accept app"):
            self.execute()
        self.assertFalse(any(c[:2] == ("hdiutil", "create") for c in self.calls))
        self.assertFalse((Path(self.args.output) / "receipt.json").exists())
        self.assertTrue((Path(self.args.output) / "app-notary.json").exists())

    def test_rejected_dmg_has_no_success_receipt(self):
        self.dmg_status = "Invalid"
        with self.assertRaisesRegex(pkg.PackagingError, "did not accept dmg"):
            self.execute()
        self.assertFalse((Path(self.args.output) / "receipt.json").exists())

    def test_mounted_tampering_fails_and_always_detaches(self):
        self.tamper = True
        with self.assertRaisesRegex(pkg.PackagingError, "Mounted app differs"):
            self.execute()
        self.assertTrue(any(c[:2] == ("hdiutil", "detach") for c in self.calls))
        self.assertFalse((Path(self.args.output) / "receipt.json").exists())

    def add_source_archive(self):
        directory = self.root / "assets/licenses"
        path = directory / "third-party-sources/example-1.0.0.crate"
        path.parent.mkdir()
        path.write_bytes(b"exact registry source bytes")
        archive = {"path": str(path.relative_to(directory)), "sha256": pkg.sha256(path)}
        sources = [{"archive": archive}]
        (directory / "upstream.json").write_text(json.dumps(sources))
        record = json.loads((directory / "manifest.json").read_text())
        record["upstream_sha256"] = hashlib.sha256(json.dumps(sources, sort_keys=True).encode()).hexdigest()
        record["source_archives"] = [archive]
        (directory / "manifest.json").write_text(json.dumps(record))
        return path

    def test_original_registry_source_is_in_the_verified_mounted_app(self):
        path = self.add_source_archive()
        self.execute()
        receipt = json.loads((Path(self.args.output) / "receipt.json").read_text())
        key = "Contents/Resources/third-party-sources/" + path.name
        self.assertEqual(receipt["app_manifest"][key]["sha256"], pkg.sha256(path))

    def test_modified_original_source_stops_before_build(self):
        self.add_source_archive().write_bytes(b"altered")
        with self.assertRaisesRegex(pkg.PackagingError, "registry source checksum differs"):
            self.execute()
        self.assertFalse(any(c[:2] == ("cargo", "build") for c in self.calls))

    def test_stale_or_modified_notices_stop_before_build(self):
        (self.root / "Cargo.lock").write_text("changed resolution")
        with self.assertRaisesRegex(pkg.PackagingError, "do not match Cargo.lock"):
            self.execute()
        self.assertFalse(any(c[:2] == ("cargo", "build") for c in self.calls))

    def test_unresolved_attribution_stops_before_build(self):
        path = self.root / "assets/licenses/manifest.json"
        record = json.loads(path.read_text())
        record["unresolved_attribution"] = ["missing@1.0.0"]
        path.write_text(json.dumps(record))
        with self.assertRaisesRegex(pkg.PackagingError, "Unresolved third-party attribution"):
            self.execute()
        self.assertFalse(any(c[:2] == ("cargo", "build") for c in self.calls))

    def test_notice_byte_tampering_stops_before_build(self):
        (self.root / "assets/licenses/THIRD-PARTY-NOTICES.txt").write_text("different")
        with self.assertRaisesRegex(pkg.PackagingError, "notice checksum differs"):
            self.execute()
        self.assertFalse(any(c[:2] == ("cargo", "build") for c in self.calls))

    def test_wrong_font_fails_before_signing(self):
        (self.root / "assets/fonts/PaperMono-Bold.otf").write_bytes(b"wrong font")
        with self.assertRaisesRegex(pkg.PackagingError, "Font provenance mismatch"):
            self.execute()
        self.assertFalse(any(c[0] == "codesign" for c in self.calls))

    def test_output_is_never_overwritten(self):
        Path(self.args.output).mkdir()
        sentinel = Path(self.args.output) / "keep"
        sentinel.write_text("untouched")
        with self.assertRaisesRegex(pkg.PackagingError, "Output already exists"):
            self.execute()
        self.assertEqual(sentinel.read_text(), "untouched")

    def test_missing_profile_is_explicit_and_does_no_work(self):
        self.args.notary_profile = ""
        with self.assertRaisesRegex(pkg.PackagingError, "nonempty notary profile"):
            self.execute()
        self.assertEqual(self.calls, [])

    def test_wrong_identity_and_short_sha_fail_early(self):
        self.args.identity = "-"
        with self.assertRaisesRegex(pkg.PackagingError, "Developer ID"):
            self.execute()
        self.args.expected_commit = "abc1234"
        with self.assertRaisesRegex(pkg.PackagingError, "full 40-character"):
            self.execute()

    def test_unavailable_profile_stops_before_build_or_output(self):
        self.profile_available = False
        with self.assertRaisesRegex(pkg.PackagingError, "No Keychain"):
            self.execute()
        self.assertFalse(any(c[:2] == ("cargo", "build") for c in self.calls))
        self.assertFalse(Path(self.args.output).exists())

    def test_adhoc_or_missing_runtime_signature_is_rejected(self):
        self.valid_signature = False
        with self.assertRaisesRegex(pkg.PackagingError, "hardened runtime"):
            self.execute()
        self.assertFalse((Path(self.args.output) / "receipt.json").exists())

    def test_unbundled_dynamic_library_is_rejected_before_signing(self):
        self.external_dylib = True
        with self.assertRaisesRegex(pkg.PackagingError, "Non-system dynamic dependency"):
            self.execute()
        self.assertFalse(any(c[0] == "codesign" for c in self.calls))

    def test_valid_versions_are_accepted(self):
        for value in ("2026.10.2501", "2026.01.0101", "2026.12.3199", "2024.02.2999", "2026.10.0109"):
            with self.subTest(version=value):
                self.assertEqual(pkg.validate_version(value), value)

    def test_malformed_or_unpadded_versions_are_rejected(self):
        for value in ("2026.10.251", "2026.1.2501", "26.10.2501", "2026.10.25001",
                      "2026.10.25100", "2026-10-2501", "v2026.10.2501", "2026.10.2501 ", " 2026.10.2501",
                      "2026.10.2501\n", "2026.10.25a1", "2026.10.２５01", "1.0.1", "", "2026.10", "0.0.0"):
            with self.subTest(version=value):
                with self.assertRaisesRegex(pkg.PackagingError, "YYYY.MM.DDNN"):
                    pkg.validate_version(value)

    def test_release_number_must_be_01_to_99(self):
        for value in ("2026.10.2501", "2026.10.2599"):
            self.assertEqual(pkg.validate_version(value), value)
        with self.assertRaisesRegex(pkg.PackagingError, "01 to 99"):
            pkg.validate_version("2026.10.2500")
        # A third digit makes the release number three digits long, which is not a version at all.
        with self.assertRaisesRegex(pkg.PackagingError, "YYYY.MM.DDNN"):
            pkg.validate_version("2026.10.25100")

    def test_impossible_dates_are_rejected(self):
        for value in ("2026.02.3001", "2026.02.2901", "2026.13.0101", "2026.00.1001", "2026.10.0001",
                      "2026.04.3101", "2026.11.3101", "2025.02.2901"):
            with self.subTest(version=value):
                with self.assertRaisesRegex(pkg.PackagingError, "real calendar date"):
                    pkg.validate_version(value)
        pkg.validate_version("2028.02.2901")

    def test_invalid_version_does_no_work(self):
        for value in ("2026.02.3001", "2026.10.2500", "1.0.1", None):
            with self.subTest(version=value):
                self.args.version = value
                with self.assertRaises(pkg.PackagingError):
                    self.execute()
                self.assertEqual(self.calls, [])
                self.assertFalse(Path(self.args.output).exists())

    def test_command_line_rejects_bad_versions_and_the_old_build_number(self):
        script = Path(__file__).with_name("package-mac.sh")
        common = ["--identity", IDENTITY, "--notary-profile", "p", "--expected-commit", SHA,
                  "--output", str(self.base / "never")]
        for extra in (["--version", "2026.02.3001"], ["--version", "2026.10.2500"],
                      ["--version", "2026.10.251"], ["--build-number", "4"], ["--version", "1.0.1"]):
            with self.subTest(extra=extra):
                result = subprocess.run([str(script), *common, *extra], capture_output=True, text=True)
                self.assertEqual(result.returncode, 2)
                self.assertFalse((self.base / "never").exists())

    def signing_calls(self):
        return [c for c in self.calls if c[0] == "codesign" and "--sign" in c]

    def test_sparkle_is_embedded_whole_with_its_links_licence_and_update_keys(self):
        self.execute()
        self.assertEqual(self.sparkle_roots, [self.root])
        info = self.info
        self.assertEqual(info["SUFeedURL"], "https://github.com/qoodeng/dahlia/releases/latest/download/appcast.xml")
        self.assertEqual(info["SUPublicEDKey"], PUBLIC_KEY)
        self.assertIs(info["SUEnableAutomaticChecks"], True)
        self.assertEqual(info["SUScheduledCheckInterval"], 86400)
        self.assertIs(info["SUAutomaticallyUpdate"], False)
        manifest = json.loads((Path(self.args.output) / "receipt.json").read_text())["app_manifest"]
        framework = "Contents/Frameworks/Sparkle.framework"
        for path in ("Versions/B/Sparkle", "Versions/B/Autoupdate", "Versions/B/Updater.app/Contents/MacOS/Updater",
                     "Versions/B/XPCServices/Installer.xpc/Contents/MacOS/Installer",
                     "Versions/B/XPCServices/Downloader.xpc/Contents/MacOS/Downloader", "Versions/B/Resources/Info.plist"):
            self.assertIn(f"{framework}/{path}", manifest)
        self.assertEqual(manifest[f"{framework}/Versions/Current"], {"symlink": "B"})
        self.assertEqual(manifest[f"{framework}/Sparkle"], {"symlink": "Versions/Current/Sparkle"})
        self.assertIn("Contents/Resources/Sparkle-LICENSE.txt", manifest)
        receipt = json.loads((Path(self.args.output) / "receipt.json").read_text())
        self.assertEqual((receipt["sparkle_version"], receipt["sparkle_sha256"]), ("2.10.1", "b" * 64))

    def test_sparkle_is_signed_inside_out_without_deep_and_before_notarization(self):
        self.execute()
        signed = [Path(c[-1]).name for c in self.signing_calls()]
        self.assertEqual(signed, ["Downloader.xpc", "Installer.xpc", "Autoupdate", "Updater.app",
                                  "Sparkle.framework", "Dahlia.app", f"Dahlia-{VERSION}.dmg"])
        for call in self.signing_calls():
            self.assertNotIn("--deep", call)
            self.assertIn("--timestamp", call)
            self.assertEqual(call[call.index("--sign") + 1], IDENTITY)
            if not call[-1].endswith(".dmg"):
                self.assertEqual(call[call.index("--options") + 1], "runtime")
        services = [c for c in self.signing_calls() if c[-1].endswith(".xpc")]
        self.assertTrue(all("--preserve-metadata=entitlements" in c for c in services))
        self.assertFalse(any("--preserve-metadata=entitlements" in c for c in self.signing_calls() if not c[-1].endswith(".xpc")))
        submit = next(i for i, c in enumerate(self.calls) if c[:3] == ("xcrun", "notarytool", "submit"))
        self.assertLess(self.calls.index(self.signing_calls()[5]), submit)

    def test_verification_covers_the_framework_and_its_helpers(self):
        self.execute()
        framework = [c for c in self.calls if c[:2] == ("codesign", "--verify") and c[-1].endswith("Sparkle.framework")]
        self.assertEqual(len(framework), 2, "once for the staged app and once for the mounted copy")
        shown = [Path(c[-1]).name for c in self.calls if c[:2] == ("codesign", "--display")]
        for name in ("Installer.xpc", "Downloader.xpc", "Autoupdate", "Updater.app", "Sparkle.framework"):
            self.assertEqual(shown.count(name), 2, name)

    def test_unsigned_or_wrong_team_sparkle_code_is_rejected(self):
        self.bad_nested_signature = True
        with self.assertRaisesRegex(pkg.PackagingError, "Sparkle’s .* lacks the requested identity"):
            self.execute()
        self.assertFalse((Path(self.args.output) / "receipt.json").exists())
        self.assertFalse(any(c[:2] == ("hdiutil", "create") for c in self.calls))

    def test_sparkle_missing_helpers_stops_before_signing(self):
        self.sparkle = self.fake_sparkle(services=(), name="incomplete")
        with self.assertRaisesRegex(pkg.PackagingError, "lacks its XPC services"):
            self.execute()
        self.assertFalse(self.signing_calls())

    def test_sparkle_without_the_host_architecture_is_rejected(self):
        self.sparkle_archs = b"x86_64"
        with self.assertRaisesRegex(pkg.PackagingError, "no arm64 code"):
            self.execute()
        self.assertFalse(self.signing_calls())

    def test_missing_or_placeholder_update_key_is_refused_before_any_work(self):
        key = self.root / "scripts/sparkle-public-key.txt"
        cases = {"missing": None, "empty": "", "comments only": "# generate it\n# later\n",
                 "text": "PLACEHOLDER\n", "not base64": "!!!notbase64!!!\n",
                 "short": base64.b64encode(b"short").decode() + "\n",
                 "zeros": base64.b64encode(bytes(32)).decode() + "\n",
                 "two keys": PUBLIC_KEY + "\n" + PUBLIC_KEY + "\n",
                 "dry-run stand-in": pkg.DRY_RUN_PUBLIC_KEY + "\n"}
        for name, content in cases.items():
            with self.subTest(case=name):
                key.unlink(missing_ok=True)
                if content is not None:
                    key.write_text(content)
                with self.assertRaisesRegex(pkg.PackagingError, "generate_keys"):
                    self.execute()
                self.assertEqual(self.calls, [])
                self.assertEqual(self.sparkle_roots, [])
                self.assertFalse(Path(self.args.output).exists())

    def test_the_dry_run_stand_in_key_is_a_well_formed_fake(self):
        self.assertEqual(len(base64.b64decode(pkg.DRY_RUN_PUBLIC_KEY, validate=True)), 32)

    def test_sparkle_fetch_failure_stops_before_output_or_build(self):
        def refuse(root):
            raise pkg.SPARKLE.SparkleError("Sparkle 2.10.1 checksum mismatch")
        with patch.object(pkg, "run", side_effect=self.fake_run), patch.object(pkg, "prepare_sparkle", side_effect=refuse), \
                patch.object(pkg.platform, "system", return_value="Darwin"):
            with self.assertRaisesRegex(pkg.SPARKLE.SparkleError, "checksum mismatch"):
                pkg.package(self.args, self.root)
        self.assertFalse(any(c[:2] == ("cargo", "build") for c in self.calls))
        self.assertFalse(Path(self.args.output).exists())

    def test_dry_run_is_ad_hoc_offline_and_cannot_pass_for_a_release(self):
        self.args.identity = self.args.notary_profile = None
        self.args.dry_run = True
        (self.root / "scripts/sparkle-public-key.txt").write_text("# no key yet\n")
        self.execute()
        output = Path(self.args.output)
        for forbidden in ("security", "spctl"):
            self.assertFalse(any(c[0] == forbidden for c in self.calls), forbidden)
        self.assertFalse(any(c[0] == "xcrun" and c[1] in ("notarytool", "stapler") for c in self.calls))
        self.assertFalse(any(c[0] == "ditto" for c in self.calls))
        self.assertEqual([Path(c[-1]).name for c in self.signing_calls()],
                         ["Downloader.xpc", "Installer.xpc", "Autoupdate", "Updater.app", "Sparkle.framework", "Dahlia.app"])
        for call in self.signing_calls():
            self.assertEqual(call[call.index("--sign") + 1], "-")
            self.assertNotIn("--timestamp", call)
            self.assertNotIn("--deep", call)
        verified = [c for c in self.calls if c[:2] == ("codesign", "--verify")]
        self.assertTrue(verified and all("--deep" in c and "--strict" in c for c in verified))
        self.assertFalse((output / "receipt.json").exists())
        self.assertFalse((output / "SHA256SUMS").exists())
        receipt = json.loads((output / "dry-run-receipt.json").read_text())
        self.assertTrue(receipt["dry_run"])
        self.assertIn("DRY RUN", receipt["qualification"])
        self.assertEqual(receipt["dmg"], f"Dahlia-{VERSION}.dmg")
        self.assertEqual(receipt["public_key"], pkg.DRY_RUN_PUBLIC_KEY)
        self.assertEqual(self.info["SUPublicEDKey"], pkg.DRY_RUN_PUBLIC_KEY)
        self.assertEqual(self.info["CFBundleVersion"], VERSION)

    def test_dry_run_uses_the_real_key_once_it_exists(self):
        self.args.identity = self.args.notary_profile = None
        self.args.dry_run = True
        self.execute()
        self.assertEqual(self.info["SUPublicEDKey"], PUBLIC_KEY)

    def test_without_dry_run_credentials_are_required_on_the_command_line(self):
        script = Path(__file__).with_name("package-mac.sh")
        result = subprocess.run([str(script), "--expected-commit", SHA, "--version", VERSION, "--output",
                                 str(self.base / "never")], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("required unless --dry-run", result.stderr)

    def test_non_mac_is_rejected(self):
        with patch.object(pkg.platform, "system", return_value="Linux"):
            with self.assertRaisesRegex(pkg.PackagingError, "user's Mac"):
                pkg.package(self.args, self.root)

    def test_clean_check_catches_untracked_and_tracked_changes(self):
        def git(*args):
            return subprocess.check_output(["git", *args], cwd=self.root, stderr=subprocess.DEVNULL).decode().strip()
        git("init", "-q")
        git("add", ".")
        git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture")
        head = git("rev-parse", "HEAD")
        self.assertEqual(pkg.clean_revision(self.root, head), head)
        with self.assertRaisesRegex(pkg.PackagingError, "expected"):
            pkg.clean_revision(self.root, SHA)
        (self.root / "untracked").write_text("preserve")
        with self.assertRaisesRegex(pkg.PackagingError, "untracked"):
            pkg.clean_revision(self.root, head)
        (self.root / "untracked").unlink()
        (self.root / "Cargo.toml").write_text("edited")
        with self.assertRaisesRegex(pkg.PackagingError, "Cargo.toml"):
            pkg.clean_revision(self.root, head)

    def test_help_and_missing_arguments_are_safe(self):
        script = Path(__file__).with_name("package-mac.sh")
        help_result = subprocess.run([str(script), "--help"], capture_output=True, text=True)
        self.assertEqual(help_result.returncode, 0)
        self.assertIn("--notary-profile", help_result.stdout)
        self.assertIn("YYYY.MM.DDNN", help_result.stdout)
        self.assertNotIn("--build-number", help_result.stdout)
        missing = subprocess.run([str(script)], capture_output=True, text=True)
        self.assertEqual(missing.returncode, 2)
        self.assertIn("required", missing.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
