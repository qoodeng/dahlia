# Appearance milestone — 2026-10-14

The isolated Rust workspace remains separate from Swift Ortensia (`2ae2c2c` reference). Swift source, storage and release packaging are untouched. GitHub backup is private at https://github.com/qoodeng/ortensia-rust; first push verified main `71ff0d8de99b81728caa79ed3f6a78e106593e32`. Existing local `gh` qoodeng keyring authentication succeeded once network access was approved; no new credentials were obtained. Tracked content was checked for secret patterns and runtime/private notes; only source, docs, licensed assets and synthetic/reference evidence are included.

## Verified

- Final native `cargo build --locked --offline`: success. `cargo test --locked --offline`: **9 passed, 0 failed**. Owned-source `cargo clippy --locked --offline --all-targets --no-deps -- -D warnings`: success. Upstream patched macOS dependency still emits its deprecation warnings.
- Added focused tests for legacy Classic preservation, new Glass defaults, style/default/shared-tint persistence, exact Swift transparency mapping and endpoints, appearance changes retaining the editor entity/source/selection/undo, and shared live background updates preserving both notes' editor histories. Existing storage and note tests remain passing.
- Runtime reports **Prose font: Helvetica Neue; code font: Paper Mono**. Unmodified Paper Mono Regular/Bold assets copied from the read-only Swift reference; checksums and OFL license in `assets/fonts`. Fonts register in-process through GPUI, not system-wide. The reproducible Markraft patch adds a code-font style hook through its existing shaping paths, without replacing the editor.
- Native Cmd-2 changed Classic to the exact Swift blue paper/spine pair. Appearance > Glass changed the same note to HUD/Aqua material with 6-point corners and transparent control fill. Source/text remained present. New note used factory Glass/yellow/12 pt.
- Native multiline paste rendered Helvetica Neue regular/bold/italic, Paper Mono fenced and inline code. Cmd-Z/Cmd-Shift-Z still removed/restored that paste after a live transparency change.
- Shared slider initially 56% (stored tint 0.22); pointer changes updated percentage and both existing Glass backdrops. Cmd-Option-] reached stored tint 0 (100% transparency) while text remained opaque. Final keyboard focus fix verified Home=0%, End=100%, Left=99%, then pointer midpoint=50% (stored tint 0.25). Global preference writes coalesce after 400 ms and flush on quit.
- Separate Cmd-Option-T visibly faded the entire note to native window alpha 0.75; toggled back before finish. Glass collapse produced a 12-point strip with retained title, then expansion restored the editor. Normal quits returned exit 0. Relaunch restored content, styles and shared 50% tint.
- Existing app handle reused throughout; no new app-binding call. Most UI calls took roughly 0.7–3 seconds; first capture after process launch took about 10 seconds. No UI stall or unresolved approval remained at completion.
- Font patch reverse-check and `sh -n scripts/prepare.sh` passed. A pristine pinned-source fixture accepted the patch and exactly reproduced all five compiled editor files. Format/diff checks recorded at commit.

## Specific remaining limits

- Captures include content only, so exact installed Apple Stickies exterior-border parity remains blocked. The Classic 1-point black perimeter follows Swift source, not measured Apple parity. No capture restriction was bypassed.
- macOS material uses the same public HUD/behind-window/Aqua primitives and mask mapping as Swift; matched active/inactive/compositor/full-frame pixel fidelity is not established. The transparency slider opens in a compact GPUI window instead of Swift's inline native-menu control.
- Linux/Windows are configured but not built/run here. They use GPUI's available backdrop support and content-alpha fallback; macOS mask/HUD fidelity and whole-native-window opacity are not claimed there. No platform packaging or performance claim.
- Reduce Transparency is checked when appearance/background is refreshed; live OS accessibility-setting toggling without another refresh has not been verified.
- Reading size and current appearance defaults exist. Use as Default captures implemented appearance/color/dimensions/floating/translucency/reading fields, but Swift's checking/smart-edit propagation remains queued. Complete paragraph/list/code spacing and non-default code-size parity are unverified.
- Archive/trash/restore, import/export/managed attachments, arrangement, search and system editor integration remain queued in `PARITY.md`.

Raw captures are `stage3-classic-blue.png`, `stage3-glass-blue.png`, `stage3-glass-clear.png`, `stage3-glass-blue-shared.png`, `stage3-typography-glass.png`, `stage3-glass-translucent.png`, `stage3-glass-collapsed.png` and `stage3-transparency-control.png`. `stage3-note-state.json` contains only synthetic metadata. `appearance-comparison.html` shows raw references; Swift capture content/tint differs and is not a same-fixture pixel-match assertion.

Library images, version 0: typography `libfile_20da6dd7160c8191bd7bc5c2aef86f99`; Classic blue `libfile_2d5003c5cf9c81919084c2445b9d6429`; shared slider `libfile_275a4eb903248191a235d02eb8b6c8b3`; collapsed Glass `libfile_2a83a4c514cc8191ba7ff0050bfd7951`.
