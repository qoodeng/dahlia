#!/usr/bin/env python3
"""Release script tests. Every command is faked: nothing signs, notarizes, tags, pushes or publishes."""
import base64
import contextlib
import datetime
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import plistlib
import re
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
rel = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rel)
SHA = "a" * 40
IDENTITY = "Developer ID Application: Test Owner (A123456789)"
TODAY = datetime.date(2026, 10, 25)
NOW = datetime.datetime(2026, 10, 25, 15, 0, tzinfo=datetime.timezone.utc)
SEED = bytes(range(32))


def keypair(seed=SEED):
    digest = hashlib.sha512(seed).digest()
    scalar = int.from_bytes(digest[:32], "little")
    scalar &= (1 << 254) - 8
    scalar |= 1 << 254
    return scalar, digest[32:], rel._compress(rel._multiply(scalar, rel._G))


def ed25519_sign(seed, message):
    scalar, prefix, public = keypair(seed)
    nonce = int.from_bytes(hashlib.sha512(prefix + message).digest(), "little") % rel._Q
    r = rel._compress(rel._multiply(nonce, rel._G))
    h = int.from_bytes(hashlib.sha512(r + public + message).digest(), "little") % rel._Q
    return r + ((nonce + h * scalar) % rel._Q).to_bytes(32, "little")


PUBLIC = keypair()[2]
PUBLIC_B64 = base64.b64encode(PUBLIC).decode()


class Ed25519Tests(unittest.TestCase):
    def test_rfc_8032_vectors(self):
        vectors = [
            ("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a", "",
             "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"),
            ("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c", "72",
             "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"),
        ]
        for public, message, signature in vectors:
            with self.subTest(message=message):
                args = (bytes.fromhex(public), bytes.fromhex(signature))
                self.assertTrue(rel.ed25519_verify(*args, [bytes.fromhex(message)]))
                self.assertFalse(rel.ed25519_verify(*args, [bytes.fromhex(message) + b"x"]))

    def test_signing_round_trips_and_rejects_changes(self):
        message = b"a disk image" * 100000
        signature = ed25519_sign(SEED, message)
        chunks = [message[:777], message[777:]]
        self.assertTrue(rel.ed25519_verify(PUBLIC, signature, chunks))
        self.assertFalse(rel.ed25519_verify(PUBLIC, signature, [message[:-1] + b"!"]))
        self.assertFalse(rel.ed25519_verify(keypair(bytes(32))[2], signature, chunks))
        self.assertFalse(rel.ed25519_verify(PUBLIC, signature[:-1] + b"\xff", chunks))
        self.assertFalse(rel.ed25519_verify(PUBLIC, signature[:63], chunks))
        self.assertFalse(rel.ed25519_verify(PUBLIC[:31], signature, chunks))


