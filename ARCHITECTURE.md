# Dahlia architecture

Dahlia is a GPUI application over ordinary Markdown files. UI state belongs to the GPUI
thread; source preparation, file writes, fsync and structural metadata publication run
outside it. A visible operation completing is different from an I/O request being queued.

## Module ownership

| Module | Owns |
| --- | --- |
| `main.rs`, `lib.rs`, `application.rs` | Native entry point, private module declarations and internal reexports. The normal binary calls `dahlia::run()`. |
| `app.rs` | `AppState`, open-window registry, duplicate-open protection, startup, note creation, linked-source upgrades, shared appearance and quit orchestration. |
| `note.rs` | `Note`, editor/source snapshots, save/close task state, checking, note rendering and source-to-editor setup. |
| `keys.rs`, `menus.rs` | Typed actions and key bindings; menu inputs, availability and rebuilding only when visible inputs change. |
| `notebook/` | `mod.rs`: validated index, note records and the `Notebook` API. `commit.rs`: pending/completed structural commits. `metadata.rs`: the metadata writer thread. `checkpoint.rs`: immutable checkpoints. `recovery.rs`: recovery inventory, previews and restores. `tests.rs`: the notebook suite. |
| `storage.rs` | Managed-file checks, exact source publication, source history and one serial `NoteWriter` per open note. |
| `finding.rs` | Background search, immutable replacement preparation and guarded one-undo editor application. |
| `operations.rs`, `window_commands.rs` | Import/export, restore/delete orchestration and durable multi-window geometry changes. |
| `recovery.rs` | Recovery selection, lazy preview and restore/export UI; `RecoveryKind` selects the read protocol. |
| `transfer.rs`, `attachments.rs` | Bounded input reads, Markdown/image transfer, managed attachment names and native image decoding allow-list. |
| `appearance.rs`, `chrome.rs`, `panel.rs`, `settings.rs` | Validated style values, shared palettes, note chrome and settings controls. Strip height has one definition in `notebook/mod.rs`. |
| `platform.rs`, `macos_input.rs`, `checking.rs` | Native boundaries, first-responder preservation and document checking. Native-only code is cfg-gated. |
| `updates.rs` | Sparkle 2 automatic updates in packaged macOS builds: loads `Contents/Frameworks/Sparkle.framework` at run time, owns the `SPUStandardUpdaterController`, and backs the app menu's "Check for Updates…". Absent in development builds and tests. Sparkle quits the app through `[NSApp terminate:]`, which reaches `app.rs`'s termination guard (answers Later, saves every note, then replies); the guard is unchanged. |
| `copy.rs`, `notify.rs` | Plain error wording and the visible failure channel (active note or app alert). |

Modules are private. Cross-module application interfaces are `pub(crate)`; the library's
external surface is application startup and the narrow native style transition function
used by the macOS regression harness. Internal note subscriptions and task bookkeeping
stay private where no other module needs them.

## Save and structural commit ordering

1. `Note::request_save` captures an immutable source snapshot, schema and committed node
   on the UI thread and returns an owned future. Rendering/validation runs on the serial
   `NoteWriter`, before its corresponding disk publication. A slow render cannot overtake
   a newer save. Dropping a waiter does not cancel an already queued save.
2. `NoteStore` checks the loaded baseline against the current disk bytes, preserves exact
   history, writes a unique pending file, syncs it, renames it and syncs the directory.
   Before rename, a failure leaves the published source and cached baseline unchanged;
   partial pending files remain recoverable. After rename, a failed directory sync remains
   pending and an identical retry must still perform that sync.
3. Structural notebook requests produce `PendingCommit<T>`. The worker acknowledges I/O;
   `complete` then publishes the state. Overlapping structural requests return typed Busy
   with `WouldBlock`, preventing stale snapshots from silently replacing each other.
   Cosmetic fields changed during a pending commit are rebased before a follow-up write.
4. Close and quit protect in-flight edits with the durability lock and await source and
   metadata acknowledgement. A failed close keeps the note window and edits available;
   successful close also waits for the writer's lock release. Import, restore and linked
   source changes retain their existing ordered commit and failure paths.

