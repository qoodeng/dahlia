# Dahlia

A native sticky-notes app for macOS and Linux in Rust/GPUI, modeled on Apple Stickies and developed beside the existing Swift Ortensia release. Start with [current handoff](HANDOFF.md) for development/release status, [contributing](CONTRIBUTING.md) for checks, [design](DESIGN.md) for appearance decisions, and [Mac packaging](docs/PACKAGING.md) for distribution. [MAC-1.0.md](MAC-1.0.md) preserves the historical scope contract.

Native Markdown notes with independent editor histories using Markraft's MIT `markraft-gpui`, `markraft-core` and `markraft-commonmark`. The host owns its compact Classic, Glass and Sea Glass shell and isolated storage. No Markraft application/workspace dependency and no webview.

## Build and run on this Mac

```sh
sh scripts/prepare.sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo build --locked
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test --locked
DAHLIA_DATA_DIR="$PWD/evidence/test-data" cargo run --locked
```

## Build and run on Linux

GPUI's Linux backend renders through Vulkan on X11 or Wayland. On Debian/Ubuntu:

```sh
sudo apt-get install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev \
  libxcb1-dev libfontconfig1-dev libfreetype-dev libvulkan-dev libssl-dev pkg-config \
  mesa-vulkan-drivers
sh scripts/prepare.sh
cargo build --locked
cargo test --locked
DAHLIA_DATA_DIR="$PWD/evidence/test-data" cargo run --locked
```