class VersionTests(unittest.TestCase):
    def test_first_release_of_the_day_is_01(self):
        self.assertEqual(rel.next_version(TODAY, []), "2026.10.2501")

    def test_the_number_after_the_highest_existing_tag_today(self):
        self.assertEqual(rel.next_version(TODAY, ["v2026.10.2501"]), "2026.10.2502")
        self.assertEqual(rel.next_version(TODAY, ["v2026.10.2501", "v2026.10.2502"]), "2026.10.2503")
        self.assertEqual(rel.next_version(TODAY, ["v2026.10.2502", "v2026.10.2501"]), "2026.10.2503")
        # A gap is never refilled, and nine becomes ten.
        self.assertEqual(rel.next_version(TODAY, ["v2026.10.2504"]), "2026.10.2505")
        self.assertEqual(rel.next_version(TODAY, ["v2026.10.2509"]), "2026.10.2510")

    def test_other_days_and_other_tag_styles_do_not_count(self):
        tags = ["v2026.10.2401", "v2026.10.2402", "v2026.09.2501", "v2025.10.2503", "v2026.11.2507",
                "v1.0.1-remaster.1", "v2026.10.25", "v2026.10.250", "v2026.10.25001", "v2026.10.2500",
                "2026.10.2505", "v2026.10.2501-rc1", "xv2026.10.2506"]
        self.assertEqual(rel.next_version(TODAY, tags), "2026.10.2501")
        self.assertEqual(rel.next_version(TODAY, tags + ["v2026.10.2501"]), "2026.10.2502")

    def test_month_and_day_are_zero_padded(self):
        self.assertEqual(rel.next_version(datetime.date(2027, 1, 5), []), "2027.01.0501")
        self.assertEqual(rel.next_version(datetime.date(2027, 1, 5), ["v2027.01.0501"]), "2027.01.0502")

    def test_ninety_nine_is_the_limit(self):
        self.assertEqual(rel.next_version(TODAY, ["v2026.10.2598"]), "2026.10.2599")
        with self.assertRaisesRegex(rel.ReleaseError, "99 releases"):
            rel.next_version(TODAY, ["v2026.10.2599"])

    def test_local_and_remote_tags_both_count(self):
        env = Environment(self)
        env.local_tags, env.remote_tags = ["v2026.10.2501"], ["v2026.10.2502", "v2026.10.2402"]
        with env.patched():
            self.assertEqual(rel.next_version(TODAY, rel.existing_tags(env.root)), "2026.10.2503")
        env.local_tags, env.remote_tags = [], []
        with env.patched():
            self.assertEqual(rel.existing_tags(env.root), [])


class Environment:
    """A fake Mac, repository and GitHub, recording every command in order."""

    def __init__(self, test):
        scratch = tempfile.TemporaryDirectory()
        test.addCleanup(scratch.cleanup)
        self.base = Path(scratch.name)
        self.root = self.base / "repo"
        (self.root / "assets").mkdir(parents=True)
        (self.root / "scripts").mkdir()
        (self.root / "assets/Info.plist").write_bytes(plistlib.dumps({"LSMinimumSystemVersion": "13.0"}))
        (self.root / "scripts/sparkle-public-key.txt").write_text("# key\n" + PUBLIC_B64 + "\n")
        self.notes = self.base / "notes.md"
        self.notes.write_text("## Better\n\n- Faster **saves**\n- Fixes <script>alert(1)</script>\n")
        self.output = self.base / "out"
        self.calls = []
        self.branch, self.dirty, self.head, self.origin = "main", "", SHA, SHA
        self.remote = "https://github.com/qoodeng/dahlia.git"
        self.local_tags, self.remote_tags = [], []
        self.gh_signed_in, self.sparkle_key, self.host = True, True, "aarch64-apple-darwin"
        self.identities = f'  1) AAAA "{IDENTITY}"\n  2) BBBB "Apple Development: Someone (ZZZZ)"\n'
        self.failing = []
        self.signing_seed = SEED
        self.assets = ["Dahlia-2026.10.2501.dmg", "appcast.xml"]
        self.sparkle = types.SimpleNamespace(sign_update=self.base / "sparkle/sign_update")
        self.platform = "darwin"

    def failing_on(self, *prefix):
        self.failing.append(prefix)

    def record(self, args):
        self.calls.append(tuple(str(a) for a in args))
        for prefix in self.failing:
            if self.calls[-1][:len(prefix)] == prefix:
                raise rel.ReleaseError("forced failure: " + " ".join(prefix))

    def run(self, *args, cwd=None):
        self.record(args)
        command = self.calls[-1]
        if command[:3] == ("git", "rev-parse", "--abbrev-ref"):
            return self.branch + "\n"
        if command[:2] == ("git", "status"):
            return self.dirty
        if command[:3] == ("git", "rev-parse", "HEAD"):
            return self.head + "\n"
        if command[:3] == ("git", "rev-parse", "origin/main"):
            return self.origin + "\n"
        if command[:3] == ("git", "remote", "get-url"):
            return self.remote + "\n"
        if command[:3] == ("git", "tag", "--list"):
            return "\n".join(self.local_tags)
        if command[:2] == ("git", "ls-remote"):
            return "".join(f"{SHA}\trefs/tags/{tag}\n" for tag in self.remote_tags)
        if command[:2] == ("security", "find-identity"):
            return self.identities
        if command[:3] == ("gh", "release", "view"):
            return "\n".join(self.assets) + "\n"
        if command[0] == str(self.sparkle.sign_update):
            dmg = Path(command[1]).read_bytes()
            signature = base64.b64encode(ed25519_sign(self.signing_seed, dmg)).decode()
            return f'sparkle:edSignature="{signature}" length="{len(dmg)}"\n'
        return ""

    def succeeds(self, *args, cwd=None):
        self.calls.append(tuple(str(a) for a in args))
        if args[:3] == ("gh", "auth", "status"):
            return self.gh_signed_in
        if args[:2] == ("security", "find-generic-password"):
            return self.sparkle_key
        return True

    def stream(self, *args, cwd=None):
        self.record(args)
        command = self.calls[-1]
        out = Path(command[command.index("--output") + 1])
        version = command[command.index("--version") + 1]
        out.mkdir(parents=True)
        (out / f"Dahlia-{version}.dmg").write_bytes(b"disk image bytes " * 1000)
        name = "dry-run-receipt.json" if "--dry-run" in command else "receipt.json"
        (out / name).write_text(json.dumps({"version": version, "dmg": f"Dahlia-{version}.dmg",
                                            "host": self.host}))

    def prepared(self, root):
        self.calls.append(("prepare-sparkle",))
        return self.sparkle

    @contextlib.contextmanager
    def patched(self):
        with patch.object(rel, "run", self.run), patch.object(rel, "succeeds", self.succeeds), \
                patch.object(rel, "stream", self.stream), \
                patch.object(rel.packaging, "prepare_sparkle", self.prepared), \
                patch.object(rel.sys, "platform", self.platform):
            yield

    def args(self, **overrides):
        values = {"dry_run": False, "notes": str(self.notes), "notary_profile": "profile",
                  "identity": None, "output": str(self.output)}
        values.update(overrides)
        return types.SimpleNamespace(**values)

    def release(self, **overrides):
        with self.patched(), contextlib.redirect_stdout(io.StringIO()) as printed:
            try:
                version = rel.release(self.args(**overrides), self.root, today=TODAY, now=NOW)
            finally:
                self.printed = printed.getvalue()
        return version

    def commands(self, *prefix):
        return [c for c in self.calls if c[:len(prefix)] == prefix]

    def published(self):
        return [c for c in self.calls if c[:2] in (("git", "tag"), ("git", "push"), ("gh", "release"))
                and c[:3] != ("gh", "release", "view") and c[:3] != ("git", "tag", "--list")]


