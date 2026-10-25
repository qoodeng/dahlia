# Mac distribution

Releases are normally made with the one-command [`scripts/release.sh`](../RELEASING.md), which
calls the packaging script below with the version it computes, then publishes. This page
describes packaging itself.

Run `scripts/package-mac.sh` on the Mac with full Xcode selected, Rust's pinned
toolchain installed, the Developer ID private key available, and an existing
`notarytool` keychain profile. The workflow does not launch Dahlia, Finder or a
browser, install the app, or open a data directory. Keychain access may still
require macOS approval if its signing key is locked.

First complete the release's code, performance and isolated-data acceptance gates.
Commit the exact reviewed source and initialize its Markraft submodule with
`scripts/prepare.sh`. Packaging rejects tracked or untracked changes, a mismatched
expected commit, custom compiler overrides, and an existing output directory.
Ignored files are excluded by cloning committed objects into a fresh temporary
checkout; its Markraft copy starts at the committed gitlink and receives only the
owned preparation patches. The original worktree is never prepared or changed.

```sh
scripts/package-mac.sh \
  --identity 'Developer ID Application: Christian Stephens (H45U5SB635)' \
  --notary-profile YOUR_EXISTING_PROFILE_NAME \
  --expected-commit "$(git rev-parse HEAD)" \
  --version 2026.10.2501 \
  --output /tmp/dahlia-distribution-2026.10.2501
```

`--version` is the dated release version `YYYY.MM.DDNN` (a real date, `NN` from 01 to 99,
zero-padded). It is stamped as both `CFBundleShortVersionString` and `CFBundleVersion`, names
the disk image `Dahlia-<version>.dmg` and is recorded in the receipt. Choose a new output
directory. `--dry-run` (with no identity or notary profile) builds the same bundle ad-hoc
signed for inspection, never contacts Apple and writes `dry-run-receipt.json` instead of
`receipt.json`.
The profile name is required; there is no credential or profile fallback. Set
`DEVELOPER_DIR` if Xcode is somewhere other than `/Applications/Xcode.app`.
Do not put passwords or API keys in the script, command line, or repository.
Create/update credentials separately with Apple's `notarytool store-credentials`.
The profile is checked before the build; invalid or unavailable credentials stop
packaging explicitly.

The script selects the root application's binary through locked Cargo metadata,
supporting both `ortensia-rust` and `dahlia`. Cargo's package version is internal and is
never used for the bundle. It builds
`--locked --release` for the native Mac architecture with a fresh target directory
and two compile jobs. This produces a native arm64 or x86_64 installer, not a
universal binary. Full Xcode and network access for pinned dependencies and Apple's
services are required. No remote development or test runner is used.

The bundle uses `assets/Info.plist` when present, retaining its bundle identifier
and minimum system version; otherwise it retains the compatible `dev.ortensia.rust`
identifier and macOS 12 minimum. Cargo supplies the executable; `--version` supplies the
version; the product name is Dahlia. Resources include the Dahlia icon, both Paper Mono fonts and their
verified provenance, plus Dahlia, Markraft and font licenses and generated third-party notices with their provenance manifest. A non-system dynamic
library fails packaging until it has an explicit bundling/signing implementation.

### Sparkle

The app updates itself with Sparkle 2. `scripts/prepare-sparkle.py` (run by packaging, not
by `prepare.sh`, so `cargo build` and `cargo test` never need it) downloads the pinned
Sparkle release into the ignored `.build/sparkle/<version>/` cache, verifies its SHA-256,
and extracts `Sparkle.framework` and Sparkle's `sign_update`, `generate_appcast` and
`generate_keys`. Packaging copies the whole framework, with its links and XPC services, into
`Contents/Frameworks` and adds Sparkle's licence to Resources. Nothing is removed from it.

For a non-sandboxed app, signing follows Sparkle's documentation and never uses `--deep`:
the two XPC services (entitlements preserved), `Autoupdate` and `Updater.app`, then
`Sparkle.framework`, then the app, each with the hardened runtime and a secure timestamp.
Verification checks the signature, team and hardened runtime of every one of them, in the
staged app and again in the mounted disk image.

