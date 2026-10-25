# Markraft host style hooks

Upstream MIT Markraft stays pinned to `d2ff6779e6098add5a1c2d09a3e09f102e5ca628`. Its editor style exposed the prose face but used a fixed code-face constant. `markraft-code-font.patch` adds `EditorStyle.code_font_family` and threads it through existing source/code/inline-code shaping, including syntax-highlighted runs. The default remains the upstream face; the Dahlia host chooses licensed bundled Paper Mono. Core editing, document structure, source tracking and history are unchanged.

`scripts/prepare.sh` applies the patch on a fresh clone or verifies it is already present. The expected modified working copy is ignored as dirty submodule state in `.gitmodules`; the complete patch is tracked here. Do not replace the upstream SHA with an unpublished local commit. The patch was checked against pristine files at the pinned SHA and reproduced the compiled files byte-for-byte. Recheck it explicitly when updating Markraft. Upstream license is `vendor/markraft/LICENSE`.

`markraft-task-marker.patch` adds an optional `EditorStyle.task_marker_size` hook. Dahlia selects 0.8 times its reading size: 9.6 points at the default 12 points. The painted box and check scale together and center on the actual font cap-height band. With the hook set, list markers share one column: the box ends half its own side (0.4 em) before the text, a bullet (0.36 em) is centred on the box on the same cap-height band, and an ordinal ends where the box does, so bullets, numbers and tasks in one note line up at every reading size. It corrects the plain-row baseline fallback, which previously returned the line bottom. Completed-task and manually marked strikethrough use cap-height/x-height metrics and follows each wrapped visual row. Pointer and accessibility bounds retain at least a 14-point target without covering text. Task gutters grow only when a larger box needs room. Existing presets retain their original marker geometry when the hook is unset. Geometry tests cover reading sizes 9 through 48, nested checked/unchecked and wrapped tasks, and clicks in the padding outside the smaller visual. Apply this patch after the code-font patch.

`markraft-image-confinement.patch` adds `EditorView::with_image_confinement`. Upstream resolves `file:` URLs and `..` segments independently of the image root, so note content could display any readable local image. With confinement on, local images load only from inside the image base or root (Dahlia sets both to the note's folder); escaping paths, including symbolic links that lead elsewhere, show as invalid paths. Remote and host-asset images are unchanged. Unit tests in `images.rs` cover relative, rooted, `..`, `file:` and symbolic-link sources. It touches files no other patch changes.

`markraft-find-results.patch` lets a host search off the main thread. It adds `find_hits_in`, a pure search over an `EditorState`; `EditorView::set_find_results`, which shows hits found that way without searching again; `EditorView::find_hits`, which reads the current hits; `EditorView::doc_types`; and `EditorView::state_without_find`, a state without the find query for planning many edits (an active query searches the whole document again after every edit). Find behaviour is otherwise unchanged. Apply it after the image-confinement patch, which also touches `lib.rs`.


`markraft-batch-corrections.patch` caches per-round CommonMark correction context and merges changed ranges before checking overlap. Bulk transactions retain the normal correction rules while avoiding repeated whole-document work. Regression tests cover interval boundaries and cache invalidation between correction rounds.

`markraft-batch-source.patch` patches independent changed blocks in parallel for large shape-preserving transactions. It retains the serial renderer for structural changes, definitions, footnotes and ordered-list context. The resulting source is parsed and validated before publication. Tests cover byte preservation (BOM/front matter/CRLF), lists/code/setext headings, block maps, undo and conservative fallbacks.

`markraft-linear-find.patch` maps successive Unicode search matches to document positions with a forward cursor rather than recounting the entire prefix for every hit. Tests cover repeated folded Unicode matches and source coordinates.

`markraft-prepared-edit.patch` prepares immutable editor transactions and source validation off the UI thread, then installs them through the normal guarded edit/history/event path. Installation requires the exact captured editor state and source baseline; intervening edits, selections or durability guards reject stale work. Large source batching starts only when at least 64 top-level nodes changed, so ordinary typing retains the incremental path. Externally managed Find results invalidate on edit and let the host refresh asynchronously; built-in Find keeps its usual behavior. Tests cover stale candidates, guard rejection, single undo, source-cache isolation and ordinary editing with Find open.

Apply the patches in the order listed by `scripts/prepare.sh`. `scripts/check-patch-drift.sh` reconstructs that entire stack from the pinned upstream commit and compares it with the working copy.

`markraft-scaled-style.patch` turns Markraft's fixed layout constants into `EditorStyle` fields, so a host can scale them with its reading size: heading and code-block leading (`heading_line_height_ratio`, `code_line_height_ratio`), code-block size (`code_size`), the code panel's padding, inset and corner radius, inline-code pill padding, table cell padding, the quote bar's colour and width, rule thickness, round bullets that stay round at any size, underline and strikethrough thickness, and the syntax palette (`syntax_tones`). Every field defaults to the constant it replaces, so presets that leave them unset draw exactly as before. Dahlia sets them as ems of the reading size in `appearance::editor_style`. `host_geometry_replaces_the_fixed_constants` covers the defaults and the scaled values at 9 to 48 points. Apply it last, after the prepared-edit patch.

`markraft-prepared-setup.patch` prepares the initial immutable editor state, projection and document analysis without a window, then installs that exact state through `EditorView::from_prepared`. Dahlia builds this alongside large-note parsing on its background executor. The native view still owns focus, layout, input and rendering on the UI thread. A regression builds a setup on a native worker thread and verifies installation reuses the exact state and projection allocations. Apply after the scaled-style patch.

`markraft-native-quit.patch` extends the generated GPUI delegate with the public AppKit
`applicationShouldTerminate:` callback. A main-thread guard answers `TerminationReply::Cancel`,
`Now` or `Later`; Dahlia answers `Later` while it saves, so a system logout or shutdown is not
aborted, then calls `reply_to_termination` when source and metadata saves are acknowledged.
Preparation regenerates the native dependency after applying owned patches so a clean checkout
gets the same code.

`markraft-native-window-menu.patch` makes GPUI respect a native window's exclusion
flag when its title changes. Dahlia removes the initial automatic AppKit entry and
keeps its own open/closed note list, without losing standard native Window commands.

`markraft-maintained-selectors.patch` updates CommonMark's HTML parser from scraper
0.24 to 0.25, the nearest release using selectors with maintained `rustc-hash`
instead of the abandoned `fxhash`. The HTML import/preview regression suite verifies
the unchanged host API and source behavior. Its workspace manifest/lockfile also
select the host's prepared GPUI Base/metal/pulp sources, so the complete upstream
suite exercises the same dependency patches as the product. Apply after the
existing patch stack.

`gpui-base-0.6.6.patch` replaces GPUI Base's abandoned `instant` dependency
with its existing maintained `web-time` dependency and updates clock imports.
On native targets both expose the standard library clock/duration types; no
widget logic or UI behavior changes. `scripts/prepare-maintained-dependencies.py` reuses the
pinned Markraft preparer's verified archive extraction and atomic cache checks,
with the original registry checksum and this owned dependency/import patch. The
prepared crate lives under ignored `.build/dependencies/gpui-base`.

`metal-0.33.0.patch` and `pulp-0.22.3.patch` replace their compile-time `paste`
dependency with maintained `pastey` 0.2.3 using a Cargo package alias. No parent
source or public APIs are changed. `scripts/prepare-maintained-dependencies.py`
prepares these sources and GPUI Base from their original registry checksums with
the pinned Markraft verifier. The integrity tests cover all prepared crates.
