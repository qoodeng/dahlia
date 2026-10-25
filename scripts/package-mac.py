#!/usr/bin/env python3
"""Mac-only packaging. Uses fresh committed source, never an existing binary."""
import argparse
import base64
import binascii
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile


class PackagingError(Exception):
    pass


FEED_URL = "https://github.com/qoodeng/dahlia/releases/latest/download/appcast.xml"
SPARKLE_KEY_FILE = "scripts/sparkle-public-key.txt"
# Not a Sparkle key: a clearly fake 32 bytes so a --dry-run bundle has a well-formed plist.
DRY_RUN_PUBLIC_KEY = base64.b64encode(b"dry-run-NOT-A-REAL-SPARKLE-KEY!!").decode()


def _load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SPARKLE = _load("prepare-sparkle")
prepare_sparkle = SPARKLE.prepare


# Releases are dated: YYYY.MM.DDNN, where NN (01-99) counts that day's releases. The same string
# is CFBundleShortVersionString and CFBundleVersion, so Sparkle can compare it. Cargo's own
# version is internal and never reaches the bundle.
VERSION_PATTERN = re.compile(r"([0-9]{4})\.([0-9]{2})\.([0-9]{2})([0-9]{2})")


def validate_version(value):
    """Return `value` if it is a zero-padded YYYY.MM.DDNN release version, else raise."""
    match = VERSION_PATTERN.fullmatch(value) if isinstance(value, str) else None
    if match is None:
        raise PackagingError(f"Version {value!r} must look like YYYY.MM.DDNN, for example 2026.10.2501")
    year, month, day, number = (int(part) for part in match.groups())
    try:
        datetime.date(year, month, day)
    except ValueError:
        raise PackagingError(f"Version {value!r} does not start with a real calendar date") from None
    if not 1 <= number <= 99:
        raise PackagingError(f"Version {value!r} must end with a release number from 01 to 99")
    return value


def version_argument(value):
    try:
        return validate_version(value)
    except PackagingError as error:
        raise argparse.ArgumentTypeError(str(error)) from None


