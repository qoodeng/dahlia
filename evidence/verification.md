# First-note milestone verification

2026-10-14 on Apple Silicon macOS 26.5.2. Workspace: `/Users/christianstephens/Documents/Codex/2026-10-14/task-3`.

- Native `cargo build --locked --offline` linked successfully with command-scoped Xcode developer directory. No global Xcode selection changed.
- `cargo test --locked --offline`: 2 passed. Tests cover exact source persistence/reopen, one-instance locking, pre-existing external edit detection, editor autosave, undo/redo, front matter/comment preservation, immediate flush, IME candidate exclusion/cancel.
- `cargo clippy --locked --offline --all-targets --no-deps -- -D warnings`: passed for owned source. Patched upstream GPUI macOS code emits deprecation warnings; not treated as owned code or silently rewritten.
- `cargo fmt --check`, `git diff --check`: passed. Impeccable detector emitted no findings for the Rust target (it is not a Rust correctness checker).
- Dependency tree has one GPUI 0.3.6 graph, base 0.6.6, selected Markraft editor/core/commonmark plus its math/media/syntax dependencies. No Markraft app/workspace/notes crate linked.
- Native CUA: heading, bold, tasks, native paste, task toggle, wrapping, autosave and reopened state verified. Broader fixture renders lists/link/code/table/local PNG; native table-cell paste, undo/redo and saved Markdown verified.
- Native normal Cmd-Q: original fixture PID 46301 exited, execution session reported 0. Post-close UI observation relaunched a new PID 46726. No forced exit is used.
- Final source review found readable content and truthful limits. Close/Retry now have named button roles, tab focus and Enter/Space handlers. Keyboard activation of those controls is not separately verified.
- Public AppKit borderless/resizable frame adjustment runs asynchronously on the main queue; prior creation-borrow error resolved. No private window API added.
- First-note source snapshot is in `first-note-saved.md`; shared comparison fixture in `../fixtures/editor-comparison.md`.

Library screenshot: `libfile_70531c4f7a64819197c4d63a22ed2628`, image `rust-note-reopened.png`, successful upload, version 0. Local Library version xattr saved.

Remaining: exact Apple perimeter evidence (app capture clips to content; no documented permitted full-desktop route in current CUA APIs; screen-context tool is voice-only), matched native Swift comparison, mixed-selection/native IME/resize/failure interactions, and Swift feature parity. Swift source/production data/frozen release remain untouched. No performance or universal-platform claim.
