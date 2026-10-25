# Mac deliverable verification

App source/build commit: `ff06c6e622f1251fa6a8fd96fcd65ae87b288494`; lifecycle implementation commit `245f42c64cd1ccdbffd995067972a6c7015241a8`. Version 0.1.0, Apple Silicon development build, not labeled 1.0. Existing hydrangea icon was copied unchanged from the frozen Swift archive into `assets/brand/ortensia.icns` and the new Rust bundle; Swift archive/storage remain untouched.

The exact packaged executable launched successfully on macOS 26.5.2 against a separate synthetic note-data directory and reported Helvetica Neue / Paper Mono. The corrected lifecycle executable passed native Cancel, Archive/restore, Delete/Undo Delete, import and Export and Archive; input/stored/exported Markdown matched all 132 bytes. All twelve focused tests and owned-source Clippy checks passed. Details in `stage4-verification.md`.

Signing: **ad hoc**, no Developer ID, no TeamIdentifier, not notarized. `codesign --verify --deep --strict` passed both the packaged app and its extracted ZIP copy. Packaging initially found copied FinderInfo/resource-fork metadata; only those metadata attributes on the new Rust bundle were removed and signature verification then passed. No quarantine/security setting or Swift resource was changed. The ZIP is produced without resource forks/extended attributes, includes licenses and build commit, and contains no runtime notes or source repository.

Archive: `build/ortensia-rust-macos-dev.zip`, 19,504,615 bytes. SHA-256 `7236b517def63029d0723b73e94724e828f4dc7fa915d8e34adfdd1e3196ed51`.
Library artifact: `libfile_c1f2578b22cc81918a2e7416c0def561`, version 0; underlying file `file_0000000050b881f595f31531f32291c9`. Successful upload confirmed.

Consequential limits: exact installed Stickies exterior border remains unverified; geometry-only investigation did not supply exterior pixels, and unavailable/denied capture routes were not bypassed. See `stickies-border-status.md`. Portable attachments, full Recovery/checkpoint UI and broader Swift/platform parity remain in `PARITY.md`. User data defaults to the isolated Rust application-data directory. No speed, universal platform, or production-release claim.