class PreconditionTests(unittest.TestCase):
    def setUp(self):
        self.env = Environment(self)

    def refused(self, message, **overrides):
        with self.assertRaisesRegex(rel.ReleaseError, message):
            self.env.release(**overrides)
        for forbidden in (("git", "tag", "-a"), ("git", "push"), ("gh", "release", "create")):
            self.assertEqual(self.env.commands(*forbidden), [])
        self.assertEqual(self.env.commands(rel.sys.executable), [], "nothing was built")
        self.assertFalse(self.env.output.exists())

    def test_a_ready_environment_releases(self):
        self.assertEqual(self.env.release(), "2026.10.2501")

    def test_must_be_on_main(self):
        self.env.branch = "release-pipeline"
        self.refused("Release from main, not release-pipeline")

    def test_detached_head_is_not_main(self):
        self.env.branch = "HEAD"
        self.refused("Release from main")

    def test_working_tree_must_be_clean(self):
        self.env.dirty = " M src/app.rs\n?? stray.txt\n"
        self.refused("uncommitted changes:\n  .*src/app.rs")

    def test_main_must_match_origin_main(self):
        self.env.origin = "b" * 40
        self.refused("not in sync with origin/main")

    def test_origin_main_is_fetched_before_comparing(self):
        self.env.release()
        fetch = self.env.calls.index(("git", "fetch", "--quiet", "origin", "main"))
        self.assertLess(fetch, self.env.calls.index(("git", "rev-parse", "origin/main")))

    def test_github_cli_must_be_signed_in(self):
        self.env.gh_signed_in = False
        self.refused("gh is not signed in")

    def test_a_developer_id_identity_must_exist(self):
        self.env.identities = '  1) BBBB "Apple Development: Someone (ZZZZ)"\n'
        self.refused("No Developer ID Application identity")

    def test_several_identities_need_a_choice(self):
        other = "Developer ID Application: Other Org (B987654321)"
        self.env.identities += f'  3) CCCC "{other}"\n'
        self.refused("Several Developer ID identities")
        self.env.calls.clear()
        self.assertEqual(self.env.release(identity=other), "2026.10.2501")
        self.assertIn(other, self.env.commands(rel.sys.executable)[0])

    def test_a_named_identity_must_be_present(self):
        self.refused("is not in the keychain", identity="Developer ID Application: Nobody (C000000000)")

    def test_the_notary_profile_is_required_from_a_flag_or_the_environment(self):
        self.refused("--notary-profile or DAHLIA_NOTARY_PROFILE", notary_profile=None)
        self.refused("--notary-profile or DAHLIA_NOTARY_PROFILE", notary_profile="")

    def test_environment_profile_is_the_flag_default(self):
        with patch.dict(rel.os.environ, {"DAHLIA_NOTARY_PROFILE": "from-env"}):
            with patch.object(rel, "release", side_effect=lambda args, root: setattr(self, "seen", args)):
                rel.main(["--notes", "n.md"])
        self.assertEqual(self.seen.notary_profile, "from-env")

    def test_the_sparkle_public_key_must_be_real(self):
        (self.env.root / "scripts/sparkle-public-key.txt").write_text("# nothing yet\n")
        self.refused("generate_keys")
        (self.env.root / "scripts/sparkle-public-key.txt").unlink()
        self.refused("is missing")

    def test_the_private_signing_key_must_be_in_the_keychain(self):
        self.env.sparkle_key = False
        self.refused("private signing key is not in the login keychain")

    def test_origin_must_be_this_repository(self):
        self.env.remote = "https://github.com/someone/else.git"
        self.refused("update feed points at github.com/qoodeng/dahlia")
        for remote in ("git@github.com:qoodeng/dahlia.git", "https://github.com/qoodeng/dahlia"):
            self.env.remote = remote
            self.env.calls.clear()
            shutil_rmtree(self.env.output)
            self.assertEqual(self.env.release(), "2026.10.2501")

    def test_release_notes_are_required(self):
        self.refused("--notes FILE", notes=None)
        self.env.notes.write_text("  \n")
        self.refused("--notes FILE")
        self.refused("--notes FILE", notes=str(self.env.base / "absent.md"))

    def test_only_a_mac_can_release(self):
        self.env.platform = "linux"
        self.refused("owner's Mac")

    def test_every_problem_is_reported_at_once(self):
        self.env.branch, self.env.dirty, self.env.gh_signed_in = "topic", " M x\n", False
        with self.assertRaises(rel.ReleaseError) as raised:
            self.env.release()
        for text in ("Release from main", "uncommitted changes", "gh is not signed in"):
            self.assertIn(text, str(raised.exception))


