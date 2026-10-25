# Lifecycle milestone — 2026-10-14

The existing appearance commit `2f7e5b4` stays intact. All tests and native interactions used isolated synthetic Rust fixtures; Swift production source/storage/packaging and real user notes were not changed. Publication was paused while this stage was implemented; the approved appearance push has subsequently reached private origin main `2f7e5b4`.

## Implementation and focused checks

- Active/Archived/Trash are persisted per-note states. Archive and Delete flush committed source before an atomic metadata transition; files, relative-resource folders and identities are never deleted or moved. Restore validates the expected collection and reopens the same identity/style/geometry. Failed saves/transitions keep the note open. Existing closed prototype notes remain readable as Active notes rather than silently changing their meaning.
- Close uses the frozen Swift source's compact app-modal `NSAlert`: “Close note?”, Archive, Cancel, Export and Archive…, Delete. Empty source archives directly. The public macOS adapter preserves Swift's key equivalents and avoids GPUI's generic prompt initially focusing Delete. Header controls are unchanged; no new toolbar or custom oversized close controls.
- Per-session Undo/Redo Delete is separate from editor undo, capped at 20 entries. Restoring one note leaves another note's editor/history untouched. Manual restore clears that note's deletion-history entry. A small unavailable-action mapping preserves native deletion-menu availability despite GPUI's global action validation.
- Import prepares every chosen UTF-8 .md/.markdown/.txt file before publishing the metadata batch; limit 10 MiB per file. Originals are never modified. Imported local references retain their original base folder and require a disclosure. Export writes exact committed source to a new file and refuses collisions; same-directory hard-link publication avoids overwriting an existing destination. Local assets are not bundled; Export and Archive keeps a note open if linked files were not exported or the note changed during IO.
- `cargo build --locked --offline`: success. `cargo test --locked --offline`: **12 passed, 0 failed**. `cargo clippy --locked --offline --all-targets --no-deps -- -D warnings`: success for owned source. Existing patched macOS dependency warnings remain upstream.
- Added tests cover durable archive/Trash/restore and failed metadata transitions, exact CRLF/front-matter/comment source transfer, collision refusal and invalid UTF-8 preparation, independent undo after deletion/restoration, and failed close preserving an externally changed source/window. Prior appearance/storage/editor tests continue passing.

## Native proof

The first native check reached the prior test bundle and was not counted as lifecycle proof. The correct executable was installed into the already bound fixture bundle, with only its duplicate synthetic process stopped; no app rebind was attempted. Corrected native menu showed Edit and transfer/recovery actions, confirming the lifecycle binary.

- Cancel retained the note; Archive removed its window and File > Archived Notes restored full content/style.
- Delete moved the synthetic note to Trash; Edit > Undo Delete Note restored its full Markdown.
- Native file picker imported `import-fixture.md`; rendered heading/bold/task and original comment.
- Native Export and Archive saved `native-export.md`, then archived the imported note. Shell checks proved **all 132 bytes identical** between original input, persisted note and exported file, including CRLF, front matter and comment.
- Runtime produced some window-not-found teardown diagnostics while closing fixture windows. Source/state checks passed; the final code removes observers before close and skips redundant export cleanup on an already archived window. No universal claim is made that every upstream teardown diagnostic is eliminated.
- Existing CUA handle remained responsive, generally 0.6–3 seconds per action; corrected first capture was under one second. No stalled UI operation remains.

## Remaining consequential gaps

Full Swift Recovery/checkpoint/corruption workflows, multi-selection/batch transfer, portable attachments, non-UTF-8 conversions and full transfer reports are queued. Importing linked local files currently requires an open note for the disclosure; errors with no open note are logged rather than a complete recovery presentation. Generic/extended HTML resource discovery is not a complete asset manifest. Export's collision-safe hard-link publication requires a supporting destination filesystem. Per-note editor history starts afresh after a closed note is restored, matching the boundary of a new editor; another open note's history is retained.

macOS is built/exercised; Linux/Windows native behavior is untested. Glass/compositor/full-frame parity and the exact installed Stickies exterior border remain separately unverified. Source spacing and full system editor services still differ. There is no speed or 1.0 parity claim.
