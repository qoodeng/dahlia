# Dahlia parity with Swift Ortensia

Reference: Swift `qoodeng/ortensia` main `2ae2c2c251d4f2bc5b918c3b3ae5ab25c8c92229`, read-only. Checked against `Model.swift`, `NoteStyleDefaults.swift`, `NoteWindow.swift`, `NativeCommands.swift`, `NativeMenuBuilder.swift`, `NoteOperations.swift`, and the existing QA checks. This list describes implemented Swift behavior, not a claim that Rust already matches it.

## First milestone, verified

- [x] Reused native Markdown editor, no webview.
- [x] Source-preserving autosave and save/reopen.
- [x] Headings, inline formatting, links, nested/numbered lists, tasks, code, editable tables and local images render.
- [x] Native paste, task toggle, table-cell edit, undo/redo and normal quit observed on macOS.
- [x] Separate Rust data; Swift source, user notes and frozen signed release untouched.

## Current bounded stage

- [x] Multiple notes: stable identities; create/open/close without deleting content.
- [x] One editor/history per open note; close/reopen cannot affect another note's edits or undo.
- [x] Per-note window position/size (storage + restoration tests); restore visible bounds after display changes; preserve expanded size while collapsed.
- [x] Collapse/expand Classic shell without replacing its editor or losing selection/history.
- [x] Per-note float toggle (macOS native), supported native adapter; unsupported platform behavior stays explicit.
- [x] macOS/Linux/Windows path, storage and build configuration. macOS and Linux build, lint and test in CI; Linux has been run on X11 and natively on Wayland. Windows has not been compiled.

Native stage evidence: `evidence/stage2-verification.md`. Selection retention has code continuity but no separate mixed-selection interaction proof.

## Remaining Swift feature parity (historical checklist)

Unchecked items below were open when this list was written; the Mac 1.0 section at the end records what has since been implemented.

- [x] macOS Classic native frame uses installed Stickies' verified style flags 14 expanded / 6 collapsed; custom GPUI perimeter removed. Evidence: `evidence/stickies-native-frame.md`.
- [ ] Exterior border pixel thickness/color/corners and active/inactive screenshot comparison: permitted app snapshots crop the outer frame; no documented full-desktop capture API here. No denied access was bypassed.
- [x] Classic/Glass (plus Dahlia's own Sea Glass), six per-note colors, reading size and live global Glass transparency; macOS HUD/Aqua material, mask strength, 6-point corners and separate 75% translucency. Native evidence in `evidence/stage3-verification.md`; full platform/pixel fidelity is still unverified.
- [x] Minimize/zoom individually and all notes; arrangement by color/content/date/location; undo/redo arrangement and collapse policy (macOS). Linux omits Arrange: GPUI cannot move windows there (see README).
- [ ] Full Use as Default style capture (appearance/color/size/float/translucency/reading captured now; checking propagation remains), new-note cascade, reading size and checking settings (including applying checking defaults to existing open notes).
- [x] Archive and recoverable Trash with restore, Undo/Redo Delete, and Swift's compact native Close choices. Files and identities are retained; full Recovery/checkpoint UI remains queued. See `evidence/stage4-verification.md`.
- [ ] Full transfer parity: UTF-8 Markdown/plain-text source import and exact Markdown export are implemented with linked-file disclosures; portable attachment bundles, additional encodings, batch export/report/rescue flows remain.
- [ ] Managed image paste/drop/import, attachment identity/reuse/limits and ordering; current Rust supports local image display only.
- [ ] Find/replace across notes, source/display mapping, all matching and failure checks.
- [ ] Platform editor integration: spelling/grammar, substitutions, smart editing, transformations, speech/dictation, emoji picker and Services where available.
- [ ] Matched Swift/Rust same-fixture comparison; mixed selection, native IME, save-failure and resize interactions.
- [ ] Recovery/migration/checkpoint/large-notebook behavior and accessible keyboard flow verification.

Target platforms are macOS, Linux and Windows. No universal parity or speed claim. Small coherent stages retain Markraft's editor/source APIs rather than rebuilding an editor.

## Frozen Mac 1.0 implementation

The earlier stage checklist above is historical. `MAC-1.0.md` now defines the frozen required scope. Window bulk operations/arrangement, complete default capture and checking propagation, managed images and portable transfers, Find/Replace across open notes, native prose checking/substitutions, Recovery previews/multi-selection/restore copies and corruption fallback are implemented. Integrated tests and owned-source Clippy pass; CI records the current results. Final native and distribution qualification is recorded separately in `evidence/mac-1.0-verification.md`; optional OS service panels, other-platform runners and exterior pixel measurement remain explicitly unqualified.