def shutil_rmtree(path):
    import shutil
    shutil.rmtree(path, ignore_errors=True)


class ReleaseFlowTests(unittest.TestCase):
    def setUp(self):
        self.env = Environment(self)

    def test_steps_run_in_order_and_publish_last(self):
        self.assertEqual(self.env.release(), "2026.10.2501")

        def step(call):
            if call[0] == rel.sys.executable:
                return "package"
            if call[0] == str(self.env.sparkle.sign_update):
                return "sign"
            if call[:3] == ("git", "tag", "-a"):
                return "tag"
            if call[:2] == ("git", "push"):
                return "push"
            if call[:3] == ("gh", "release", "create"):
                return "create"
            if call[:3] == ("gh", "release", "view"):
                return "check"
        steps = [name for name in map(step, self.env.calls) if name]
        self.assertEqual(steps, ["package", "sign", "tag", "push", "create", "check"])

    def test_packaging_gets_the_dated_version_the_exact_commit_and_the_credentials(self):
        self.env.release()
        command = self.env.commands(rel.sys.executable)[0]
        self.assertTrue(command[1].endswith("scripts/package-mac.py"))
        self.assertEqual(command[command.index("--version") + 1], "2026.10.2501")
        self.assertEqual(command[command.index("--expected-commit") + 1], SHA)
        self.assertEqual(command[command.index("--identity") + 1], IDENTITY)
        self.assertEqual(command[command.index("--notary-profile") + 1], "profile")
        self.assertNotIn("--dry-run", command)

    def test_the_second_release_of_a_day_is_02(self):
        self.env.local_tags = ["v2026.10.2501"]
        self.env.assets = ["Dahlia-2026.10.2502.dmg", "appcast.xml"]
        self.assertEqual(self.env.release(), "2026.10.2502")

    def test_publication_commands_tag_push_and_create_the_release(self):
        self.env.release()
        tag = self.env.commands("git", "tag", "-a")[0]
        self.assertEqual(tag, ("git", "tag", "-a", "v2026.10.2501", "-m", "Dahlia 2026.10.2501", SHA))
        self.assertEqual(self.env.commands("git", "push")[0],
                         ("git", "push", "origin", "refs/tags/v2026.10.2501:refs/tags/v2026.10.2501"))
        create = self.env.commands("gh", "release", "create")[0]
        self.assertEqual(create[3:5], ("v2026.10.2501", "--repo"))
        self.assertIn("qoodeng/dahlia", create)
        self.assertEqual(create[create.index("--title") + 1], "Dahlia 2026.10.2501")
        self.assertEqual(create[create.index("--notes-file") + 1], str(self.env.notes))
        self.assertIn("--verify-tag", create)
        self.assertEqual([Path(a).name for a in create[-2:]], ["Dahlia-2026.10.2501.dmg", "appcast.xml"])

    def test_the_appcast_is_written_beside_the_disk_image_and_names_the_release(self):
        self.env.release()
        feed = (self.env.output / "appcast.xml").read_text()
        root = ET.fromstring(feed)
        item = root.find("channel/item")
        ns = "{http://www.andymatuschak.org/xml-namespaces/sparkle}"
        self.assertEqual(item.find(ns + "version").text, "2026.10.2501")
        self.assertEqual(item.find(ns + "shortVersionString").text, "2026.10.2501")
        self.assertEqual(item.find(ns + "minimumSystemVersion").text, "13.0")
        self.assertEqual(item.find(ns + "hardwareRequirements").text, "arm64")
        enclosure = item.find("enclosure")
        self.assertEqual(enclosure.get("url"),
                         "https://github.com/qoodeng/dahlia/releases/download/v2026.10.2501/Dahlia-2026.10.2501.dmg")
        dmg = (self.env.output / "Dahlia-2026.10.2501.dmg").read_bytes()
        self.assertEqual(enclosure.get("length"), str(len(dmg)))
        signature = base64.b64decode(enclosure.get(ns + "edSignature"))
        self.assertTrue(rel.ed25519_verify(PUBLIC, signature, [dmg]))
        self.assertNotIn("DRY RUN", feed)

    def test_a_signature_from_another_key_is_never_published(self):
        self.env.signing_seed = bytes(range(1, 33))
        with self.assertRaisesRegex(rel.ReleaseError, "does not verify against scripts/sparkle-public-key.txt"):
            self.env.release()
        self.assertEqual(self.env.published(), [])
        self.assertFalse((self.env.output / "appcast.xml").exists())

    def test_unreadable_sign_update_output_stops_before_publishing(self):
        with patch.object(rel, "parse_signature", side_effect=rel.ReleaseError("Could not read")):
            with self.assertRaisesRegex(rel.ReleaseError, "Could not read"):
                self.env.release()
        self.assertEqual(self.env.published(), [])

    def test_a_failed_build_publishes_nothing(self):
        self.env.failing_on(rel.sys.executable)
        with self.assertRaisesRegex(rel.ReleaseError, "forced failure"):
            self.env.release()
        self.assertEqual(self.env.published(), [])
        self.assertEqual(self.env.commands(str(self.env.sparkle.sign_update)), [])

    def test_origin_moving_during_the_build_stops_before_tagging(self):
        original = self.env.stream

        def moving(*args, cwd=None):
            original(*args, cwd=cwd)
            self.env.origin = "c" * 40
        with patch.object(self.env, "stream", moving):
            with self.assertRaisesRegex(rel.ReleaseError, "origin/main moved"):
                self.env.release()
        self.assertEqual(self.env.published(), [])

    def test_a_tag_appearing_during_the_build_stops_before_tagging(self):
        original = self.env.stream

        def racing(*args, cwd=None):
            original(*args, cwd=cwd)
            self.env.remote_tags = ["v2026.10.2501"]
        with patch.object(self.env, "stream", racing):
            with self.assertRaisesRegex(rel.ReleaseError, "appeared during the build"):
                self.env.release()
        self.assertEqual(self.env.published(), [])

    def test_a_failed_push_creates_no_release_and_says_how_to_recover(self):
        self.env.failing_on("git", "push")
        with self.assertRaisesRegex(rel.ReleaseError, "RELEASING.md"):
            self.env.release()
        self.assertEqual(self.env.commands("gh", "release", "create"), [])

    def test_a_failed_release_after_tagging_keeps_the_artifacts_and_says_so(self):
        self.env.failing_on("gh", "release", "create")
        with self.assertRaisesRegex(rel.ReleaseError, "kept in .*out"):
            self.env.release()
        self.assertTrue((self.env.output / "appcast.xml").exists())

    def test_missing_assets_after_release_are_reported(self):
        self.env.assets = ["Dahlia-2026.10.2501.dmg"]
        with self.assertRaisesRegex(rel.ReleaseError, "expected Dahlia-2026.10.2501.dmg and appcast.xml"):
            self.env.release()

    def test_unexpected_packaging_output_is_rejected(self):
        original = self.env.stream

        def wrong(*args, cwd=None):
            original(*args, cwd=cwd)
            receipt = self.env.output / "receipt.json"
            receipt.write_text(json.dumps({"version": "2026.10.2501", "dmg": "Dahlia-1.0.dmg", "host": "aarch64-apple-darwin"}))
        with patch.object(self.env, "stream", wrong):
            with self.assertRaisesRegex(rel.ReleaseError, "something other than"):
                self.env.release()
        self.assertEqual(self.env.published(), [])