The bundle's Info.plist gets `SUFeedURL`, `SUPublicEDKey` (read from the committed
`scripts/sparkle-public-key.txt`; packaging refuses a missing file or a placeholder),
`SUEnableAutomaticChecks` true, `SUScheduledCheckInterval` 86400 and `SUAutomaticallyUpdate`
false. Dahlia loads the framework at run time only when the bundle has it and a feed; see
`src/updates.rs`.

The workflow signs with hardened runtime and secure timestamp, submits a ZIP of
the app to Apple, requires `Accepted`, staples/verifies the app, creates and signs
a read-only compressed DMG, then notarizes/staples/verifies that exact DMG. It
mounts the image read-only without opening windows and checks the mounted app's
strict signature, identity/team, hardened runtime, ticket, Gatekeeper assessment,
and every file's bytes/mode and symlink against the accepted stapled app. It then
unmounts the image. A failure leaves no successful receipt; do not distribute
partial output. Apple submission results/logs survive for diagnosis. A timeout may
leave a submission still processing at Apple; inspect its submission ID before
re-running in a new output directory.

Successful output contains the DMG, `SHA256SUMS`, `receipt.json`, Apple submission
IDs/logs, and app/DMG/mounted verification logs. The receipt records source and
Markraft revisions, Cargo.lock hash, compiler/Xcode/SDK versions, signing identity,
architecture and the exact mounted app manifest. The procedure is repeatable;
secure timestamps, notarization tickets and DMG metadata prevent a promise of
bit-for-bit identical installers across runs.

A packaging receipt qualifies signatures and delivered bytes. Interactive
click/type/save/reopen, Classic/Glass screenshots, IME and other release acceptance
gates must still be checked against the signed artifact with isolated data and the
owner's permission before opening windows. Packaging never asserts that those
checks ran. Keep that evidence beside the receipt before distribution.

Run the packaging-only regression checks with:

```sh
python3 scripts/package-mac-test.py
python3 scripts/prepare-sparkle-test.py
python3 scripts/release-test.py
```

The tests use temporary files and mocks; they never build the real application,
access signing credentials, submit to Apple or mount a real image. Workflow follows
[Apple's notarization guidance](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
and [custom workflow guidance](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).

## Existing Xcode account workflow

The prior Dahlia 1.0.1 build was submitted through Xcode Organizer with the owner's existing account. Its archive is `Dahlia-cd5d739.xcarchive`, with app submission `530B714D-C133-4EEF-BC8C-CA413448A1C7`. An absent notarytool keychain profile does not prove those credentials are unavailable.

The script above intentionally requires a notarytool profile. When using the established Organizer workflow instead, stage a fresh archive of the exact reviewed source, distribute with Developer ID, wait for Apple's acceptance, and preserve the exported signed app and submission log. Verify that Xcode's re-signing retained the executable's Mach-O sections. Staple and strictly assess the exported app, then create/sign/notarize/staple the exact final DMG and verify its mounted payload. Never reuse the old accepted archive as the new release, and do not claim the scripted path ran when Organizer was used. Account secrets stay in Xcode/Keychain.

## Locked third-party notices

`assets/licenses/THIRD-PARTY-NOTICES.txt` contains the selected Cargo license texts, published author/source records and a supplement of upstream copyright files. `upstream.json` pins those recovered public texts by blob ID and SHA-256. `manifest.json` binds the notices to the exact Cargo.lock and native target. Packaging checks all hashes and stops before compiling if any attribution source is unresolved. Do not erase an unresolved entry to obtain a release receipt.

Regenerate after dependency/lock changes with the pinned developer tool:

```sh
cargo install --locked --features cli --version 0.9.2 cargo-about
python3 scripts/generate-licenses.py --target aarch64-apple-darwin
python3 scripts/generate-licenses-test.py
python3 scripts/package-mac-test.py
```

Use `x86_64-apple-darwin` for an Intel build. Cargo-about's generic SPDX fallback does not recover missing copyright notices. For `block`, `fxhash`, `leak`, `leaky-cow`, `mac` and `seahash`, the bundle includes the complete original registry source archive, verified against Cargo.lock. This preserves all metadata, source headers and notices actually published with those packages alongside their selected license terms. Archive hashes, paths and copies are checked before building and in the mounted app manifest. Existing recovered upstream texts are retained even when several packages share one notice.
