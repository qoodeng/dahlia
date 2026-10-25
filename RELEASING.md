# Releasing Dahlia

A release is one command, run on the owner's Mac:

```sh
scripts/release.sh --notes notes.md
```

It builds `main`, signs and notarizes the app and disk image, signs the disk image for
Sparkle, writes the update feed, tags the commit and publishes a GitHub release. Rehearse
first with `scripts/release.sh --dry-run`. Packaging details are in
[docs/PACKAGING.md](docs/PACKAGING.md).

## How versions work

Releases are dated: `YYYY.MM.DDNN`, always zero-padded, where `NN` (01–99) counts that day's
releases. The first release on 25 October 2026 is `2026.10.2501`, the second `2026.10.2502`.

| Where | Value |
| --- | --- |
| `CFBundleShortVersionString` and `CFBundleVersion` | `2026.10.2501` (the same string; Sparkle compares it) |
| Git tag | `v2026.10.2501` |
| Release title | `Dahlia 2026.10.2501` |
| Disk image | `Dahlia-2026.10.2501.dmg` |

The script picks the version itself: today's local date, and the next `NN` after the highest
`v<today>NN` tag among the local and remote tags. `package-mac.py --version` rejects anything
that is not a real calendar date with a release number from 01 to 99.

`Cargo.toml`'s `version` (`1.0.0`) is internal. Semver forbids leading zeros, so it cannot hold
a date version; it never reaches the bundle and the app does not show it. `assets/Info.plist`
carries `0.0.0` development values that packaging replaces.

## One-time setup

Do these once, on the Mac that builds releases.

`scripts/setup-release.sh` walks through all of it step by step (it checks out `main`, verifies
the certificate, saves the notarization profile, creates and backs up the Sparkle key, and
commits the public key). The steps it follows:

1. **Check the Developer ID certificate.**
   `security find-identity -v -p codesigning` must list
   `Developer ID Application: Christian Stephens (H45U5SB635)`. If the keychain has more than
   one Developer ID identity, pass `--identity` to the release command.

2. **Store notarization credentials.** Create an app-specific password at
   <https://account.apple.com>, then:

   ```sh
   xcrun notarytool store-credentials dahlia-notary \
     --apple-id YOUR_APPLE_ID --team-id H45U5SB635
   ```

   It prompts for the password and saves a keychain profile; nothing goes in the repository.
   Name the profile to the release command with `--notary-profile dahlia-notary` or
   `export DAHLIA_NOTARY_PROFILE=dahlia-notary`.

3. **Create the Sparkle signing key.** Fetch Sparkle, then run its key tool once:

   ```sh
   python3 scripts/prepare-sparkle.py          # prints the directory it prepared
   .build/sparkle/2.10.1/payload/bin/generate_keys
   ```

   Allow the keychain prompt. The private key is stored in your login keychain (service
   `https://sparkle-project.org`, account `ed25519`). The tool prints the public key. Put it on
   its own line in `scripts/sparkle-public-key.txt` (the `#` comment lines may stay), then
   commit and merge that to `main`. Packaging and the release script refuse to run while the
   file has no real key, and the release script checks every disk image signature against it
   before publishing.

4. **Back up the private key.** If the keychain is lost, no installed copy of Dahlia can ever
   verify an update again; users would have to download a new build by hand.

   ```sh
   .build/sparkle/2.10.1/payload/bin/generate_keys -x ~/dahlia-sparkle-private-key
   ```

   Move that file to a password manager or an encrypted drive and delete the copy. To restore
   on another Mac: `generate_keys -f FILE`. Never commit it.

5. **GitHub.** `gh auth login` as an account that can create releases on `qoodeng/dahlia`.
   The update feed is `https://github.com/qoodeng/dahlia/releases/latest/download/appcast.xml`.
   Installed copies download it without signing in, so the repository must be public before the
   first release is installed by anyone else.

The first release with Sparkle has to be installed by hand; earlier builds have no updater.
After that, copies update themselves.

## Each release

1. Merge everything to `main` and pull. The release script requires `main`, a clean tree and
   an exact match with `origin/main`.
2. Write the notes in a Markdown file. They become the GitHub release body and, as simple
   HTML, the text in Sparkle's update window (headings, paragraphs, `-` lists, `**bold**`,
   `*italic*`, `` `code` `` and `http(s)` links; other markup is escaped).
3. Rehearse:

   ```sh
   scripts/release.sh --dry-run
   ```

   This runs every local, safe step: the version, the preconditions (reported, not enforced),
   a full release build with Sparkle embedded, an ad-hoc signature verified with `codesign`,
   and an `appcast.xml` with a placeholder signature. It prints the exact notarize, sign, tag
   and publish commands and runs none of them. Nothing is sent to Apple or GitHub. The build is
   under `~/Dahlia-releases/<version>-dry-run`; the dry-run disk image is not for distribution.
4. Release:

   ```sh
   scripts/release.sh --notes notes.md
   ```

   Allow the keychain prompt when `sign_update` asks to use the Sparkle key ("Always Allow"
   avoids it next time). Notarization takes minutes; Apple's services must be reachable.

The script, in order:

