# Contributing to Dahlia

Start with [README.md](README.md), then [DESIGN.md](DESIGN.md) for product decisions and [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) for dependency policy. Full Xcode and the pinned Rust toolchain are required on macOS.

## Local checks

```sh
sh scripts/prepare.sh
sh scripts/check-patch-drift.sh
cargo fmt --check
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo clippy --locked --all-targets -- -D warnings
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test --locked
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test --locked --release --bin dahlia -- --ignored --nocapture --test-threads=1 perf_bench
cargo deny --locked check
cargo install cargo-llvm-cov --version 0.9.1 --locked
rustup component add llvm-tools-preview
mkdir -p target/coverage
cargo llvm-cov --locked --release --all-targets --lcov --output-path target/coverage/lcov.info
python3 scripts/package-mac-test.py
python3 scripts/prepare-sparkle-test.py
python3 scripts/release-test.py
python3 scripts/generate-licenses-test.py
```

Timing gates run sequentially in an optimized build. Record the exact commit, command, machine and limitations with results; a test-context timing is not proof of interactive frame latency.

Run the complete patched Markraft workspace suite after changing a patch. Preparation
selects the same maintained dependency sources for the host and the workspace;
Sparkle is needed only to link the standalone upstream application's tests:

```sh
(cd vendor/markraft && bash scripts/download-sparkle.sh && \
  DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test --workspace --release --locked)
(cd vendor/markraft && DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  cargo test -p markraft-workspace -p markraft-notes --release --locked)
sh scripts/check-patch-drift.sh
```

The app-flow tests use GPUI's headless test platform. This does not qualify visible
windows or replace the separate native/signed-installer acceptance checks. The
separate embeddable-crate run tests host features without standalone application
feature unification. Never weaken a gate to obtain a pass.

Use a temporary `DAHLIA_DATA_DIR` for every native test. Never use the real note library or Swift Ortensia data. GUI automation requires the owner's authorization and the app's access approval; respect a declined request.

Keep commits reviewable and PRs in draft until their listed gates pass. Do not merge or enable auto-merge without an explicit owner instruction. Signing/notarization and installer qualification follow [docs/PACKAGING.md](docs/PACKAGING.md); compiling a binary is not a release receipt. Releases are one command, `scripts/release.sh` ([RELEASING.md](RELEASING.md)); versions are dates, `YYYY.MM.DDNN`, so Cargo's `version` is internal and never shown.

A change an older build cannot read (a new enum variant such as an appearance, a new required field) must bump the notes-list `version` in `src/notebook`; older builds refuse newer lists instead of rebuilding them. See the compatibility rule in [RELEASING.md](RELEASING.md).
