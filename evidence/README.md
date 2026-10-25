# Evidence

Dated verification records and the captures they cite. They are historical: each describes the commit and build named inside it, not current `main`. Current status is in [HANDOFF.md](../HANDOFF.md), and CI records current test results.

Files from before the rename still say `ortensia-rust` (the repository, binary, bundle `dev.ortensia.rust` and data folder of the time) or "Ortensia" for the Rust prototype. They are left as recorded. Today the app is Dahlia: package and executable `dahlia`, bundle `dev.qoodeng.dahlia`, data folder `Dahlia`. "Swift Ortensia" is the separate Swift app that Dahlia is compared with.

| Record | What it covers |
|---|---|
| `stickies-*.md`, `stickies-*.png/json/plist`, `inspect-stickies-nib.py` | Measurements of installed Apple Stickies, the reference for the Classic shell and strip glyphs. |
| `stage2-…`, `stage3-…`, `stage4-…` | Staged prototype verification: shell, appearance, close/trash. |
| `mac-1.0-*`, `mac-package-verification.md`, `mac-build-manifest.json` | Mac 1.0 qualification, packaging and notarization receipts. |
| `dahlia-ui-fixes.md` | 1.0.1 UI fixes and the Stickies control measurements. |
| `*-comparison.*`, `swift-*-reference.png`, `rust-*.png` | Swift and Rust visual comparisons. |
| `source-counts.json`, `verification.md`, `first-note-saved.md`, `comparison-saved.md`, `editor-comparison.md` | Early milestone records. |