1. Checks the preconditions and reports all that fail: on `main`, clean, in sync with
   `origin/main`, `origin` is `qoodeng/dahlia`, `gh` signed in, a Developer ID identity,
   a notary profile, a real Sparkle public key, the private key in the keychain, notes given.
2. Computes the version.
3. Runs `scripts/package-mac.py`: clean clone of the exact commit, `cargo build --locked
   --release`, embeds and signs Sparkle then the app, notarizes and staples the app, builds and
   signs the disk image, notarizes and staples it, verifies the mounted copy byte for byte.
4. Signs the disk image with Sparkle's `sign_update` and verifies that signature against
   `scripts/sparkle-public-key.txt`. A mismatch stops everything: such an update would be
   rejected by every installed copy.
5. Writes `appcast.xml` beside the disk image.
6. Rechecks that `origin/main` has not moved and the tag does not exist, then creates and
   pushes `v<version>` and runs `gh release create` with the disk image and `appcast.xml`.
7. Confirms the release has exactly those two assets.

Everything stays in `~/Dahlia-releases/<version>` (receipt, Apple's notarization logs,
verification logs, `SHA256SUMS`). A packaging receipt qualifies signatures and bytes only; the
GUI acceptance checks in [docs/PACKAGING.md](docs/PACKAGING.md) are still the owner's.

After publishing, check the feed once: `curl -sIL
https://github.com/qoodeng/dahlia/releases/latest/download/appcast.xml | head -1` should end
in `200`. To test an update end to end, keep the previous release installed and use Dahlia ▸
Check for Updates…

### Why the feed is built this way

* **One item.** `releases/latest/download/appcast.xml` always serves the newest release's
  asset, so each feed describes only that release. Sparkle needs nothing older; there are no
  delta updates.
* **Notes embedded.** The notes are escaped HTML inside the item, rather than a
  `sparkle:releaseNotesLink` to the GitHub release page. That link would load GitHub's full
  page chrome in the update window and depend on a second URL staying valid; the embedded copy
  needs neither and cannot add markup. The GitHub release body carries the same Markdown.
* **Apple silicon.** A release built on an Apple silicon Mac adds
  `sparkle:hardwareRequirements` `arm64`, so an Intel Mac is never offered a build it cannot
  run natively. `minimumSystemVersion` comes from `LSMinimumSystemVersion` in
  `assets/Info.plist`.
* **Updates ask first.** `SUAutomaticallyUpdate` is false and `SUEnableAutomaticChecks` is true
  with a 24-hour interval, so Sparkle checks daily and asks before installing. It installs by
  quitting Dahlia through the normal quit path, which saves every note first.

## When publishing fails

Nothing public happens until step 6. If the build, notarization or signing fails, fix the
cause and run again; no tag exists. If step 6 fails, the disk image and appcast are kept in the
output directory, and the message says which command failed:

* **The tag was created locally but not pushed:** `git tag -d v<version>`, then run again.
* **The tag was pushed but there is no release:** either delete the tag
  (`git push origin :refs/tags/v<version>` and `git tag -d v<version>`) and run again, or
  finish by hand from the output directory:
  `gh release create v<version> --repo qoodeng/dahlia --title "Dahlia <version>" --notes-file notes.md --verify-tag --latest Dahlia-<version>.dmg appcast.xml`.
* **The release exists but an asset is missing:**
  `gh release upload v<version> --repo qoodeng/dahlia FILE`.
* **A bad release is public:** deleting it (or marking it a pre-release) makes the previous
  release `latest` again, and its feed with it. Installed copies that already updated stay
  where they are; ship a fixed `NN+1` for them.

## Compatibility rule

Older builds refuse a notes list newer than they understand rather than rebuild it, and
automatic updates make it ordinary for a library written by a new build to meet an old one
(restoring a backup, running a second copy). So **any change an older build cannot read must
bump the notes-list `version`**: a new enum variant (a new appearance, say), a new
required field, a changed meaning for an existing field. The version is the one in the list
of notes in `src/notebook`: written where `Index { version: 1, … }` is built (`mod.rs`,
`recovery.rs`) and checked by `parse_index` and its validation in `recovery.rs` (`saved.version
> 1`). Raise it in both places and add a test that the old value is refused. Additive,
optional fields with a `#[serde(default)]` that older builds can ignore safely do not need it.

## Updating Sparkle

`scripts/prepare-sparkle.py` pins the version and the SHA-256 of the release tarball
(`Sparkle-<version>.tar.xz` from <https://github.com/sparkle-project/Sparkle/releases>).
To update, download the tarball, compute its SHA-256 yourself, change both constants, run the
three Python test files and a `--dry-run`. The cache is `.build/sparkle/<version>/` and is
ignored by git; it is re-verified on every use.

## Tests

```sh
python3 scripts/package-mac-test.py
python3 scripts/prepare-sparkle-test.py
python3 scripts/release-test.py
```

They use temporary files and fakes. They never sign, notarize, tag, publish, touch the
keychain or use the network. What they cannot show, and only a real release does: Apple
accepting the submission, Sparkle's window finding and installing an update, and the keychain
prompt for the Sparkle key.
