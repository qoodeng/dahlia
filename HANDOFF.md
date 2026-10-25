# Dahlia handoff

Start with [README.md](README.md), [CONTRIBUTING.md](CONTRIBUTING.md), [DESIGN.md](DESIGN.md) and [dependency audit](docs/DEPENDENCIES.md). This Rust/GPUI application is Dahlia. Its separate Swift Ortensia release, repository and real note data must remain untouched. Native tests use a temporary `DAHLIA_DATA_DIR`.

## Current development state

Main has CI green on Linux, macOS and Dependencies, and no pull requests are open. It contains foundations #1 and #2, the first A+ stack (performance #3, Mac packaging #4, asynchronous durability #7, the integration #8, coverage/maintained dependencies #9), and the second stack, merged in order on 2026-10-15 with regular merge commits:

- [#6](https://github.com/qoodeng/dahlia/pull/6): build, test and run on Linux (X11 and Wayland).
- [#11](https://github.com/qoodeng/dahlia/pull/11): in-window app menus on Linux.
- [#12](https://github.com/qoodeng/dahlia/pull/12): Linux polish (Ctrl-only shortcut labels, wrapping in-window alerts, Show in File Manager, Arrange only where windows can move).
- [#13](https://github.com/qoodeng/dahlia/pull/13): the cargo-deny CI gate.
- [#14](https://github.com/qoodeng/dahlia/pull/14): storage property and fault-injection tests.
- [#15](https://github.com/qoodeng/dahlia/pull/15): collapsed-title ellipsis and Stickies-matched strip glyphs.
- [#16](https://github.com/qoodeng/dahlia/pull/16): SAFETY comments, the notebook split and docs.
- [#17](https://github.com/qoodeng/dahlia/pull/17): the six installed Stickies colours and an sRGB-tagged Metal layer.
- [#18](https://github.com/qoodeng/dahlia/pull/18): the Stickies shortcut audit for macOS and Linux.

The native Window-menu fix from the draft [#10](https://github.com/qoodeng/dahlia/pull/10) (no duplicate AppKit note entries; the Collapse/Expand label refreshes) was based on the coverage branch, not `main`. It landed through [#19](https://github.com/qoodeng/dahlia/pull/19), merged from current `main` with the owned `markraft-native-window-menu.patch`, both regression tests and the regenerated third-party notices. #10 was closed in its favour. #5 was closed earlier, superseded by #8.

Do not merge or enable auto-merge without an explicit owner instruction.

## Remaining A+ gates

Architecture/module APIs, typed protocols, guarded fixtures, storage write-fault regressions and seeded storage property tests are in the stack above. Mac LCOV reporting/upload is implemented and locally validated. Final optimized app benchmarks, the complete patched Markraft workspace suite, and its separate host-feature suite pass locally for the combined candidate. The owned workspace now uses the same prepared GPUI Base/metal/pulp dependencies as Dahlia. Repeat commands are in [CONTRIBUTING.md](CONTRIBUTING.md); the exact local logs and checksummed candidate receipt for the #9 candidate were kept on the Mac in `/tmp/dahlia-a-plus-coverage-dependencies/build/release-candidate/` and are not in the repository.

`cargo deny` runs in CI. Three maintenance advisories (bincode, rustybuzz, ttf-parser; no vulnerabilities) are pinned by syntect and GPUI and are ignored by ID with reasons in `deny.toml`; [dependency policy](docs/DEPENDENCIES.md) records why, the input boundaries and a scoped font-hardening recommendation. No font migration/backport was made. Final native interactive/design qualification and actual signed installer qualification remain open. Test-context benchmarks are not proof of real keyboard/frame latency. The isolated candidate QA app is ad-hoc signed, uses a unique bundle identifier and temporary data directory, and has not been launched. It is not a distributable release. Do not retry the blocked GUI action or bypass its unresolved approval.

Ordinary AppKit termination now routes through the durable Quit operation: it is answered Terminate Later while saving (so logout and shutdown are not aborted), then replied to, and replied NO on save failure. Forced termination remains best effort.

The app is package/executable `dahlia`, bundle `dev.qoodeng.dahlia`, data folder `Application Support/Dahlia`. Migration from `ortensia-rust` is guarded by the legacy lock; fixtures use explicit overrides. Markraft stays at the pinned public commit plus owned patches. Never substitute an unpublished submodule commit.

## Prior delivered release

The prior Dahlia 1.0.1 source was `cd5d739ae7a4646f450b90eb3f5a1caa249a0c98`, build 3. The local release receipt records Apple acceptance of app submission `530B714D-C133-4EEF-BC8C-CA413448A1C7`, the exact mounted app checks and DMG SHA-256 `88d899ca665a371396bf2d147f96425a4afe035eecaaf4deaa8f7e7ffbf7fd9a`. Its outer DMG was not separately notarized. Those historical results do not qualify current A+ source. Xcode Organizer's existing account/archive workflow was used; absence of a notarytool profile alone is not a release blocker. Personal 1.0.1 DMGs built since then from merged `main` (build 4 and later) are Developer ID-signed with the hardened runtime but not notarized; they are for the owner's own use, not releases.

The pre-A+ handoff is retained as [historical evidence](docs/history/PRE-A-PLUS-HANDOFF.md). It described earlier 1.0.0 artifacts and must not be read as current release status. [MAC-1.0.md](MAC-1.0.md) is the historical scope contract; current A+ gates supersede its performance/testing exceptions. See [Mac packaging](docs/PACKAGING.md) for the scripted path and exact release evidence requirements.
