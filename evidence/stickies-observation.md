# Installed Apple Stickies observation — 2026-10-14

System: macOS 26.5.2 (25F84), installed Stickies 10.3. Native `cua_repl` app access succeeded. No new capture permission requested; no denial bypassed. The notarization task released desktop/Xcode before manipulation.

Observed existing `Untitled` note. No text changed. Window > Collapse changed the screenshot from 600 × 400 pixels to 600 × 24 pixels. The collapse button's Help changed to Expand, then back to Collapse after restoring the original expanded state. The close and collapse controls remain visible in the collapsed strip. These are screenshot dimensions; no screen-scale conversion to points is asserted.

Saved app-scoped captures: `stickies-expanded.png` and `stickies-collapsed.png`. Dominant captured expanded body pixel is RGB(252,245,167), dominant strip pixel RGB(251,235,96). Color-profile compositing means these are captured pixels, not authoritative system color constants.

**Exact perimeter is unmeasured.** The supported app screenshot ends at the content rectangle: all four collapsed capture corners equal the strip fill, and the two expanded bottom corners equal the body fill. It includes no surrounding pixels, exterior shadow or outer frame. Thickness, border color, corner coverage, active/inactive outer-frame differences and collapsed/expanded frame parity cannot be established from these captures. Do not report a 1-point black outline as matching Apple. The prototype outline is explicitly provisional.

Next required evidence: a permitted capture that includes the entire window frame plus surrounding desktop, at known display scale, both active/inactive and collapsed/expanded. Prior capture restrictions must be respected. Swift source and frozen release were not touched.
