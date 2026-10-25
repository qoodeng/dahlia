#!/usr/bin/env python3
"""Generate locked Mac dependency notices; preserve unresolved attribution as a release blocker."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

VERSION = "cargo-about 0.9.2"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def render(inventory, sources, lock, target):
    resolved = set()
    sections = ["Dahlia — third-party notices\n", "Cargo dependency licenses for " + target + ".\n"]
    for source in sources:
        for file in source.get("files", []):
            if digest(file["text"].encode()) != file["sha256"]:
                raise ValueError("Upstream license checksum differs: " + source["repo"])
        if source.get("files") or source.get("archive"):
            resolved.update(source["crates"])
    missing = set()
    for group in inventory["licenses"]:
        users = group["used_by"]
        sections += ["\n" + "=" * 78 + "\n", group["name"] + "\n"]
        for user in users:
            crate = user["crate"]
            name = crate["name"] + "@" + crate["version"]
            sections.append(name + "\n")
            sections.append("Authors: " + "; ".join(crate.get("authors", [])) + "\n")
            sections.append("Source: " + (crate.get("repository") or
                                          "https://crates.io/crates/" + crate["name"] + "/" + crate["version"]) + "\n")
            # Markraft's pinned workspace license is separately included in the bundle.
            if not group.get("source_path") and name not in resolved and not (crate.get("source") is None and crate["name"] in {
                    "markraft-core", "markraft-commonmark", "markraft-gpui", "markraft-math", "markraft-media", "markraft-syntax"}):
                missing.add(name)
        sections += ["\n", group["text"], "\n"]
    sections.append("\nRecovered upstream copyright and license notices\n")
    emitted = set()
    for source in sources:
        sections.append("\nApplies to: " + ", ".join(source["crates"]) + "\n")
        if source.get("archive"):
            archive = source["archive"]
            sections.append("Original complete published source: " + archive["path"] + "\n")
            sections.append("Registry source: " + archive["url"] + "\nSHA-256: " + archive["sha256"] + "\n")
        for file in source.get("files", []):
            sections.append("Source: " + file["url"] + "\n")
            if file["sha256"] not in emitted:
                sections += [file["text"], "\n"]
                emitted.add(file["sha256"])
    if missing:
        sections.append("\nUnresolved attribution sources (release is blocked):\n" + "\n".join(sorted(missing)) + "\n")
    text = "".join(sections).encode()
    manifest = {"generator": VERSION, "target": target, "cargo_lock_sha256": digest(lock),
                "notices_sha256": digest(text), "upstream_sha256": digest(json.dumps(sources, sort_keys=True).encode()),
                "unresolved_attribution": sorted(missing),
                "source_archives": [source["archive"] for source in sources if source.get("archive")]}
    return text, manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=["aarch64-apple-darwin", "x86_64-apple-darwin"])
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    version = subprocess.check_output(["cargo", "about", "--version"], cwd=root, text=True).strip()
    if version != VERSION:
        parser.error("Install cargo-about 0.9.2 with --locked --features cli")
    with tempfile.TemporaryDirectory() as temporary:
        inventory = Path(temporary) / "licenses.json"
        subprocess.run(["cargo", "about", "generate", "--locked", "--target", args.target,
                        "--format", "json", "--fail", "--output-file", inventory], cwd=root, check=True)
        sources = json.loads((root / "assets/licenses/upstream.json").read_text())
        for source in sources:
            if source.get("archive"):
                archive = source["archive"]
                if digest((root / "assets/licenses" / archive["path"]).read_bytes()) != archive["sha256"]:
                    raise ValueError("Original registry source checksum differs: " + archive["path"])
        text, manifest = render(json.loads(inventory.read_text()), sources, (root / "Cargo.lock").read_bytes(), args.target)
    destination = root / "assets/licenses"
    (destination / "THIRD-PARTY-NOTICES.txt").write_bytes(text)
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print("Notices written. Unresolved attribution:", ", ".join(manifest["unresolved_attribution"]) or "none")


if __name__ == "__main__":
    main()
