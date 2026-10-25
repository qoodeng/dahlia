#!/usr/bin/env python3
"""Prepare pinned dependencies with maintained clock and token-pasting crates."""
import importlib.util
import sys
from pathlib import Path

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
PACKAGES = (
    ("gpui-base", "0.6.6", "c9e904ee0d351f4e85b967e26ae986d3aa195a8d9d35fcd18184a2c40989694e"),
    ("metal", "0.33.0", "c7047791b5bc903b8cd963014b355f71dc9864a9a0b727057676c1dcae5cbc15"),
    ("pulp", "0.22.3", "046aa45b989642ec2e4717c8e72d677b13edd831a4d3b6cf37d9a3e54912496a"),
)


def main():
    # Reuse the pinned Markraft preparer's checksum, extraction and atomic-publication checks.
    spec = importlib.util.spec_from_file_location(
        "markraft_prepare", ROOT / "vendor/markraft/scripts/prepare-dependencies.py"
    )
    prepare = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(prepare)
    for name, version, checksum in PACKAGES:
        prepare.CRATE = name
        prepare.VERSION = version
        prepare.ARCHIVE_NAME = f"{name}-{version}.crate"
        prepare.ARCHIVE_URL = f"https://static.crates.io/crates/{name}/" + prepare.ARCHIVE_NAME
        prepare.ARCHIVE_SHA256 = checksum
        print(f"Prepared {name}:", prepare.prepare(ROOT))


if __name__ == "__main__":
    main()