class DryRunTests(unittest.TestCase):
    def setUp(self):
        self.env = Environment(self)

    def test_dry_run_writes_nothing_remote_and_never_signs_for_real(self):
        self.assertEqual(self.env.release(dry_run=True), "2026.10.2501")
        self.assertEqual(self.env.published(), [])
        self.assertEqual(self.env.commands("git", "push"), [])
        self.assertEqual(self.env.commands(str(self.env.sparkle.sign_update)), [])
        self.assertEqual(self.env.commands("xcrun"), [])
        package = self.env.commands(rel.sys.executable)
        self.assertEqual(len(package), 1)
        self.assertIn("--dry-run", package[0])
        self.assertNotIn("--identity", package[0])
        self.assertNotIn("--notary-profile", package[0])

    def test_dry_run_prints_the_exact_commands_it_would_run(self):
        self.env.release(dry_run=True)
        out = self.env.printed
        for expected in (
                f"would run: {self.env.sparkle.sign_update} {self.env.output}/Dahlia-2026.10.2501.dmg",
                f"--identity '{IDENTITY}'", "--notary-profile profile", "--version 2026.10.2501",
                f"--expected-commit {SHA}",
                "would run: git tag -a v2026.10.2501 -m 'Dahlia 2026.10.2501' " + SHA,
                "would run: git push origin refs/tags/v2026.10.2501:refs/tags/v2026.10.2501",
                "would run: gh release create v2026.10.2501 --repo qoodeng/dahlia --title 'Dahlia 2026.10.2501'",
                "--verify-tag --latest", "Dahlia-2026.10.2501.dmg", "appcast.xml",
                "releases/latest/download/appcast.xml"):
            self.assertIn(expected, out)
        self.assertIn("Nothing was signed with your identity, sent to Apple, tagged, pushed or published.", out)

    def test_dry_run_appcast_is_marked_and_has_a_placeholder_signature(self):
        self.env.release(dry_run=True)
        feed = (self.env.output / "appcast.xml").read_text()
        self.assertIn("DRY RUN", feed)
        root = ET.fromstring(feed)
        signature = root.find("channel/item/enclosure").get("{http://www.andymatuschak.org/xml-namespaces/sparkle}edSignature")
        self.assertEqual(signature, rel.DRY_RUN_SIGNATURE)
        self.assertFalse(rel.ed25519_verify(PUBLIC, base64.b64decode(signature), [b"anything"]))
        self.assertIn("v2026.10.2501", feed)

    def test_dry_run_reports_what_would_block_a_real_release_and_carries_on(self):
        self.env.branch, self.env.dirty, self.env.gh_signed_in, self.env.sparkle_key = "topic", " M x\n", False, False
        self.env.identities = ""
        (self.env.root / "scripts/sparkle-public-key.txt").write_text("# none\n")
        self.assertEqual(self.env.release(dry_run=True, notes=None, notary_profile=None), "2026.10.2501")
        out = self.env.printed
        self.assertIn("a real release would stop here", out)
        for text in ("Release from main, not topic", "uncommitted changes", "gh is not signed in",
                     "No Developer ID Application identity", "generate_keys", "private signing key"):
            self.assertIn(text, out)
        self.assertIn("<Developer ID Application identity>", out)
        self.assertEqual(self.env.published(), [])
        self.assertTrue((self.env.output / "appcast.xml").exists())

    def test_dry_run_without_notes_still_makes_a_feed(self):
        self.env.release(dry_run=True, notes=None)
        self.assertIn("Dry run: no release notes.", (self.env.output / "appcast.xml").read_text())

    def test_dry_run_and_real_default_to_different_directories(self):
        with patch.object(rel.Path, "home", return_value=self.env.base / "home"):
            self.env.release(dry_run=True, output=None)
            self.assertTrue((self.env.base / "home/Dahlia-releases/2026.10.2501-dry-run/appcast.xml").exists())
            self.env.release(output=None)
            self.assertTrue((self.env.base / "home/Dahlia-releases/2026.10.2501/appcast.xml").exists())

    def test_intel_builds_are_not_restricted_to_apple_silicon(self):
        self.env.host = "x86_64-apple-darwin"
        self.env.release(dry_run=True)
        self.assertNotIn("hardwareRequirements", (self.env.output / "appcast.xml").read_text())

    def test_the_command_line_wrapper_is_safe(self):
        script = Path(__file__).with_name("release.sh")
        result = subprocess.run([str(script), "--help"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0)
        for flag in ("--dry-run", "--notes", "--notary-profile", "--identity", "--output"):
            self.assertIn(flag, result.stdout)


class AppcastTests(unittest.TestCase):
    NS = "{http://www.andymatuschak.org/xml-namespaces/sparkle}"

    def feed(self, notes="", **overrides):
        values = dict(version="2026.10.2501", length=1234, signature="c2ln+/==", minimum_system="13.0",
                      notes_markdown=notes, published=NOW)
        values.update(overrides)
        return rel.build_appcast(**values)

    def test_fixture_feed_has_every_field_sparkle_needs(self):
        item = ET.fromstring(self.feed("- one")).find("channel/item")
        self.assertEqual(item.find("title").text, "Dahlia 2026.10.2501")
        self.assertEqual(item.find("pubDate").text, "Sun, 25 Oct 2026 15:00:00 +0000")
        self.assertEqual(item.find(self.NS + "version").text, "2026.10.2501")
        self.assertEqual(item.find(self.NS + "shortVersionString").text, "2026.10.2501")
        self.assertEqual(item.find(self.NS + "minimumSystemVersion").text, "13.0")
        enclosure = item.find("enclosure")
        self.assertEqual(enclosure.get("url"),
                         "https://github.com/qoodeng/dahlia/releases/download/v2026.10.2501/Dahlia-2026.10.2501.dmg")
        self.assertEqual(enclosure.get("length"), "1234")
        self.assertEqual(enclosure.get("type"), "application/x-apple-diskimage")
        self.assertEqual(enclosure.get(self.NS + "edSignature"), "c2ln+/==")
        self.assertIsNone(item.find(self.NS + "hardwareRequirements"))

    def test_hardware_requirement_only_when_asked(self):
        item = ET.fromstring(self.feed(arm64_only=True)).find("channel/item")
        self.assertEqual(item.find(self.NS + "hardwareRequirements").text, "arm64")

    def test_notes_become_simple_html(self):
        markdown = ("# What’s new\n\nA **fast** and *friendly* update with `code`.\nSecond line.\n\n"
                    "- First [link](https://example.com/a?b=1&c=2)\n- Second\n\n## Fixed\n* Thing\n")
        html_ = rel.notes_html(markdown)
        self.assertEqual(html_, "\n".join([
            "<h3>What’s new</h3>",
            "<p>A <strong>fast</strong> and <em>friendly</em> update with <code>code</code>. Second line.</p>",
            '<ul>', '<li>First <a href="https://example.com/a?b=1&amp;c=2">link</a></li>', "<li>Second</li>", "</ul>",
            "<h4>Fixed</h4>", "<ul>", "<li>Thing</li>", "</ul>"]))

    def test_notes_cannot_inject_markup_scripts_or_break_out_of_cdata(self):
        hostile = ("<script>alert(1)</script> & <img src=x onerror=y>\n\n"
                   "[bad](javascript:alert(1)) [worse](data:text/html,x)\n\n"
                   "]]> ><![CDATA[ <!-- --> \"quoted\" 'single'\n")
        html_ = rel.notes_html(hostile)
        self.assertNotIn("<script", html_)
        self.assertNotIn("<img", html_)
        self.assertNotIn("<a ", html_)
        self.assertNotIn("]]>", html_)
        self.assertNotIn("<!--", html_)
        feed = self.feed(hostile)
        description = ET.fromstring(feed).find("channel/item/description").text
        self.assertEqual(description, html_)
        self.assertEqual(feed.count("<![CDATA["), 1)

    def test_empty_notes_leave_no_description(self):
        self.assertNotIn("<description>", self.feed(""))
        self.assertNotIn("<description>", self.feed("   \n\n"))

    def test_version_and_values_are_escaped(self):
        item = ET.fromstring(self.feed(signature='a"b<c&')).find("channel/item")
        self.assertEqual(item.find("enclosure").get(self.NS + "edSignature"), 'a"b<c&')

    def test_signature_output_parsing(self):
        output = 'sparkle:edSignature="abc123+/==" length="4096"\n'
        self.assertEqual(rel.parse_signature(output, 4096), "abc123+/==")
        with self.assertRaisesRegex(rel.ReleaseError, "Could not read"):
            rel.parse_signature(output, 4097)
        with self.assertRaisesRegex(rel.ReleaseError, "Could not read"):
            rel.parse_signature("error: no key", 4096)

    def test_identity_selection(self):
        listing = f'1) A "{IDENTITY}"\n2) B "Apple Development: X (Y)"\n3) C "{IDENTITY}"\n'
        self.assertEqual(rel.find_identity(listing, None), (IDENTITY, None))
        self.assertEqual(rel.find_identity(listing, IDENTITY), (IDENTITY, None))
        self.assertIsNone(rel.find_identity("", None)[0])


if __name__ == "__main__":
    unittest.main(verbosity=2)
