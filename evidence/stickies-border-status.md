# Installed Stickies exterior-border investigation

Update: the native frame correction is implemented using installed Stickies' verified window-style flags. See `stickies-native-frame.md` and `stickies-window-template.json`; exterior pixel sampling remains unavailable. The paragraphs below record the earlier geometry-only investigation.

macOS 26.5.2, Stickies 10.3, display scale 2. Public `CGWindowListCopyWindowInfo` metadata read successfully without pixel capture, note-title access or note-content access. The expanded observed window bounds include 300×200-point windows, matching earlier 600×400-pixel content captures. The recorded window sharing-state values are 0. Geometry alone does not identify an exterior border's thickness, color or corner coverage.

The available documented CUA screenshot methods provide the previously observed content-only region; there is no full-desktop/full-frame option in that API. The only separate screen-context capture tool exposed here is restricted to active voice conversations and cannot be called in this text task. No previously denied capture route was retried or bypassed. The installed resource catalog provided icon metadata, not a window-perimeter definition. Compiled NIB inspection is not exterior-pixel proof.

**Exact exterior match remains unverified.** The Rust Classic outline still follows current read-only Swift source (1-point black, square), not a measured installed Stickies border. Expanded/collapsed active/inactive exterior pixels at known scale remain the required evidence. No speculative border change has been labeled exact.