`mesa-vulkan-drivers` supplies a Vulkan driver, including the lavapipe software renderer for machines without a GPU. The default data directory is `$XDG_DATA_HOME/Dahlia` (usually `~/.local/share/Dahlia`). Shortcuts use Ctrl where macOS uses Cmd (Ctrl+C/V/X/Z/A, Ctrl+Shift+Z or Ctrl+Y, Ctrl+B/I, Ctrl+F, Ctrl+N, Ctrl+S, Ctrl+W, Ctrl+Q, Ctrl+1–6 for the six colours and the rest), except where the Linux desktops expect otherwise: Replace is Ctrl+H, Code Block Ctrl+Shift+K, Choose Code Language Ctrl+Shift+L, Translucent Ctrl+Shift+T (Ctrl+Alt+T and Ctrl+Alt+L belong to the desktop), Help F1, and Markraft's Mac Emacs keys (Ctrl+D/K/P) are off. GPUI has no menu bar on Linux, so each note carries the same menus in-window: the strip's menu control or F10 opens them; arrows, Enter, Left/Backspace and type-to-select navigate, and Esc or a click outside dismisses. Menus show Ctrl shortcuts only (Collapse is Ctrl+M and Zoom Ctrl+Shift+M, as in Stickies; minimizing is the window manager's); Super is left to the desktop. Alerts such as the close prompt are drawn inside the note, wrap to its width and answer Esc with Cancel. Recovery's reveal button reads Show in File Manager and opens the folder through the desktop portal or `xdg-open`. Native spelling/grammar checking, HEIC import, Float on Top, app hiding/Services and the AppKit Classic frame stay macOS-only.

Known limitations on Linux, checked on X11 (xfwm4) and natively on Wayland (Weston 14, no XWayland):

- **Window placement.** GPUI has no API to move a window on Linux. On X11 the window manager also ignores the position Dahlia asks for when a note opens, so it picks the spot itself (xfwm4 centres every new note, which stacks them); saved positions are not restored. Core Wayland gives apps no way to choose or read their window position at all; the compositor places every window.
- **Arrange.** Because of that, Window ▸ Arrange By, its When Arranging policy and Undo/Redo Arrange are left out of the Linux menus (`platform::can_arrange`, decided from GPUI's backend at runtime). Running Arrange on either backend changed no window positions.

## Build & CI

- **Xcode is required.** The macOS GPUI platform needs full Xcode, not the Command Line Tools; set `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` (or select Xcode with `xcode-select`).
- **Prepare first.** `sh scripts/prepare.sh` initializes `vendor/markraft`, verifies Markraft's macOS dependency and applies `patches/`. `sh scripts/check-patch-drift.sh` fails if the submodule differs from its pinned SHA plus those patches; `.gitmodules` hides that drift from `git status`.
- **Toolchain.** `rust-toolchain.toml` pins the Rust release with rustfmt and Clippy; rustup installs it automatically.
- **CI is the source of truth.** `.github/workflows/ci.yml` runs prepare, patch drift, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` on macOS, and Clippy plus `cargo test --locked` on Ubuntu 24.04. Docs do not record test counts; the CI run does.
- **Benchmarks.** `src/perf_bench.rs` holds the performance gates as ignored tests: `cargo test --locked --release --bin dahlia -- --ignored --nocapture --test-threads=1 perf_bench`. `.github/workflows/perf.yml` runs them weekly or on demand and uploads the output.

The default macOS data directory is `~/Library/Application Support/Dahlia`, separate from Swift Ortensia and Markraft. Builds from before the rename used `~/Library/Application Support/ortensia-rust`; the first launch moves that folder to `Dahlia` with an atomic rename (or, failing that, a byte-for-byte verified copy that leaves the original in place), never while an older build has the library open and never when a `Dahlia` folder already exists (`platform::migrate`). Linux and Windows use the platform local application-data directory. A versioned index stores note identities and window state; each new note owns `notes/<UUID>/note.md`. An existing first-stage `note.md` stays in place, preserving relative image paths. Set `DAHLIA_DATA_DIR` for fixtures and `DAHLIA_EXPORT_DIR` for the export panel's starting folder; the old `ORTENSIA_RUST_*` names still work. The prototype never imports existing app storage. `assets/Info.plist` is the app bundle's property list: name Dahlia, executable `dahlia`, bundle identifier `dev.qoodeng.dahlia`. A development bundle built from it for UI tests should use its own identifier and data directory.

Typing saves after 400 ms. Save/quit shortcuts and the close control flush committed content immediately. Failed interactive saves keep the note open and offer Try Again. A library advisory lock and one lock per note protect cooperating prototype instances, and the storage detects pre-existing changes on disk before saving. An unrelated external writer can still race the check and atomic rename. Note writes run through one serial background worker per open note, using a synchronized temporary file and atomic rename. Close and managed Quit await durable acknowledgement asynchronously before releasing windows; metadata persists atomically. A pending structural library change reports Busy rather than applying a stale snapshot. Unix also synchronizes the parent directory; Windows lacks that directory-sync step in this implementation. A system quit hook is best-effort fallback.

Source preservation uses Markraft's `SourceDocument`/`SourceTrack` and transaction guard. Saves use `committed_document`, excluding an uncommitted IME candidate. Undo/redo follow the same change-and-save path. Rich clipboard codecs and Markdown formatting come from the reused editor. Managed image paste/drop/import normalizes bounded image inputs into immutable content-addressed PNG attachments. Portable Markdown export copies image resources; UTF-8/UTF-16 and folder import report conversions and failures.

## Dependencies

[Dependency purposes and the open audit findings](docs/DEPENDENCIES.md) are tracked alongside `deny.toml`. CI runs `cargo deny`; the three tolerated maintenance advisories are listed there.

The submodule pins Markraft `d2ff6779e6098add5a1c2d09a3e09f102e5ca628` (MIT, v0.1.10). GPUI and its platform package are exactly 0.3.6, GPUI base exactly 0.6.6. The platform requires `font-kit` and `runtime_shaders`, as in Markraft's external host example. Keep these versions together. `patches/markraft-code-font.patch` adds one host-selectable code-font field through the existing editor style API; `scripts/prepare.sh` applies/verifies it idempotently. The upstream submodule SHA stays pinned. Its expected working-copy patch is ignored as dirty state in `.gitmodules`; the complete patch is owned and backed up here. No unreachable dependency commit is required.

Cargo patches belong at the final workspace root. `scripts/prepare.sh` runs Markraft's reviewed checksum-verifying preparation for its public-API macOS patch. The generated dependency is ignored. Upstream provenance and licenses live in `vendor/markraft/patches/README.md` and the submodule LICENSE. `Cargo.lock` pins registry resolution.

Zorite's MIT editor-only crate was also inspected. Its smaller public API is attractive, but the inspected main revision's undo/redo handlers do not emit its change event; simple Changed-only autosave would miss history edits. Its full GPL application was not copied or linked.

## Verification and qualification

macOS Classic uses AppKit's own untitled frame with the installed Stickies resource's verified style mask 14, becoming 6 when collapsed; its hand-painted black perimeter has been removed. See `evidence/stickies-native-frame.md`. Exterior pixel thickness/color remain unmeasured because permitted screenshots omit the outer frame. The Classic strip, control geometry and palette follow the read-only Swift implementation.

The note-management stage supports new notes, close/reopen without deleting content, independent editor histories, persisted position/size, visible-bounds restoration, collapse/expand and a macOS Float on Top toggle. Close uses Swift's concise native Archive / Cancel / Export and Archive / Delete choices. Archive and recoverable Trash retain source files and identities; File submenus restore notes and Edit provides independent Undo/Redo Delete. See `PARITY.md` and `evidence/stage2-verification.md` for verified behavior and remaining gaps.

macOS is built and exercised. Linux builds and passes Clippy and the test suite in CI. It has been run on X11 (xfwm4, lavapipe) and natively on Wayland (Weston). Typing, autosave, undo/redo, new note, the in-window menus, the close prompt, minimize, quit and relaunch work. Windows paths and cfg wiring exist but have not been compiled or run. Floating is explicitly macOS-only today. Classic, Glass and Sea Glass, the six note colors, reading size, new-note appearance defaults and live transparency shared by both frosted styles are implemented. Sea Glass is a native interpretation of a sea-glass material that uses the six note colors as pastel palettes; its shader-only contour, bulge, moving reflection and grain are not reproduced, and it has not been checked on a screen. The global background control does not fade editor text; Window > Translucent separately applies 75% note opacity on macOS. The factory new-note style is Glass/yellow/12 pt at 360×350; older prototype notes retain Classic. Prose selects Helvetica Neue when available; code selects bundled Paper Mono, registered only in this process under SIL OFL 1.1. See `evidence/stage3-verification.md`. UTF-8 source import and exact Markdown source export are implemented; linked local files remain linked and are disclosed. Portable image transfer, bounded batch reports/rescue exports, Recovery previews and restore-as-copy, immutable checkpoints/history, checking/default propagation, Find/Replace across open notes, and native window arrangement with undo are implemented. Native checking uses guarded source transactions and excludes code, markup and uncommitted IME text. No speed or cross-platform parity claim.

Private GitHub backup: https://github.com/qoodeng/dahlia (`origin`, branch `main`). Build after a fresh clone with `scripts/prepare.sh` before Cargo.

Releases are dated, `YYYY.MM.DDNN` (for example `2026.10.2501`), and are built, notarized and published by one command, `scripts/release.sh`; see [RELEASING.md](RELEASING.md). That string is the bundle's `CFBundleShortVersionString` and `CFBundleVersion`, the Git tag is `v2026.10.2501` and the disk image `Dahlia-2026.10.2501.dmg`. `Cargo.toml`'s `version` is internal (`1.0.0`) and the app does not show it. Packaged builds update themselves with Sparkle 2 (Dahlia ▸ Check for Updates…), loaded at run time from `Contents/Frameworks`; development builds and tests have no updater and need no Sparkle.

Native smoke tests use isolated overrides. Lifecycle evidence is in `evidence/stage4-verification.md`; integrated Mac 1.0 evidence is recorded in `evidence/mac-1.0-verification.md`. A package is a release only after strict signing, Apple acceptance, stapling and Gatekeeper checks are recorded.
