# Dependencies

`Cargo.lock` is the reproducible resolution. Markraft and GPUI versions move together; the owned patch stack and generated macOS dependency are described in [patches/README.md](../patches/README.md). Run `cargo deny --locked check` from the repository root; CI runs it on every push and pull request (the Dependencies job). `deny.toml` lists the only three advisories it tolerates, by ID, with the reason each cannot be removed from Dahlia (below). Any other advisory, vulnerability, banned source or disallowed license fails CI.

| Direct dependency | Purpose |
|---|---|
| gpui-pre, gpui-pre-platform | Native window, input, drawing and platform integration; exact versions match Markraft. |
| gpui-base | Existing accessible controls and layouts. |
| markraft-core, markraft-commonmark, markraft-gpui | Document model/history, source-preserving Markdown and editor view. Pinned submodule, MIT. |
| log | Diagnostics without a second UI error channel. |
| serde, serde_json | Versioned library/settings metadata and portable reports. |
| uuid (v4) | Independent note, attachment staging and recovery identities. |
| dirs (6) | Platform application-data location. |
| futures | Existing GPUI asynchronous channels and task composition. |
| image | Bounded decoding/normalization of managed PNG/JPEG/GIF/WebP/BMP/TIFF resources. |
| sha2 (0.11) | Attachment addresses, source fingerprints and immutable checkpoint verification. |
| percent-encoding, url | Parse and safely resolve Markdown image/file references. |
| unicode-segmentation | Grapheme-safe editing and checking ranges. |
| libc (Unix) | File-open flags and platform constants. Locks use `std::fs::File`. |
| raw-window-handle (macOS) | Obtain the native window through GPUI's supported interface. |
| objc2, objc2-foundation, objc2-app-kit, objc2-quartz-core (macOS) | Typed AppKit/Foundation/Core Animation calls for native windows, dialogs, spelling and appearance. |
| block2, dispatch2 (macOS) | Native asynchronous callbacks and main-queue dispatch. |
| tempfile (development) | Temporary-directory lifetime guards in tests. |
| gpui-pre test-support (development) | Real editor/application context in regression tests. |

## Maintained transitive replacements

Three maintenance advisories are removed from the active application graph without changing
widget logic or the GPUI/Markraft pins:

- `fxhash` (RUSTSEC-2025-0057): the owned CommonMark dependency patch updates
  scraper 0.24 to 0.25 and selectors 0.31 to 0.33, which uses `rustc-hash`.
- `instant` (RUSTSEC-2024-0384): the checksum-verified GPUI Base 0.6.6 source
  uses its existing `web-time` dependency for all clock imports. Both libraries
  use the standard library clock/duration on native targets. Preparation repairs
  modified cache content and preserves the previous source if verification fails.

- `paste` (RUSTSEC-2024-0436): checksum-verified metal 0.33.0 and pulp 0.22.3
  manifests alias their macro dependency to maintained `pastey` 0.2.3. Their
  source, GPUI pins and public APIs stay unchanged. This is a macro implementation
  replacement, so compilation and the parent crate tests remain required.
  Cargo.lock still records `paste` for the unselected optional rav1e AVIF path;
  `cargo tree --target all -i paste` has no application path, and the unsuppressed
  audit no longer reports this advisory for the resolved application.

Run `python3 scripts/prepare-maintained-dependencies-test.py` after preparation to check those
source-integrity and failure-atomicity guarantees.

## Audit findings requiring upstream work

Three maintenance advisories (none a vulnerability; 0 vulnerabilities are reported)
remain in GPUI's and syntect's pinned trees. No patched versions are listed by RustSec;
removing these packages requires upstream migrations outside Dahlia. `deny.toml` ignores
exactly these IDs, with reasons, so CI can gate everything else. They remain open work,
not a clean audit claim: delete each entry when its parent moves (cargo-deny warns about
an ignore that no longer matches).