The explicit Quit action and ordinary AppKit termination requests share the same durable
quit operation. An owned GPUI `applicationShouldTerminate:` hook answers Terminate Later while
saving, because Cancel would abort a system logout, restart or shutdown, and replies through
`replyToApplicationShouldTerminate:` once source and metadata are acknowledged (YES on success,
NO when a save failed). Menu Quit has no pending reply and quits itself; a pending system
request is answered exactly once. A quit that cannot start (a note is still opening or closing)
answers Cancel rather than leave the system waiting. A failed save retains windows and unlocks
editing. Forced termination still cannot be cancelled and only has the existing best-effort
drain callback. Ordinary cosmetic settings update the UI immediately, then use
coalesced metadata writes; native-worker failures still go through `notify`.

## Trust and typed protocols

- `ColorIndex` accepts only the six palette indices. Serde still writes/reads the existing
  numeric format; invalid indices are rejected before constructing a style.
- `CloseChoice` converts native button responses at the platform boundary. Unknown or
  dropped responses cancel. `BulkOperation` replaces string window-operation selectors.
- `RecoveryKind::{Source, Metadata, Checkpoint}` records provenance when an item is
  inventoried. Its protocol never changes because a path later becomes a file/directory.
  Managed-path, symlink, digest and attachment checks still apply on each actual read.
- `StorageError` and `CommitError` express authored boundary failures inside `io::Error`.
  OS errors retain their original values. `NoteError` retains the actual I/O cause in an
  `Arc`, while presentation equality compares its visible message and origin. `copy::plain`
  continues preserving authored wording and translating OS kinds into plain sentences.
- External-change protection is a baseline check before publication, not a cross-process
  transaction with unrelated editors. Existing file locks and the pre-rename check remain;
  this refactor adds no claim of immunity to an editor racing that final check.
- Replace All still captures the exact immutable editor/source baseline, prepares in the
  background and checks generation, composition, durability lock and prepared-state guards
  before authoritative application. Find refresh remains asynchronous.
- Native ImageIO only receives allow-listed image types. Both crate roots deny undocumented
  unsafe blocks; concrete ownership/main-thread/layout reasoning remains at each native
  unsafe boundary.

## Tests and local verification

`[lib] test = false` keeps default test execution from running the application suite twice.
The binary's **test-only** include compiles the same private `application.rs` module tree;
its normal library startup call is cfg-disabled in that harness, avoiding duplicate GPUI
inventory/action registration. This deliberately preserves existing commands such as:

```sh
cargo test --release --locked --bin dahlia
cargo test --release --locked --bin dahlia perf_bench -- --ignored --nocapture
```

`tests/macos_input.rs` imports the real library function. It uses an unshown native window,
asserts that it stays invisible and checks first-responder identity across style changes;
it no longer imports implementation files using `#[path]`. GPUI unit tests use test windows,
not visible native application windows. All filesystem fixtures use `tempfile` guards,
including helper-returned guards whose lifetime spans the test. Intentional destructive
fixture setup (for example removing a linked image folder) remains explicit.

Storage regressions inject errors at the pending file's actual write boundary after a
partial write: disk full, permission failure and write-zero. They assert unchanged durable
bytes/baseline, retained partial work and successful retry. Additional cases cover missing
published sources and arbitrary changed external bytes, including invalid UTF-8. Existing
request/ack, worker barriers, close failure and prepared-edit tests remain intact; no
scheduler sleeps or timing gate changes are used. Benchmark fixture paths still start
absent, preserving first-directory-creation costs inside their existing measurements.

Settings tests draw the real control in GPUI test windows, send keyboard events, exercise
focus/window reuse and closure, verify slider/programmatic synchronization and latest
on-disk values, and verify a real metadata conflict is reported without overwriting it.

The shared module declarations are an include file, so format/check that tree explicitly
as well as Cargo's discovered targets:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer rustfmt --edition 2024 --check src/application.rs
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo fmt --all --check
CARGO_TARGET_DIR="$PWD/build/phase3/target" cargo clippy --release --all-targets --locked --offline --no-deps -- -D warnings
```

Phase 3 logs live under ignored `build/phase3/`. The coordinator owns coverage, vendor tests,
packaging, GUI qualification and performance comparisons. Ignored benchmark discovery is
checked here; no new timing or A+ qualification is claimed without those measurements.
