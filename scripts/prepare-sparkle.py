#!/usr/bin/env python3
"""Fetch the pinned Sparkle release into .build/sparkle/<version>/ and verify its checksum.

Run by package-mac.py and release.py, never by prepare.sh, so `cargo build` and `cargo test`
never need Sparkle or the network. Prints the prepared directory.
"""
import hashlib
import json
import os
from pathlib import Path
import posixpath
import shutil
import sys
import tarfile
import tempfile
from typing import NamedTuple
from urllib.parse import urlparse
from urllib.request import urlopen

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
# Latest stable Sparkle 2.x when this was pinned. To update, take the tarball from
# https://github.com/sparkle-project/Sparkle/releases, compute its SHA-256 yourself, change
# both lines, and re-run the release dry run.
VERSION = "2.10.1"
SHA256 = "1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27"
URL = f"https://github.com/sparkle-project/Sparkle/releases/download/{VERSION}/Sparkle-{VERSION}.tar.xz"
# The tarball also carries test apps and debug symbols; only these are extracted.
WANTED_DIRECTORIES = ("Sparkle.framework",)
WANTED_FILES = ("LICENSE", "bin/generate_appcast", "bin/generate_keys", "bin/sign_update")
MAX_BYTES = 128 * 1024 * 1024


class SparkleError(Exception):
    pass


class Sparkle(NamedTuple):
    directory: Path
    framework: Path
    sign_update: Path
    generate_appcast: Path
    generate_keys: Path
    license: Path
    version: str
    sha256: str


def digest(path):
    with Path(path).open("rb") as stream:
        result = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
        return result.hexdigest()


def download(url, target):
    if urlparse(url).scheme not in ("https", "file"):
        raise SparkleError(f"Refusing to download Sparkle over {urlparse(url).scheme or 'an unknown scheme'}")
    try:
        with urlopen(url, timeout=120) as response, target.open("wb") as out:
            size = 0
            for chunk in iter(lambda: response.read(1024 * 1024), b""):
                size += len(chunk)
                if size > MAX_BYTES:
                    raise SparkleError("The Sparkle download is larger than expected")
                out.write(chunk)
    except OSError as error:
        raise SparkleError(f"Couldn’t download Sparkle from {url}: {error}") from None


def member_path(name):
    """A tar member's path relative to the archive root, or None if it escapes it."""
    path = posixpath.normpath(name)
    if path.startswith(("/", "../")) or path == "..":
        return None
    return path.removeprefix("./")


def wanted(path):
    return path in WANTED_FILES or any(path == d or path.startswith(d + "/") for d in WANTED_DIRECTORIES)


def extract(archive, destination):
    with tarfile.open(archive, "r:xz") as tar:
        members = []
        for member in tar.getmembers():
            path = member_path(member.name)
            if path is None:
                raise SparkleError("The Sparkle archive contains a path outside itself: " + member.name)
            if wanted(path):
                member.name = path
                members.append(member)
        # The "data" filter refuses links and special files that point outside the destination.
        tar.extractall(destination, members=members, filter="data")


def describe(directory, version, sha256):
    return Sparkle(directory, directory / "Sparkle.framework", directory / "bin/sign_update",
                   directory / "bin/generate_appcast", directory / "bin/generate_keys",
                   directory / "LICENSE", version, sha256)


def complete(sparkle):
    framework = sparkle.framework / "Versions/Current"
    return (all(tool.is_file() and os.access(tool, os.X_OK)
                for tool in (sparkle.sign_update, sparkle.generate_appcast, sparkle.generate_keys))
            and sparkle.license.is_file() and (framework / "Sparkle").is_file()
            and (framework / "Autoupdate").is_file() and (framework / "Updater.app").is_dir())


def prepare(root=ROOT, version=VERSION, url=URL, sha256=SHA256):
    """Return the verified, extracted Sparkle. Downloads only when the cache lacks a match."""
    base = Path(root) / ".build/sparkle" / version
    payload = base / "payload"
    marker = payload / "prepared.json"
    wanted_marker = {"version": version, "sha256": sha256}
    if marker.is_file():
        try:
            if json.loads(marker.read_text()) == wanted_marker and complete(describe(payload, version, sha256)):
                return describe(payload, version, sha256)
        except ValueError:
            pass
    base.mkdir(parents=True, exist_ok=True)
    archive = base / f"Sparkle-{version}.tar.xz"
    if archive.exists() and digest(archive) != sha256:
        archive.unlink()  # a damaged or stale cache entry; fetch it again
    if not archive.exists():
        partial = base / (archive.name + ".partial")
        try:
            download(url, partial)
            actual = digest(partial)
            if actual != sha256:
                raise SparkleError(f"Sparkle {version} checksum mismatch: expected {sha256}, got {actual}. "
                                   "The download was not used.")
            os.replace(partial, archive)
        finally:
            partial.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory(prefix=".extract-", dir=base) as scratch:
        staged = Path(scratch) / "payload"
        staged.mkdir()
        try:
            extract(archive, staged)
        except (tarfile.TarError, OSError) as error:
            raise SparkleError(f"Couldn’t unpack the Sparkle archive: {error}") from None
        if not complete(describe(staged, version, sha256)):
            raise SparkleError("The Sparkle archive lacks the framework or tools packaging needs")
        (staged / "prepared.json").write_text(json.dumps(wanted_marker) + "\n")
        shutil.rmtree(payload, ignore_errors=True)
        os.replace(staged, payload)
    return describe(payload, version, sha256)


def main():
    try:
        print(prepare().directory)
    except SparkleError as error:
        sys.exit(f"error: {error}")


if __name__ == "__main__":
    main()
