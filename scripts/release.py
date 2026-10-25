#!/usr/bin/env python3
"""One command from main to a published, notarized, auto-updating Dahlia release.

Computes today's version (YYYY.MM.DDNN), builds, signs, notarizes and staples through
package-mac.py, signs the disk image for Sparkle, writes appcast.xml, then tags and publishes
a GitHub release. `--dry-run` does everything that is local and safe and prints the rest.
"""
import argparse
import base64
import datetime
import email.utils
import hashlib
import html
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import re
import shlex
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from xml.sax.saxutils import escape, quoteattr

sys.dont_write_bytecode = True
REPOSITORY = "qoodeng/dahlia"
REMOTE = re.compile(r"(?:https://github\.com/|git@github\.com:)" + re.escape(REPOSITORY) + r"(?:\.git)?/?")
DOWNLOAD = f"https://github.com/{REPOSITORY}/releases/download"
RELEASE_PAGE = f"https://github.com/{REPOSITORY}/releases/tag"
FEED_URL = f"https://github.com/{REPOSITORY}/releases/latest/download/appcast.xml"
SPARKLE_NS = "http://www.andymatuschak.org/xml-namespaces/sparkle"
# Sparkle keeps the private key in the login keychain under this service and account.
KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT = "https://sparkle-project.org", "ed25519"
IDENTITY = re.compile(r'"(Developer ID Application: .+ \(([A-Z0-9]{10})\))"')
DRY_RUN_SIGNATURE = base64.b64encode(b"DRY-RUN-NOT-A-SIGNATURE".ljust(64, b"\0")).decode()


class ReleaseError(Exception):
    pass


def _load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


packaging = _load("package-mac")


# --- running commands ------------------------------------------------------------------------

def run(*args, cwd=None):
    """Run a command and return its output; raise with its error text if it fails."""
    result = subprocess.run([str(a) for a in args], cwd=cwd, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True)
    if result.returncode:
        raise ReleaseError(f"Command failed ({result.returncode}): {shlex.join(str(a) for a in args)}\n"
                           + result.stderr + result.stdout)
    return result.stdout


def succeeds(*args, cwd=None):
    return subprocess.run([str(a) for a in args], cwd=cwd, stdout=subprocess.DEVNULL,
                          stderr=subprocess.DEVNULL).returncode == 0


def stream(*args, cwd=None):
    """Run a long command with its output on screen."""
    if subprocess.run([str(a) for a in args], cwd=cwd).returncode:
        raise ReleaseError(f"Command failed: {shlex.join(str(a) for a in args)}")


def say(message):
    print(message, flush=True)


# --- the version -----------------------------------------------------------------------------

def next_version(today, tags):
    """Today's next YYYY.MM.DDNN given the existing tags. Other days' tags and other tag styles
    do not count; the number after the highest tag already used today is taken, never a gap."""
    prefix = f"{today:%Y.%m.%d}"
    used = [int(m[1]) for tag in tags if (m := re.fullmatch(r"v" + re.escape(prefix) + r"([0-9]{2})", tag.strip()))]
    number = max(used, default=0) + 1
    if number > 99:
        raise ReleaseError(f"{prefix} already has 99 releases; wait until tomorrow")
    return packaging.validate_version(f"{prefix}{number:02d}")


def existing_tags(root):
    local = run("git", "tag", "--list", "v*", cwd=root).split()
    remote = [line.split("refs/tags/")[-1] for line in
              run("git", "ls-remote", "--tags", "--refs", "origin", "v*", cwd=root).splitlines() if "refs/tags/" in line]
    return sorted(set(local) | set(remote))


# --- preconditions ---------------------------------------------------------------------------

def find_identity(output, wanted):
    """The Developer ID Application identity to sign with, or a problem describing why none."""
    found = [m[1] for m in IDENTITY.finditer(output)]
    found = sorted(set(found))
    if wanted:
        return (wanted, None) if wanted in found else (None, f"Signing identity {wanted!r} is not in the keychain")
    if not found:
        return None, "No Developer ID Application identity is in the keychain (see RELEASING.md)"
    if len(found) > 1:
        return None, "Several Developer ID identities are in the keychain; choose one with --identity:\n  " + "\n  ".join(found)
    return found[0], None