def run(*args, cwd=None, env=None, evidence=None, combined=False):
    result = subprocess.run([str(a) for a in args], cwd=cwd, env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if evidence is not None:
        with evidence.open("ab") as log:
            log.write(("$ " + " ".join(str(a) for a in args) + "\n").encode())
            log.write(result.stdout + result.stderr + b"\n")
    if result.returncode:
        raise PackagingError(f"Command failed ({result.returncode}): {args[0]}\n"
                             + result.stderr.decode(errors="replace")
                             + result.stdout.decode(errors="replace"))
    return result.stdout + result.stderr if combined else result.stdout


def text(*args, **kwargs):
    return run(*args, **kwargs).decode().strip()


def sha256(path):
    with path.open("rb") as stream:
        digest = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
        return digest.hexdigest()


def manifest(root):
    """Include bytes, symlinks and modes; timestamps/xattrs aren't stable copies."""
    result = {}
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            result[relative] = {"symlink": os.readlink(path)}
        elif path.is_file():
            result[relative] = {"sha256": sha256(path), "mode": path.stat().st_mode & 0o777}
    return result


def sparkle_public_key(root):
    """The committed Ed25519 update-signing key. Refuses a missing file or a placeholder."""
    path = root / SPARKLE_KEY_FILE
    if not path.is_file():
        raise PackagingError(f"{SPARKLE_KEY_FILE} is missing. Run Sparkle's generate_keys once and commit "
                             "the public key it prints (see RELEASING.md).")
    lines = [line.strip() for line in path.read_text().splitlines()
             if line.strip() and not line.strip().startswith("#")]
    problem = f"{SPARKLE_KEY_FILE} has no real public key yet. Run Sparkle's generate_keys once and commit the key it prints (see RELEASING.md)."
    if len(lines) != 1:
        raise PackagingError(problem)
    try:
        raw = base64.b64decode(lines[0], validate=True)
    except (binascii.Error, ValueError):
        raise PackagingError(problem) from None
    if len(raw) != 32 or not any(raw) or lines[0] == DRY_RUN_PUBLIC_KEY:
        raise PackagingError(problem)
    return lines[0]


def clean_revision(root, expected):
    actual = text("git", "rev-parse", "HEAD", cwd=root)
    if actual != expected:
        raise PackagingError(f"HEAD is {actual}, expected {expected}")
    dirty = text("git", "status", "--porcelain", "--untracked-files=all",
                 "--ignore-submodules=dirty", cwd=root)
    if dirty:
        raise PackagingError("Commit or preserve working changes before packaging:\n" + dirty)
    return actual


def metadata(source, env):
    data = json.loads(run("cargo", "metadata", "--locked", "--no-deps", "--format-version", "1",
                          cwd=source, env=env))
    packages = [p for p in data["packages"]
                if Path(p["manifest_path"]).resolve() == source / "Cargo.toml"]
    if len(packages) != 1:
        raise PackagingError("Expected one root Cargo package")
    package = packages[0]
    bins = [t["name"] for t in package["targets"] if "bin" in t["kind"]]
    binary = package["name"] if package["name"] in bins else bins[0] if len(bins) == 1 else None
    if binary is None or not re.fullmatch(r"[A-Za-z0-9_-]+", binary):
        raise PackagingError("Cannot identify one application binary from Cargo metadata")
    return package["name"], binary


def verify_licenses(source, host):
    directory = source / "assets/licenses"
    record = json.loads((directory / "manifest.json").read_text())
    notices = directory / "THIRD-PARTY-NOTICES.txt"
    if record.get("generator") != "cargo-about 0.9.2" or record.get("target") != host:
        raise PackagingError("Regenerate third-party notices for this Mac target")
    if record.get("cargo_lock_sha256") != sha256(source / "Cargo.lock"):
        raise PackagingError("Third-party notices do not match Cargo.lock; regenerate them")
    if record.get("notices_sha256") != sha256(notices):
        raise PackagingError("Third-party notice checksum differs")
    upstream = json.loads((directory / "upstream.json").read_text())
    actual = hashlib.sha256(json.dumps(upstream, sort_keys=True).encode()).hexdigest()
    if record.get("upstream_sha256") != actual:
        raise PackagingError("Upstream license provenance checksum differs")
    if record.get("source_archives", []) != [source["archive"] for source in upstream if source.get("archive")]:
        raise PackagingError("Third-party source archive manifest differs")
    for archive in record.get("source_archives", []):
        name = archive.get("path", "")
        if not re.fullmatch(r"third-party-sources/[A-Za-z0-9_.-]+\.crate", name):
            raise PackagingError("Invalid third-party source archive path")
        if sha256(directory / name) != archive.get("sha256"):
            raise PackagingError("Original registry source checksum differs: " + name)
    if record.get("unresolved_attribution"):
        raise PackagingError("Unresolved third-party attribution: " + ", ".join(record["unresolved_attribution"]))
    return record


def bundle(source, binary_path, app, binary, version, revision, sparkle, public_key):
    contents = app / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    resources = contents / "Resources"
    resources.mkdir()
    shutil.copy2(binary_path, contents / "MacOS" / binary)
    shutil.copy2(source / "assets/brand/dahlia.icns", resources / "dahlia.icns")
    for filename in ("PaperMono-Regular.otf", "PaperMono-Bold.otf", "provenance.json"):
        shutil.copy2(source / "assets/fonts" / filename, resources / filename)
    provenance = json.loads((source / "assets/fonts/provenance.json").read_text())
    for filename, digest in provenance["sha256"].items():
        if Path(filename).name != filename or sha256(resources / filename) != digest:
            raise PackagingError("Font provenance mismatch: " + filename)
    for original, name in (("LICENSE", "Dahlia-LICENSE.txt"),
                           ("vendor/markraft/LICENSE", "Markraft-LICENSE.txt"),
                           ("assets/fonts/LICENSE.txt", "PaperMono-LICENSE.txt")):
        shutil.copy2(source / original, resources / name)
    frameworks = contents / "Frameworks"
    frameworks.mkdir()
    # Whole, with its links: Sparkle's versioned layout and its XPC services, Autoupdate and Updater.app.
    shutil.copytree(sparkle.framework, frameworks / "Sparkle.framework", symlinks=True)
    shutil.copy2(sparkle.license, resources / "Sparkle-LICENSE.txt")
    template = source / "assets/Info.plist"
    info = plistlib.loads(template.read_bytes()) if template.exists() else {
        "CFBundleIdentifier": "dev.ortensia.rust", "LSMinimumSystemVersion": "12.0",
        "NSHighResolutionCapable": True, "NSPrincipalClass": "NSApplication"}
    info.setdefault("LSMinimumSystemVersion", "12.0")
    info.update(CFBundleName="Dahlia", CFBundleDisplayName="Dahlia",
                CFBundleExecutable=binary, CFBundleIconFile="dahlia.icns",
                CFBundlePackageType="APPL", CFBundleShortVersionString=version,
                CFBundleVersion=version, DahliaSourceRevision=revision,
                SUFeedURL=FEED_URL, SUPublicEDKey=public_key, SUEnableAutomaticChecks=True,
                SUScheduledCheckInterval=86400, SUAutomaticallyUpdate=False)
    if not re.fullmatch(r"[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+", info.get("CFBundleIdentifier", "")):
        raise PackagingError("Invalid bundle identifier")
    (contents / "Info.plist").write_bytes(plistlib.dumps(info))
    (contents / "PkgInfo").write_bytes(b"APPL????")
    for name in ("THIRD-PARTY-NOTICES.txt", "manifest.json", "upstream.json"):
        shutil.copy2(source / "assets/licenses" / name, resources / name)
    license_record = json.loads((source / "assets/licenses/manifest.json").read_text())
    for archive in license_record.get("source_archives", []):
        target = resources / archive["path"]
        target.parent.mkdir(exist_ok=True)
        shutil.copy2(source / "assets/licenses" / archive["path"], target)
    (resources / "build-commit.txt").write_text(revision + "\n")
    return info


def codesign(path, identity, env, runtime=True, preserve_entitlements=False):
    """Sign with `identity`; "-" is an ad-hoc signature for --dry-run, which skips Apple's timestamp server."""
    command = ["codesign", "--force"]
    if runtime:
        command += ["--options", "runtime"]
    if identity != "-":
        command.append("--timestamp")
    if preserve_entitlements:
        command.append("--preserve-metadata=entitlements")
    run(*command, "--sign", identity, path, env=env)


def sparkle_code(app):
    """Sparkle's nested code, innermost first, then the framework that contains it."""
    framework = app / "Contents/Frameworks/Sparkle.framework"
    current = (framework / "Versions/Current").resolve()
    services = sorted((current / "XPCServices").glob("*.xpc"))
    nested = [(service, True) for service in services] + [(current / "Autoupdate", False),
                                                          (current / "Updater.app", False)]
    if not services or not all(path.exists() for path, _ in nested):
        raise PackagingError("Sparkle.framework lacks its XPC services, Autoupdate or Updater.app")
    return nested, framework


def sign_bundle(app, identity, env):
    """Sparkle's documented order for a non-sandboxed app, with no --deep: each helper, the
    framework, then the app, so every outer signature seals the inner ones."""
    nested, framework = sparkle_code(app)
    for path, entitlements in nested:
        codesign(path, identity, env, preserve_entitlements=entitlements)
    codesign(framework, identity, env)
    codesign(app, identity, env)


def notarize(path, label, output, profile, env):
    result = json.loads(run("xcrun", "notarytool", "submit", path, "--keychain-profile", profile,
                            "--wait", "--timeout", "60m", "--output-format", "json", env=env,
                            evidence=output / f"{label}-notary-command.txt"))
    (output / f"{label}-notary.json").write_text(json.dumps(result, indent=2) + "\n")
    if result.get("status") != "Accepted" or not result.get("id"):
        raise PackagingError(f"Apple did not accept {label}; see {label}-notary.json")
    log = run("xcrun", "notarytool", "log", result["id"], "--keychain-profile", profile, env=env)
    (output / f"{label}-notary-log.json").write_bytes(log)
    return result["id"]


def verify_app(app, identity, team, output, label, env):
    evidence = output / f"{label}-verification.txt"
    for command in (("codesign", "--verify", "--deep", "--strict", "--verbose=2", app),
                    ("xcrun", "stapler", "validate", app),
                    ("spctl", "--assess", "--type", "execute", "--verbose=4", app)):
        run(*command, env=env, evidence=evidence)
    signature = run("codesign", "--display", "--verbose=4", app, env=env, combined=True).decode()
    if (f"Authority={identity}" not in signature or f"TeamIdentifier={team}" not in signature
            or not re.search(r"flags=.*\bruntime\b", signature)):
        raise PackagingError("Signature lacks the requested identity, team or hardened runtime")
    with evidence.open("a") as log:
        log.write(signature)
    nested, framework = sparkle_code(app)
    run("codesign", "--verify", "--strict", "--verbose=2", framework, env=env, evidence=evidence)
    for path, _ in nested + [(framework, False)]:
        signature = run("codesign", "--display", "--verbose=4", path, env=env, combined=True).decode()
        if (f"Authority={identity}" not in signature or f"TeamIdentifier={team}" not in signature
                or not re.search(r"flags=.*\bruntime\b", signature)):
            raise PackagingError(f"Sparkle’s {path.name} lacks the requested identity, team or hardened runtime")
        with evidence.open("a") as log:
            log.write(signature)


def verify_adhoc(app, output, env):
    """--dry-run: the ad-hoc seal must hold over everything, Sparkle included. Nothing is notarized."""
    run("codesign", "--verify", "--deep", "--strict", "--verbose=2", app, env=env,
        evidence=output / "dry-run-verification.txt")


def package(args, root):
    if platform.system() != "Darwin":
        raise PackagingError("Packaging must run on the user's Mac")
    version = validate_version(args.version)
    if not re.fullmatch(r"[0-9a-f]{40}", args.expected_commit):
        raise PackagingError("--expected-commit must be the full 40-character Git SHA")
    dry_run = args.dry_run
    match = re.fullmatch(r"Developer ID Application: .+ \(([A-Z0-9]{10})\)", args.identity or "")
    if not dry_run and (not match or not (args.notary_profile or "").strip()):
        raise PackagingError("Provide a Developer ID Application identity and a nonempty notary profile")
    # A dry run is for inspection: ad-hoc signed, never sent to Apple, and allowed a stand-in key.
    signer = "-" if dry_run else args.identity
    team = match[1] if match else None
    try:
        public_key = sparkle_public_key(root)
    except PackagingError:
        if not dry_run:
            raise
        public_key = DRY_RUN_PUBLIC_KEY
    revision = clean_revision(root, args.expected_commit)
    output = Path(args.output).expanduser().resolve()
    if output == root or root in output.parents:
        raise PackagingError("--output must be a new directory outside the source checkout")
    if output.exists():
        raise PackagingError("Output already exists; choose a new directory (nothing is overwritten)")
    env = os.environ.copy()
    for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER",
                 "RUSTC_WORKSPACE_WRAPPER", "RUSTUP_TOOLCHAIN", "CARGO_BUILD_TARGET"):
        if env.get(name):
            raise PackagingError(f"Unset build override {name} before packaging")
    env["DEVELOPER_DIR"] = env.get("DEVELOPER_DIR", "/Applications/Xcode.app/Contents/Developer")
    env["CARGO_INCREMENTAL"] = "0"
    if not dry_run:
        identities = text("security", "find-identity", "-v", "-p", "codesigning", env=env)
        if f'"{args.identity}"' not in identities:
            raise PackagingError("Requested signing identity is unavailable in the keychain")
        # Fail before compiling if credentials are missing/expired. No credential values enter the script.
        run("xcrun", "notarytool", "history", "--keychain-profile", args.notary_profile,
            "--output-format", "json", env=env)
    # Cached under the real checkout, not the throwaway clone, so it is fetched once.
    sparkle = prepare_sparkle(root)
    output.mkdir(parents=True)
    with tempfile.TemporaryDirectory(prefix=".work-", dir=output) as scratch:
        work = Path(scratch)
        source = work / "source"
        run("git", "clone", "--quiet", "--no-hardlinks", "--no-checkout", root, source)
        run("git", "checkout", "--quiet", "--detach", revision, cwd=source)
        # Clone only committed submodule objects, never its patched working files.
        submodule = root / "vendor/markraft"
        if not (submodule / "LICENSE").exists():
            raise PackagingError("Initialize vendor/markraft with scripts/prepare.sh first")
        run("git", "config", "submodule.vendor/markraft.url", submodule, cwd=source)
        run("git", "-c", "protocol.file.allow=always", "submodule", "update", "--init",
            "vendor/markraft", cwd=source)
        run("sh", "scripts/prepare.sh", cwd=source, env=env)
        run("sh", "scripts/check-patch-drift.sh", cwd=source, env=env)
        env["CARGO_TARGET_DIR"] = str(work / "target")
        package_name, binary = metadata(source, env)
        info_path = source / "assets/Info.plist"
        info = plistlib.loads(info_path.read_bytes()) if info_path.exists() else {}
        minimum_os = info.get("LSMinimumSystemVersion", "12.0")
        if not re.fullmatch(r"\d+\.\d+(?:\.\d+)?", minimum_os):
            raise PackagingError("Invalid LSMinimumSystemVersion")
        env["MACOSX_DEPLOYMENT_TARGET"] = minimum_os
        rust = text("rustc", "-Vv", cwd=source, env=env)
        host = next(line[6:] for line in rust.splitlines() if line.startswith("host: "))
        if host not in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
            raise PackagingError("Rust host must be an Apple Silicon or Intel Mac")
        licenses = verify_licenses(source, host)
        print(f"Building {package_name} {version} from {revision}", flush=True)
        run("cargo", "build", "--locked", "--release", "--target", host,
            "--package", package_name, "--bin", binary, "-j2", cwd=source, env=env)
        run("sh", "scripts/check-patch-drift.sh", cwd=source, env=env)
        clean_revision(source, revision)
        built = Path(env["CARGO_TARGET_DIR"]) / host / "release" / binary
        arch = "arm64" if host.startswith("aarch64") else "x86_64"
        if text("lipo", "-archs", built, env=env) != arch:
            raise PackagingError("Built binary architecture differs from the Mac host")
        links = text("otool", "-L", built, env=env).splitlines()[1:]
        if any(not line.strip().startswith(("/usr/lib/", "/System/Library/")) for line in links):
            raise PackagingError("Non-system dynamic dependency needs explicit bundling:\n" + "\n".join(links))
        stage = work / "payload"
        app = stage / "Dahlia.app"
        sparkle_archs = text("lipo", "-archs", sparkle.framework / "Versions/Current/Sparkle", env=env).split()
        if arch not in sparkle_archs:
            raise PackagingError(f"Sparkle.framework has no {arch} code ({' '.join(sparkle_archs)})")
        info = bundle(source, built, app, binary, version, revision, sparkle, public_key)
        sign_bundle(app, signer, env)
        app_id = dmg_id = None
        if dry_run:
            verify_adhoc(app, output, env)
        else:
            zip_path = work / "Dahlia.zip"
            run("ditto", "-c", "-k", "--keepParent", app, zip_path, env=env)
            app_id = notarize(zip_path, "app", output, args.notary_profile, env)
            run("xcrun", "stapler", "staple", app, env=env)
            verify_app(app, args.identity, team, output, "app", env)
        expected_payload = manifest(app)
        (stage / "Applications").symlink_to("/Applications")
        dmg = output / f"Dahlia-{version}.dmg"
        run("hdiutil", "create", "-format", "UDZO", "-volname", "Dahlia", "-srcfolder", stage,
            "-ov", dmg, env=env)
        if not dry_run:
            codesign(dmg, args.identity, env, runtime=False)
            dmg_id = notarize(dmg, "dmg", output, args.notary_profile, env)
            run("xcrun", "stapler", "staple", dmg, env=env)
            for command in (("codesign", "--verify", "--strict", "--verbose=2", dmg),
                            ("xcrun", "stapler", "validate", dmg),
                            ("spctl", "--assess", "--type", "open", "--context", "context:primary-signature",
                             "--verbose=4", dmg)):
                run(*command, env=env, evidence=output / "dmg-verification.txt")
            dmg_signature = run("codesign", "--display", "--verbose=4", dmg,
                                env=env, combined=True).decode()
            if f"Authority={args.identity}" not in dmg_signature or f"TeamIdentifier={team}" not in dmg_signature:
                raise PackagingError("DMG signature differs from the requested identity/team")
            with (output / "dmg-verification.txt").open("a") as log:
                log.write(dmg_signature)
        mounted = work / "mounted"
        mounted.mkdir()
        run("hdiutil", "attach", "-readonly", "-nobrowse", "-noautoopen", "-mountpoint", mounted,
            dmg, env=env)
        try:
            if sorted(p.name for p in mounted.iterdir() if p.name != ".DS_Store") != ["Applications", "Dahlia.app"]:
                raise PackagingError("Unexpected disk image payload")
            if not (mounted / "Applications").is_symlink() or os.readlink(mounted / "Applications") != "/Applications":
                raise PackagingError("Disk image Applications link differs")
            payload = mounted / "Dahlia.app"
            if dry_run:
                verify_adhoc(payload, output, env)
            else:
                verify_app(payload, args.identity, team, output, "mounted-app", env)
            if manifest(payload) != expected_payload:
                raise PackagingError("Mounted app differs from the accepted, stapled app")
        finally:
            run("hdiutil", "detach", mounted, env=env)
        # Written only after every Apple and mounted-payload verification has succeeded.
        receipt = {"source_revision": revision, "markraft_revision": text("git", "-C", "vendor/markraft", "rev-parse", "HEAD", cwd=source),
                   "cargo_lock_sha256": sha256(source / "Cargo.lock"), "rustc": rust,
                   "third_party_notices_sha256": licenses["notices_sha256"],
                   "cargo": text("cargo", "-V", cwd=source, env=env), "host": host,
                   "xcode": text("xcodebuild", "-version", env=env),
                   "sdk": text("xcrun", "--sdk", "macosx", "--show-sdk-version", env=env),
                   "bundle_identifier": info["CFBundleIdentifier"], "version": version,
                   "sparkle_version": sparkle.version, "sparkle_sha256": sparkle.sha256,
                   "identity": args.identity, "team": team,
                   "app_submission_id": app_id, "dmg_submission_id": dmg_id,
                   "dmg": dmg.name, "dmg_sha256": sha256(dmg), "app_manifest": expected_payload,
                   "qualification": "signature, notarization, tickets and mounted bytes verified; GUI acceptance remains separate"}
        if dry_run:
            # Deliberately not receipt.json, and no SHA256SUMS: nothing here may be mistaken for a release.
            receipt.update(dry_run=True, identity=None, team=None, public_key=public_key,
                           qualification="DRY RUN: ad-hoc signed, not notarized, not for distribution")
            (output / "dry-run-receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
            print(f"Dry-run bundle (ad-hoc signed, NOT notarized): {dmg}\n"
                  f"Receipt: {output / 'dry-run-receipt.json'}", flush=True)
            return
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        (output / "SHA256SUMS").write_text(receipt["dmg_sha256"] + "  " + dmg.name + "\n")
        print(f"Verified installer: {dmg}\nReceipt: {output / 'receipt.json'}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--identity", help="Exact Developer ID Application keychain identity (required unless --dry-run)")
    parser.add_argument("--notary-profile", help="Existing notarytool keychain profile name (required unless --dry-run)")
    parser.add_argument("--expected-commit", required=True, help="Full reviewed HEAD SHA; dirty checkouts fail")
    parser.add_argument("--version", required=True, type=version_argument,
                        help="Release version YYYY.MM.DDNN (NN is 01-99); CFBundleShortVersionString and CFBundleVersion")
    parser.add_argument("--output", required=True, help="New output directory outside the checkout")
    parser.add_argument("--dry-run", action="store_true",
                        help="Build and bundle for inspection: ad-hoc signed, never sent to Apple, no receipt.json")
    args = parser.parse_args()
    if not args.dry_run and not (args.identity and args.notary_profile):
        parser.error("--identity and --notary-profile are required unless --dry-run")
    try:
        package(args, Path(__file__).resolve().parent.parent)
    except (PackagingError, OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"error: {error}\nNo successful receipt was issued; do not distribute partial output.\n")


if __name__ == "__main__":
    main()
