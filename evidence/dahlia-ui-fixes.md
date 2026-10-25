# Dahlia 1.0.1 UI fixes and native validation

Task checkboxes use 0.8 times the reading size (9.6 points at 12-point text). The box centers on the actual font cap-height band. Completed tasks and ordinary strikethrough use cap-height/x-height metrics, preserving partial mark ranges and wrapped rows. Pointer/accessibility targets remain at least 14 points, excluding text. These are optional Markraft style hooks applied to the same pinned MIT editor; source/history machinery remains upstream.

## Installed Apple Stickies comparison

Used a new synthetic Apple Stickies note, without editing existing notes. On this Retina display, Stickies screenshots measured a 12-point strip, 8-point glyphs, close and collapse centers 16 points from their edge, and zoom 28 points from the right. Dahlia previously put zoom 38 points from the right. Collapsed title ink previously began at x57/y8 physical pixels; corrected Dahlia starts x43/y4 versus Apple x44/y4. Controls now use the measured centers; the title uses an 11-point system font and a 12-point line height. The app retains native AppKit frame masks 14 expanded and 6 collapsed. Screenshot APIs crop to content, so exact exterior border thickness/color/corner parity remains unmeasured; no exact whole-window pixel parity is claimed. See stickies-native-frame.md for the frame investigation and access limitations.

## Window sizing

Removed the 2000-point size ceiling that prevented filling larger logical screens; a 32768-point sanity limit remains, with restoration clamped to visible display bounds. Persisted macOS frames now normalize against the primary screen origin, matching placement, rather than attached-screen height. On this Mac the zoomed content fills 1470 x 865 logical points (2940 x 1730 backing pixels). Six collapse/expand cycles at normal size and six while zoomed retained position and expanded height. A 3200 x 2400-point synthetic-display regression checks restoration, negative display origin, and collapsed width/top. Only one physical display is available; secondary-display moves and different DPI screens were not physically tested.

## Shortcuts and menus

The native Format menu exposes the existing editor commands. Editor commands are scoped to the note editor, excluding search/recovery inputs. Colors move to Command-Control-1 through 6 to avoid heading collisions. Format check state is cached outside the Note entity and menu refresh is deferred, fixing the reentrant Note read that crashed the earlier preview. All AppState initializers, including recovery tests, carry the cache.

| Command | Shortcut |
| --- | --- |
| Bigger / smaller / reset to 12 points | Command-= or Command-+, Command--, Command-0 |
| Bold / italic | Command-B / Command-I |
| Inline code / strikethrough | Command-Shift-C / Command-Shift-X |
| Paragraph / heading 1 through 6 | Command-Option-0 / Command-Option-1 through 6 |
| Numbered / bullet / task list | Command-Shift-7 / 8 / 9 on US layout |
| Toggle task at caret | Command-Return |
| Quote / code block | Command-Shift-B / Command-Option-C |
| Note color 1 through 6 | Command-Control-1 through 6 |

The upstream Command-1 through 6 heading aliases remain. Use Selection for Find retains Command-E. No new link command is invented; existing editor link UI remains.

## Verification

37 app tests pass, plus the macOS style-transition keyboard-receiver integration test. Six focused editor tests pass: all reading sizes 9 through 48, nested checked/unchecked task geometry, wrapped and partial strikethrough, padded target toggling, and right-click behavior. The editor-only test workspace uses only the six required library crates, not the upstream desktop app.

Native preview checks passed: reading size 9, 12, 13 and 48; checkbox and nested checkbox accessibility toggles; native Format-menu Bold followed by real key typing and undo; italic, inline code, strikethrough, heading/paragraph shortcuts and undo; task conversion and caret Command-Return; Classic/Glass rendering; collapse/expand and zoom. The reproduced menu/edit sequence completed after the fix with no panic in the native log. Native menu AX does not expose shortcut badges/check-state pixels, so their values are checked from actual registered menu/binding source, not inferred from AX.

A padded coordinate-click attempt was rejected by the UI tool with windowNotFoundAtPosition((137.0,167.5)); it was not bypassed. The equivalent padded-hit interaction passes in GPUI tests, and native AX checkbox toggles pass. Packaged-app typing and signing/notarization results are recorded separately with the delivered installer receipt.