def check_preconditions(root, args):
    """Everything that must be true before building. Returns (problems, identity)."""
    problems = []

    def check(condition, message):
        if not condition:
            problems.append(message)

    if sys.platform != "darwin":
        problems.append("Releases are built on the owner's Mac")
    branch = run("git", "rev-parse", "--abbrev-ref", "HEAD", cwd=root).strip()
    check(branch == "main", f"Release from main, not {branch}")
    dirty = run("git", "status", "--porcelain", "--untracked-files=all", "--ignore-submodules=dirty", cwd=root)
    check(not dirty.strip(), "The working tree has uncommitted changes:\n" + dirty.rstrip())
    try:
        run("git", "fetch", "--quiet", "origin", "main", cwd=root)
        head = run("git", "rev-parse", "HEAD", cwd=root).strip()
        origin = run("git", "rev-parse", "origin/main", cwd=root).strip()
        check(head == origin, "main is not in sync with origin/main; pull or push first "
                              f"(HEAD {head[:12]}, origin/main {origin[:12]})")
    except ReleaseError as error:
        problems.append(f"Could not compare with origin/main: {error}")
    remote = run("git", "remote", "get-url", "origin", cwd=root).strip()
    check(REMOTE.fullmatch(remote), f"origin is {remote}, but the update feed points at github.com/{REPOSITORY}")
    check(succeeds("gh", "auth", "status"), "gh is not signed in; run `gh auth login`")
    identity, problem = find_identity(run("security", "find-identity", "-v", "-p", "codesigning"), args.identity)
    if problem:
        problems.append(problem)
    check(args.notary_profile, "Name the notarytool keychain profile with --notary-profile or DAHLIA_NOTARY_PROFILE")
    try:
        packaging.sparkle_public_key(root)
    except packaging.PackagingError as error:
        problems.append(str(error))
    check(succeeds("security", "find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", KEYCHAIN_ACCOUNT),
          "Sparkle's private signing key is not in the login keychain (see RELEASING.md)")
    if not args.dry_run:
        check(args.notes and Path(args.notes).is_file() and Path(args.notes).read_text().strip(),
              "Give the release notes with --notes FILE (a non-empty Markdown file)")
    return problems, identity


# --- Ed25519 (RFC 8032), to check Sparkle's signature against the committed public key ---------