| Advisory | Path into the application | Required follow-up |
|---|---|---|
| [RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141) bincode | syntect → Markraft syntax | Syntect 5.3 (latest inspected) and the bundled two-face syntax/theme assets require bincode dumps; replacing the format requires rebuilding/requalifying those assets. |
| [RUSTSEC-2026-0206](https://rustsec.org/advisories/RUSTSEC-2026-0206) rustybuzz | usvg → GPUI | usvg 0.48.1 uses harfrust, but GPUI 0.3.6 requires the incompatible 0.46 API; migrate and requalify the pinned rendering stack together. |
| [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192) ttf-parser | platform/font dependencies | GPUI directly uses ttf-parser and other font parents also retain it; migrate the font APIs/assets to skrifa together with GPUI. |

The license policy includes the permissive `bzip2-1.0.6` license used by `bzip2-sys`; redistribution must preserve its copyright/license notice. The packaging helper checks generated transitive notices against Cargo.lock and includes recovered upstream copyright texts. Packages that omit their own notice files are accompanied by their complete original registry source archives, preserving every published notice/header without inventing copyright text; see [packaging](PACKAGING.md). Archive bytes are checked against the locked registry checksums. The helper stops if notices or their source provenance are stale, changed or unresolved.

## Font-input boundary and bounded hardening follow-up

The remaining font advisory IDs are classified as INFO Unmaintained by RustSec;
that is not evidence that every font-parser path is safe. The issue linked from
RUSTSEC-2026-0192 reports algorithmic-complexity denial of service, and the upstream
unreleased changelog documents concrete malformed-font traversal/recursion fixes.
No exploit has been reproduced against Dahlia by this dependency review.

The inspected Mac paths use bundled Paper Mono for app text, embedded KaTeX fonts
for math (`markraft-math` enables `ratex-svg/embed-fonts`), and local/system fonts
for Unicode/emoji fallback. RaTeX also accepts the process-level
`RATEX_UNICODE_FONT` configuration. GPUI's SVG font resolver uses its system and
bundled font database. HTML import maps formatting to document marks; no
note-controlled font-file import or network font download was found in these
paths. Managed image import allow-lists raster formats and normalizes to PNG.

An untrusted note can select characters and exercise math/font rendering, but
this inspection did not find a path by which the note supplies malformed font
bytes. A malicious locally installed/configured font, or compromised bundled
font, is therefore the demonstrated input boundary to prioritize. This is a
static reachability assessment, not a fuzzing result or security guarantee.

Before considering a broad SVG/font migration, evaluate a separate owned patch
to the pinned ttf-parser 0.25.1 using these upstream commits and their regression
fixtures. Do not replace it blindly with upstream main: that tree also contains
unrelated edition, public API and font-behavior changes and is not a published
compatible release.

| Hardening | Upstream commits |
|---|---|
| Bound total CFF/CFF2 subroutine calls and tighten the cap | [0ea2906](https://github.com/harfbuzz/ttf-parser/commit/0ea2906b0c9fcf1fe1ca651631a83aa45580ba57), then [32989c3](https://github.com/harfbuzz/ttf-parser/commit/32989c3e733a43b70f1e9e62495aedda174b03c2) |
| Reject recursive GSUB/GPOS extension lookups | [52d367e](https://github.com/harfbuzz/ttf-parser/commit/52d367e719c806ff91e0d49910aacc14d669bab4) |
| Bound composite glyf/gvar component visits | [dd7a716](https://github.com/harfbuzz/ttf-parser/commit/dd7a71698a9e1377b44c6b4354d13e5d5fab818f); regression fixtures in [6bc51bc](https://github.com/harfbuzz/ttf-parser/commit/6bc51bc9366dbac4126f774cf9fb41efc0a2abd8) |
| Bound COLRv1 paint graph visits, including early exits | [f03027c](https://github.com/harfbuzz/ttf-parser/commit/f03027cf5de4ebf4011f6b89212514d9542d91c5), then [c8a004b](https://github.com/harfbuzz/ttf-parser/commit/c8a004bfe5a5c658ca26a858858733f9bf6fc778) |
| Reject empty CFF2 BLEND stacks | [0661c26](https://github.com/harfbuzz/ttf-parser/commit/0661c26f3d3414f1032b118177a3cbda46ca0d67) |
| Reject out-of-buffer reads without input-reachable assertions | [dcb583b](https://github.com/harfbuzz/ttf-parser/commit/dcb583b911f740b34c2aea2f93b9efc3d490f8cb) |

That scoped follow-up should preserve the public 0.25 API, add the upstream
malformed-font fixtures, test normal bundled/system font output and timing, and
recheck all reverse dependency paths. It would mitigate identified bugs while
leaving the maintenance advisory visible; it is not equivalent to removing
unmaintained packages or satisfying a clean `cargo deny` gate. A renderer rewrite
is not a prerequisite for this hardening, and no such migration is part of the
current dependency change.
