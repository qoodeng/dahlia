#!/usr/bin/env python3
"""Physical source counts; excludes generated files and unused dependencies."""
from pathlib import Path
import argparse
import json
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("--swift-root", required=True, type=Path)
args = parser.parse_args()
swift = args.swift_root.resolve()
rust = Path(__file__).resolve().parents[1]

def count(lines, files):
    return {
        "files": files,
        "raw_lines": len(lines),
        "nonblank_lines": sum(bool(line.strip()) for line in lines),
        "nonblank_not_comment_only_lines": sum(
            bool(line.strip()) and not line.strip().startswith(("//", "/*", "*", "*/"))
            for line in lines
        ),
    }

def files_count(files):
    files = sorted(set(p for p in files if p.is_file()))
    return count(sum([p.read_text().splitlines() for p in files], []), len(files))

def qa(path):
    return path.name.endswith(("Checks.swift", "Diagnosis.swift")) or path.name == "CurrentPerformance.swift" or "Checks" in path.parent.name

files = list((swift / "Sources").rglob("*.swift"))
used = ["markraft-core", "markraft-commonmark", "markraft-gpui", "markraft-math", "markraft-media", "markraft-syntax"]
crates = rust / "vendor/markraft/crates"
groups = {
    "Swift owned production": [p for p in files if not qa(p)],
    "Swift embedded checks": [p for p in files if qa(p)],
    "Swift linked vendor editor": list((swift / "Vendor/MarkdownEngine").rglob("*.swift")),
    "Swift unlinked vendor reference checkout": list((swift / "Vendor/swift-markdown-engine").rglob("*.swift")),
    "Swift external test fixtures": list((swift / "Tests").rglob("*.swift")),
    "Swift scripts": [p for p in (swift / "Scripts").rglob("*") if p.is_file()],
    "Swift docs": list(swift.glob("*.md")) + list((swift / "Docs").rglob("*.md")),
    "Rust linked Markraft source (includes upstream inline tests)": sum([list((crates / name / "src").rglob("*.rs")) for name in used], []),
    "Rust linked Markraft external tests": sum([list((crates / name / "tests").rglob("*.rs")) for name in used], []),
    "Rust unused Markraft crates (checkout only)": sum([list(p.rglob("*.rs")) for p in crates.iterdir() if p.name not in used], []),
    "Rust scripts": list((rust / "scripts").glob("*")),
    "Rust docs": list(rust.glob("*.md")) + list((rust / "evidence").glob("*.md")),
    "Rust shared comparison fixture": list((rust / "fixtures").glob("*.md")),
}
result = {name: files_count(paths) for name, paths in groups.items()}
production, tests = [], []
source_files = list((rust / "src").glob("*.rs"))
test_files = 0
for path in source_files:
    lines = path.read_text().splitlines()
    boundary = next((i for i, line in enumerate(lines) if line == "#[cfg(test)]"), len(lines))
    test_files += boundary < len(lines)
    production += lines[:boundary]
    tests += lines[boundary:]
result["Rust owned production"] = count(production, len(source_files))
result["Rust inline tests"] = count(tests, test_files)
result["method"] = "Physical lines. Comment-only column is a prefix heuristic, not a language parser; inline comments remain. Rust top-level tests split at cfg(test); test-only helper methods remain in the production physical-line count. Swift QA matches Package.swift exclusions/FindChecks. Generated files and registry dependencies excluded. Markraft src includes its inline tests."
result["swift_commit"] = subprocess.check_output(["git", "-C", str(swift), "rev-parse", "HEAD"], text=True).strip()
result["swift_status"] = subprocess.check_output(["git", "-C", str(swift), "status", "--porcelain"], text=True).strip()
result["rust_scope"] = "Multi-note Classic/Glass prototype, not full Swift parity."
(rust / "evidence/source-counts.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps({name: result[name] for name in ["Swift owned production", "Rust owned production", "Rust inline tests"]}, indent=2))