_P = 2**255 - 19
_Q = 2**252 + 27742317777372353535851937790883648493
_D = -121665 * pow(121666, _P - 2, _P) % _P
_I = pow(2, (_P - 1) // 4, _P)


def _recover_x(y, sign):
    if y >= _P:
        return None
    x2 = (y * y - 1) * pow(_D * y * y + 1, _P - 2, _P) % _P
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (_P + 3) // 8, _P)
    if (x * x - x2) % _P:
        x = x * _I % _P
    if (x * x - x2) % _P:
        return None
    return _P - x if (x & 1) != sign else x


_GY = 4 * pow(5, _P - 2, _P) % _P
_GX = _recover_x(_GY, 0)
_G = (_GX, _GY, 1, _GX * _GY % _P)


def _add(a, b):
    p = (a[1] - a[0]) * (b[1] - b[0]) % _P
    q = (a[1] + a[0]) * (b[1] + b[0]) % _P
    c = 2 * a[3] * b[3] * _D % _P
    d = 2 * a[2] * b[2] % _P
    e, f, g, h = q - p, d - c, d + c, q + p
    return e * f % _P, g * h % _P, f * g % _P, e * h % _P


def _multiply(scalar, point):
    result = (0, 1, 1, 0)
    while scalar:
        if scalar & 1:
            result = _add(result, point)
        point = _add(point, point)
        scalar >>= 1
    return result


def _decompress(data):
    y = int.from_bytes(data, "little")
    x = _recover_x(y & ((1 << 255) - 1), y >> 255)
    return None if x is None else (x, y & ((1 << 255) - 1), 1, x * (y & ((1 << 255) - 1)) % _P)


def _compress(point):
    zinv = pow(point[2], _P - 2, _P)
    x, y = point[0] * zinv % _P, point[1] * zinv % _P
    return (y | ((x & 1) << 255)).to_bytes(32, "little")


def ed25519_verify(public, signature, chunks):
    """Whether `signature` (64 bytes) is `public`'s (32 bytes) Ed25519 signature of the bytes in `chunks`."""
    if len(public) != 32 or len(signature) != 64:
        return False
    a, r = _decompress(public), _decompress(signature[:32])
    s = int.from_bytes(signature[32:], "little")
    if a is None or r is None or s >= _Q:
        return False
    digest = hashlib.sha512(signature[:32] + public)
    for chunk in chunks:
        digest.update(chunk)
    h = int.from_bytes(digest.digest(), "little") % _Q
    left, right = _multiply(s, _G), _add(r, _multiply(h, a))
    return (left[0] * right[2] - right[0] * left[2]) % _P == 0 and (left[1] * right[2] - right[1] * left[2]) % _P == 0


def file_chunks(path):
    with Path(path).open("rb") as stream_:
        yield from iter(lambda: stream_.read(1024 * 1024), b"")


# --- the appcast -----------------------------------------------------------------------------

def inline(text):
    """Escape `text`, then restore the few Markdown spans release notes use."""
    text = html.escape(text, quote=True)
    text = re.sub(r"`([^`]+)`", r"<code>\1</code>", text)
    text = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    text = re.sub(r"(?<![*\w])\*([^*\s][^*]*)\*(?![*\w])", r"<em>\1</em>", text)
    return re.sub(r"\[([^\]]+)\]\((https?://(?:[^\s)&<>\"']|&amp;)+)\)", r'<a href="\2">\1</a>', text)


def notes_html(markdown):
    """Release notes as simple HTML: headings, paragraphs, bullet lists, bold, italic, code, links.
    Everything is escaped first, so the notes cannot add markup, scripts or `]]>`."""
    out, paragraph, in_list = [], [], False

    def flush():
        if paragraph:
            out.append("<p>" + " ".join(paragraph) + "</p>")
            paragraph.clear()

    def close_list():
        nonlocal in_list
        if in_list:
            out.append("</ul>")
            in_list = False

    for line in markdown.splitlines():
        stripped = line.strip()
        heading = re.fullmatch(r"(#{1,6})\s+(.*)", stripped)
        bullet = re.fullmatch(r"[-*+]\s+(.*)", stripped)
        if not stripped:
            flush()
            close_list()
        elif heading:
            flush()
            close_list()
            level = min(len(heading[1]) + 2, 6)  # the update window supplies the title
            out.append(f"<h{level}>{inline(heading[2])}</h{level}>")
        elif bullet:
            flush()
            if not in_list:
                out.append("<ul>")
                in_list = True
            out.append(f"<li>{inline(bullet[1])}</li>")
        else:
            close_list()
            paragraph.append(inline(stripped))
    flush()
    close_list()
    return "\n".join(out)


def build_appcast(version, length, signature, minimum_system, notes_markdown, published,
                  arm64_only=False, dry_run=False):
    """A one-item Sparkle feed. `releases/latest/download/appcast.xml` always serves the newest
    release's feed, so the feed only ever needs to describe that release."""
    url = f"{DOWNLOAD}/v{version}/Dahlia-{version}.dmg"
    body = notes_html(notes_markdown)
    lines = ['<?xml version="1.0" encoding="utf-8"?>']
    if dry_run:
        lines.append("<!-- DRY RUN: the signature is a placeholder and the file was never published. -->")
    lines += [
        f'<rss version="2.0" xmlns:sparkle="{SPARKLE_NS}">',
        "  <channel>",
        "    <title>Dahlia</title>",
        "    <item>",
        f"      <title>{escape(f'Dahlia {version}')}</title>",
        f"      <pubDate>{email.utils.format_datetime(published)}</pubDate>",
        f"      <sparkle:version>{escape(version)}</sparkle:version>",
        f"      <sparkle:shortVersionString>{escape(version)}</sparkle:shortVersionString>",
        f"      <sparkle:minimumSystemVersion>{escape(minimum_system)}</sparkle:minimumSystemVersion>",
    ]
    if arm64_only:
        lines.append("      <sparkle:hardwareRequirements>arm64</sparkle:hardwareRequirements>")
    if body:
        lines.append(f"      <description><![CDATA[{body}]]></description>")
    lines += [
        f"      <enclosure url={quoteattr(url)} length={quoteattr(str(length))} "
        f'type="application/x-apple-diskimage" sparkle:edSignature={quoteattr(signature)}/>',
        "    </item>",
        "  </channel>",
        "</rss>",
        "",
    ]
    text = "\n".join(lines)
    ET.fromstring(text)  # well-formed, or fail here rather than ship a feed Sparkle rejects
    return text


def parse_signature(output, size):
    match = re.search(r'sparkle:edSignature="([A-Za-z0-9+/=]+)"\s+length="(\d+)"', output)
    if not match or int(match[2]) != size:
        raise ReleaseError("Could not read sign_update's signature and length:\n" + output)
    return match[1]


# --- the release -----------------------------------------------------------------------------

def publication_commands(version, sha, dmg, appcast, notes):
    """The commands that make a release public, in order. Run for real, or printed by --dry-run."""
    tag = f"v{version}"
    return [
        ["git", "tag", "-a", tag, "-m", f"Dahlia {version}", sha],
        ["git", "push", "origin", f"refs/tags/{tag}:refs/tags/{tag}"],
        ["gh", "release", "create", tag, "--repo", REPOSITORY, "--title", f"Dahlia {version}",
         "--notes-file", str(notes), "--verify-tag", "--latest", str(dmg), str(appcast)],
    ]


def package_command(root, version, sha, output, identity, profile, dry_run):
    command = [sys.executable, str(root / "scripts/package-mac.py"), "--version", version,
               "--expected-commit", sha, "--output", str(output)]
    if dry_run:
        return command + ["--dry-run"]
    return command + ["--identity", identity, "--notary-profile", profile]


def sign_command(sparkle, dmg):
    return [str(sparkle.sign_update), str(dmg)]


def release(args, root, today=None, now=None):
    today = today or datetime.date.today()
    now = now or datetime.datetime.now(datetime.timezone.utc)
    dry_run = args.dry_run
    problems, identity = check_preconditions(root, args)
    if problems:
        if not dry_run:
            raise ReleaseError("Not ready to release:\n- " + "\n- ".join(p.replace("\n", "\n  ") for p in problems))
        say("Dry run: a real release would stop here.\n- " + "\n- ".join(p.replace("\n", "\n  ") for p in problems) + "\n")
    version = next_version(today, existing_tags(root))
    sha = run("git", "rev-parse", "HEAD", cwd=root).strip()
    # A rehearsal never shares a directory with the real build: packaging refuses to reuse one.
    output = Path(args.output).expanduser() if args.output else (
        Path.home() / "Dahlia-releases" / (version + ("-dry-run" if dry_run else "")))
    say(f"{'DRY RUN: ' if dry_run else ''}Releasing Dahlia {version} from {sha[:12]}; output {output}")
    plist = plistlib.loads((root / "assets/Info.plist").read_bytes())
    minimum_system = plist["LSMinimumSystemVersion"]
    notes_path = Path(args.notes) if args.notes else None
    notes = notes_path.read_text() if notes_path else "Dry run: no release notes."
    sparkle = packaging.prepare_sparkle(root)

    say("==> Building" + ("" if dry_run else ", signing and notarizing") + " (this takes a while)")
    stream(*package_command(root, version, sha, output, identity, args.notary_profile, dry_run))
    receipt = json.loads((output / ("dry-run-receipt.json" if dry_run else "receipt.json")).read_text())
    dmg = output / receipt["dmg"]
    if dmg.name != f"Dahlia-{version}.dmg" or receipt["version"] != version:
        raise ReleaseError("Packaging produced something other than Dahlia-<version>.dmg for this version")
    size = dmg.stat().st_size

    say("==> Signing the disk image for Sparkle")
    if dry_run:
        signature = DRY_RUN_SIGNATURE
        say("would run: " + shlex.join(sign_command(sparkle, dmg)) + "   (dry run: placeholder signature used)")
    else:
        signature = parse_signature(run(*sign_command(sparkle, dmg)), size)
        key = base64_key(root)
        if not ed25519_verify(key, base64.b64decode(signature), file_chunks(dmg)):
            raise ReleaseError("Sparkle's signature does not verify against scripts/sparkle-public-key.txt. "
                               "The login keychain holds a different key than the one in the app; nothing was published.")

    say("==> Writing appcast.xml")
    appcast = output / "appcast.xml"
    appcast.write_text(build_appcast(version, size, signature, minimum_system, notes, now,
                                     arm64_only=receipt["host"].startswith("aarch64"), dry_run=dry_run))
    say(f"    {appcast}")

    commands = publication_commands(version, sha, dmg, appcast, notes_path or "<notes file>")
    if dry_run:
        say("==> Would notarize and publish (not run)")
        real = output if args.output else output.with_name(version)
        say("would run: " + shlex.join(package_command(root, version, sha, real,
                                                       identity or "<Developer ID Application identity>",
                                                       args.notary_profile or "<notary profile>", False)))
        for command in commands:
            say("would run: " + shlex.join(command))
        say(f"Nothing was signed with your identity, sent to Apple, tagged, pushed or published.\n"
            f"Feed URL the app reads: {FEED_URL}")
        return version

    say("==> Publishing")
    # Re-check what the long build could have changed before anything becomes public.
    run("git", "fetch", "--quiet", "origin", "main", cwd=root)
    if run("git", "rev-parse", "origin/main", cwd=root).strip() != sha:
        raise ReleaseError("origin/main moved during the build; nothing was published. Run the release again.")
    if f"v{version}" in existing_tags(root):
        raise ReleaseError(f"Tag v{version} appeared during the build; nothing was published. Run the release again.")
    tag, push, create = commands
    run(*tag, cwd=root)
    try:
        run(*push, cwd=root)
        run(*create, cwd=root)
    except ReleaseError as error:
        raise ReleaseError(f"{error}\nThe disk image and appcast are kept in {output}. v{version} may now exist locally "
                           f"or on GitHub; see RELEASING.md, \"When publishing fails\", before running again.") from None
    assets = run("gh", "release", "view", f"v{version}", "--repo", REPOSITORY, "--json", "assets",
                 "--jq", ".assets[].name", cwd=root).split()
    if sorted(assets) != sorted([dmg.name, "appcast.xml"]):
        raise ReleaseError(f"The release exists but its assets are {assets}; expected {dmg.name} and appcast.xml")
    say(f"Published https://github.com/{REPOSITORY}/releases/tag/v{version}\nFeed: {FEED_URL}")
    return version


def base64_key(root):
    return base64.b64decode(packaging.sparkle_public_key(root))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true",
                        help="Do the local, safe parts (unsigned build with Sparkle, appcast with a placeholder "
                             "signature) and print the signing, notarizing, tagging and publishing commands")
    parser.add_argument("--notes", metavar="FILE", help="Release notes, Markdown (required unless --dry-run)")
    parser.add_argument("--notary-profile", default=os.environ.get("DAHLIA_NOTARY_PROFILE"),
                        help="notarytool keychain profile (default: $DAHLIA_NOTARY_PROFILE)")
    parser.add_argument("--identity", help="Developer ID Application identity, if the keychain has more than one")
    parser.add_argument("--output", help="New directory for the build (default ~/Dahlia-releases/<version>)")
    args = parser.parse_args(argv)
    try:
        release(args, Path(__file__).resolve().parent.parent)
    except (ReleaseError, packaging.PackagingError, packaging.SPARKLE.SparkleError, OSError) as error:
        sys.exit(f"error: {error}")


if __name__ == "__main__":
    main()
