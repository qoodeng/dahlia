# Multi-note Classic milestone — 2026-10-14

Workspace `/Users/christianstephens/Documents/Codex/2026-10-14/task-3`; Apple Silicon macOS 26.5.2. Swift reference commit `2ae2c2c251d4f2bc5b918c3b3ae5ab25c8c92229` remains clean/read-only. No Swift packaging or production data changed. Markraft stays pinned at `d2ff6779e6098add5a1c2d09a3e09f102e5ca628`.

## Final checks

- `cargo build --locked --offline`: success, native linked debug executable; command-scoped Xcode path only.
- `cargo test --locked --offline`: **5 passed, 0 failed**. Exact source and front matter/comment preservation, pre-existing external edits, lock release/reopen, legacy path preservation, note identities and metadata, per-window editor undo/write independence, IME candidate exclusion/cancel and restoration of corrupt/offscreen/removed-display bounds.
- `cargo clippy --locked --offline --all-targets --no-deps -- -D warnings`: success for owned source. Upstream patched GPUI still emits deprecation warnings.
- `cargo fmt --all -- --check` and `git diff --check`: success.

## Native smoke evidence

Using the existing CUA app handle (no rebind), Cmd-N created a second independent note. Pasted `Second note — independent history.`; Cmd-Z removed it, Cmd-Shift-Z restored it and autosave updated its Markdown. First note remained unchanged. Collapse produced the 12-point strip, showed the title and controls, and expansion retained the editor text. Window > Float on Top persisted `floating:true`. Cmd-W hid the second note without deleting its file and left the first open. Window > its title reopened saved text. Normal Cmd-Q completed with process exit code 0.

The final binary restored note state and content after relaunch. A native smoke check exposed GPUI 0.3.6's app-menu targeting gap for NSPanel-based Floating windows. Host now creates GPUI Normal windows and applies macOS floating level through public AppKit. The final collapsed floating window's Collapse / Expand, Zoom and Float on Top menu items were enabled; menu expansion and Cmd-W succeeded. Shell focus is retained when the editor is hidden. Final normal quit returned 0.

Captures: `stage2-classic-final.png` (final binary), `stage2-classic-collapsed.png` and `stage2-second-reopened.png` (same appearance, earlier stage binary). `stage2-note-state.json` records the final synthetic metadata; fixture content itself is ignored. `classic-comparison.html` displays raw Swift and Rust captures side by side. Swift reference capture predates some source changes; contents and height differ, so this is shell evidence rather than full pixel parity.

Classic source match: paper FEF49C, spine FEEA3D, 12-point strip, 8-point glyph paths, 20-point targets, left/right offsets 6/6/28, hidden expanded title and visible collapsed title; no extra app label/toolbar. Text uses Helvetica Neue 12 with heading scale and 10/4 insets. Markraft's document spacing still differs from Swift.

## Limits and next bounded work

- Exact **installed Apple Stickies exterior border** remains unmeasured: permitted app screenshots omit it; no documented full-desktop capture route here. The screen-context tool is voice-only and is unavailable for this text task. No denied route was bypassed. Current 1-point black perimeter follows Swift source only. See `stickies-observation.md`.
- macOS only built/run. Linux X11/Wayland build features and Linux/Windows data paths/storage cfg exist; native compilation, fonts, collapse/window behavior and packaging there require native runners. Float toggle is macOS-only. Windows does not use the Unix parent-directory sync step.
- Closed notes are retained, not archived. Glass, appearance/defaults/transparency, archive/trash/restore/recovery, import/export/managed attachments, arrangement, find/replace and system editor services remain. `PARITY.md` inventories Swift's actual features.
- No separate mixed-selection retention, native IME keyboard, manual drag/resize, interactive save-failure or display hot-plug smoke test yet. Restoration and committed-source behavior have focused tests; this is not equivalent to all those native interactions. No speed claim.

Git remote configuration at completion: **none** (`git remote -v` empty). Local commit is the publishing starting point; no GitHub repository was invented or pushed.
