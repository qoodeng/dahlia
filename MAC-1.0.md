# Historical Mac 1.0 release contract

This contract records the earlier 1.0 scope and historical qualification; its optional-performance exceptions do not relax the current A+ gates. See [HANDOFF.md](HANDOFF.md) for current source and release status.

Product name: Dahlia (original working name: ortensia-rust). Separate native Rust/GPUI app and storage beside the unchanged Swift release. No further development archive is a deliverable. Required scope was frozen by the parent delegation before parallel implementation.

## Required functionality

- Multiple editable Markdown notes, exact source-preserving autosave/undo, safe close/quit/save failures, isolated persistent geometry and advisory locking.
- Classic using installed Stickies' AppKit window mechanism: style 14 expanded, 6 collapsed, native background color and no added macOS perimeter; Glass, colors, shared transparency and per-note translucency.
- Collapse/expand, float, minimize/zoom individually and all notes; arrange by color/content/date/location, collapse policy, arrangement undo/redo.
- New-note cascade, complete implemented style/default capture, persisted checking settings and propagation of checking defaults to other open notes.
- Archive/Trash/restore/delete undo, Recovery with previews, multi-selection, immutable checkpoints/source history, corrupt-index fallback, safe restore-as-copy and exact rescue exports.
- Managed image file paste/drop/import, order and deduplication, bounds/symlink safety, portable Markdown image export and batch reports; UTF-8/UTF-16 and folder imports. Native ImageIO supports macOS-only input formats such as HEIC.
- Find/Replace in the current note and across open notes (same scope as Swift controllers), next/previous/selection search, all replacements with preflight validation and a durable recovery snapshot; independent editor undo remains intact.
- Native prose checking and guarded substitutions, excluding code/markup/IME; keyboard and source-preservation workflow checks.
- One final signed/notarized Mac installer with the existing hydrangea icon, strict signature/ticket/Gatekeeper checks, private GitHub source backup and final Library deliverable.

## Qualification and optional extras

Linux/Windows native builds, performance benchmarking, exterior screenshot pixel measurement, native system spelling/substitution panels, rich Data Detector action menus, dictation/speech/emoji panels and additional rich-text transfer formats are not new gates for this frozen Mac release. No universal parity, faster-performance or numerical exterior border claim will be made. Other-platform Classic keeps a provisional outline.

## Current verification

Integrated tests pass; owned-source Clippy passes with warnings denied. CI records the current results. Native HEIC normalization passes outside the shell sandbox both for a generated 128×128 image and the public libheif example. Normal sandbox tests exercise native PNG/header bounds and explicitly report unavailable HEIC fixture encoding. Native Recovery, image import/export and folder round trip, defaults/checking propagation, arrangement/collapse, prose correction and save-failure retry have been exercised; detailed evidence and explicit gesture/IME qualification limits are in `evidence/mac-1.0-verification.md`. Apple accepted the app and exact outer DMG. App and separately named installer pass strict signature, stapled-ticket and Gatekeeper checks. Original Library delivery is retained unchanged; its Apple acceptance is recorded in the release receipt.

Operational image bounds: 16 MiB input/normalized output, 32 megapixels, 16,000 pixels per side; each cohort 64 images/64 MiB. Source imports: 10 MiB each, 256 files/256 MiB per batch. Recovery previews show up to 12,000 characters while exports/restores retain the full source. Source history and published library checkpoints retain twenty entries; immutable attachment bytes remain available for undo/recovery.
