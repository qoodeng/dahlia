# Dahlia design

How a note looks and why. Values come from `src/appearance.rs` (`editor_style`, the palettes and the Glass mapping), `src/chrome.rs` (strip controls, rim), `src/panel.rs` (auxiliary windows) and `src/platform.rs` (AppKit frame and material). Tests in those files check the contrast and scaling rules below.

## The note shell

A note is a floating window with a 12-point strip and no title bar text. Classic copies installed Stickies: paper, spine and control colours (`BODY`, `SPINE`, `CONTROL`, Yellow `#FEF49C` / `#FEEA3D` / `#DBC503` and five more), AppKit's own window frame with style mask 14 (Closable, Miniaturizable, Resizable; 6 while collapsed), and no rule under the strip. Other platforms paint a provisional 1-point black outline instead of the AppKit frame. The colours are Stickies 10.3's named sRGB colours, read from its asset catalog (`evidence/stickies-colors.json`, checked by a test). On macOS GPUI's Metal layers are tagged sRGB so Core Animation colour-matches them to the display as it does for Stickies; untagged, they reached a Display P3 screen as raw values and looked more saturated.

Strip controls are 8-point glyphs (1-point strokes) in adjacent 12-point cells: close at 10 points from the left, collapse at 10 and zoom at 22 from the right. Glyphs match installed Stickies (measured from `evidence/stickies-collapsed.png`) and have no black outline: the 1-point line starts from Stickies' own control colour for the note's colour (`CONTROL`, `StickiesControl<Color>Color` in its asset catalog, Yellow `#DBC503`) and is taken only as far towards black as it takes to reach 3.5:1 on the strip (Yellow `#887B02`), because Stickies' own colours are 1.4 to 1.9:1 on their strips and these are the only way to close, zoom or collapse a note; it surrounds a face of the note's paper that lightens towards its upper middle by up to 20% white, as Stickies' faces do. The collapse rule sits 2.5 points under the box's top line at 2x and snaps to 2 at 1x. Glass uses the same glyphs, since its strip keeps the opaque spine colour. While the note is not the key window the line fades towards the spine, opaquely, by at most half and never below 3.1:1. Increase Contrast darkens it to 7:1 at rest and 4.6:1 faded. A test checks every colour in all three appearances, active and inactive, with and without Increase Contrast. On Linux the in-window menu control is one more glyph of the set: the same paper box and line with a small pull-down mark inside. The Glass spine is rounded with the note's corners, so on Linux, where no window frame clips it, it does not show past the rim. Each control is named for VoiceOver (Close, Zoom, Collapse or Expand) and has a hover tooltip with its name and key on the Mac (“Close ⌘W”, “Zoom ⇧⌘M”, “Collapse ⌘M”); a collapsed note's 12-point window is too short to show one. A control darkens the moment it is pressed, acts on release, and shows a focus ring in the link ink (`#083B96`) when reached from the keyboard; GPUI clicks a focused control on Return or Space. Double-clicking the strip follows the system “Double-click a window’s title bar to” preference (Zoom, Minimize or None) and collapses the note when that is not set, as Stickies does; the outer 6 points at each end are left for resizing.

The collapsed strip shows the note's first line in the 10-point system font (9 points on Linux and Windows, whose taller UI faces would otherwise clip descenders in the 12-point strip), 26 points in from both sides, vertically centred, in 85% black on Glass. A title longer than the strip ends in an ellipsis; titles are kept to 60 characters, and a longer first line ends at its last whole word with an ellipsis. An error replaces it until it is resolved.

The editor is announced to VoiceOver by the note's title. Text that appears without taking focus is spoken too: a note's error and the Find and Recovery status lines post an `NSAccessibilityAnnouncementRequestedNotification` at high priority (`platform::announce`), once per message until it clears, because GPUI 0.3.6 has no live regions.

## Text in a note

Prose is Helvetica Neue (the system font where it is missing) and code is bundled Paper Mono, through the owned Markraft style hooks. The default reading size is 12 points, from 9 to 48 in 1-point steps (Font ▸ Bigger and Smaller).

Every distance is an em of the reading size, rounded to whole points, so 9 and 48 points are the same note scaled:

| Role | Value | At 12 pt |
|---|---|---|
| Body line | 1.25, rounded to a whole-point line | 15 |
| Heading line | 1.1 | |
| Code line | 1.3 | |
| Paragraph gap | 0.6 em | 7 |
| List item gap | 0.25 em | 3 |
| Space above Title, Heading, Subheading, H4–H6 | 1.0, 0.83, 0.67, 0.5, 0.5, 0.5 em | 12, 10, 8, 6, 6, 6 |
| Space below a heading | 0.33 em | 4 |
| List and quote indent (one text column) | 1.5 em | 18 |
| Code size | 0.875 em, at least 9 pt | 10.5 |
| Code panel padding / inset | 0.67 em / 0.5 em | 8 / 6 |
| Inline code padding | 0.25 em | 3 |
| Table cell padding | 0.6 em × 0.33 em | 7 × 4 |
| Quote bar | 0.2 em (2–6 pt), black at 25% | 2 |
| Rule thickness | 0.08 em, at least 1 pt | 1 |
| Underline and strike | 0.06 em, at least 1 px | 1 |
| Measure in a zoomed note | 36 em | 432 |

Headings follow the Format menu's Title / Heading / Subheading scale, 1.67, 1.33, 1.17, 1.08, 1 and 1 em: 20, 16, 14, 13, 12 and 12 points at the default size, all bold. Title, Heading, Subheading and Body are in the Format menu; Heading 4 to 6 are under More Heading Levels.

The side margin is Stickies' 10 points (a 6-point inset plus 4 points of editor padding) at every size, with the same 10 points below the last line. Tables sit directly under the block above; Markraft's toolbar row is zero because Dahlia draws no table toolbar.

List markers share one column. A task box is 0.8 em and ends 0.4 em before its text, a bullet (0.36 em) is centred on the same point, and an ordinal ends where the box does; boxes and bullets sit on the cap-height band. Hollow and square bullets mark deeper levels.

The code panel's corner radius is 0.5 em and never more than 6 points, so it is never rounder than the Glass window around it (Sea Glass's 18-point window allows up to 18); inline code and images use 0.3 em, also at most 6.

## Colour on six papers

Body text and markers are pure black: Stickies' ink, and full strength for Glass. Sea Glass takes its palette ink instead (see Sea Glass). Every other role is either a fixed dark ink or translucent black, so it takes each paper's hue instead of sitting on it as neutral grey:

| Role | Value |
|---|---|
| Muted text (completed tasks, revealed markup) | black at 65% |
| Link | `#083B96` |
| Code well / inline code pill | black at 5% / 6% |
| Inline code text | black at 80% |
| Table header band | black at 5% |
| Rules and table grid | black at 30%: quiet separators, 1.7–3:1 on every paper |
| Selection | the user's highlight colour as a 45% fill; blue `#0064E1` at 30% when that would vanish on the paper |
| Unfocused selection | black at 12% |
| Find hit / highlight | system orange at 50% / 40% |
| Syntax: keyword, name, string, comment | `#2C08BB`, `#053684`, `#0F4425`, `#393B3E` |

`every_text_role_is_readable_on_all_six_papers_in_classic_and_glass` checks each text role at 4.5:1 or more on every paper, in Classic and in Glass at the default transparency and at the 30% paper floor over a white and a near-black (`#1E1E1E`) desktop, and that selection, find and the code well stand apart from the paper.

## Glass

Glass is the default appearance. The window shows AppKit's light Popover material at full strength whenever Glass is on, and never masks or hides it: the desktop is always blurred behind the text. The transparency setting (0–100%, default 56%; stored as a tint of 0–0.5, default 0.22) only decides how much paper colour tints the blur, from 75% opaque at 0% transparency to 30% at 100%. The 30% floor keeps every text role readable, syntax colours included, and the six colours distinguishable; Increase Contrast raises the paper to at least 85%. The contrast test models the material as lightening what is behind it by 60%.

Corners are 6 points. AppKit recomputes the window shadow after the corners are rounded. A rim gives a pale note a shape on a pale desktop: a 1-pixel light line inside (white at 45%) and a faint dark one outside (black at 12%), or one black line at 50% under Increase Contrast. The strip carries the note's spine colour, opaque (colour on a solid layer, not the translucent body), with a hairline (black at 12%) under it.

Reduce Transparency removes the material and makes the paper opaque. Reduce Transparency, Increase Contrast and the accent and highlight colours are re-read for every open note and redrawn in every panel the moment the system posts the change (the workspace's accessibility-display-options and `NSSystemColorsDidChangeNotification`), and again whenever a note becomes active. Notes keep the light (Aqua) appearance in Dark Mode, as Stickies does. The separate Window ▸ Translucent command applies native window alpha 0.75.

## Sea Glass

Sea Glass is a third appearance, a native approximation of a sea-glass material rather than a copy of its shader. The six note colours map to pastel palettes (amber `#F0D9BE`, lagoon `#B5D0EB`, mint `#C0DED0`, rose `#EECED6`, lilac `#E1CDE8`, neutral `#E8ECEA`, with matching edge and ink colours in `SEA_BODY`, `SEA_EDGE` and `SEA_INK`), over a lighter `UnderWindowBackground` material instead of Popover, in an 18-point clipped silhouette. So the strip controls, collapsed title and text clear that rounder corner, they sit an extra half of the additional radius inward (`NoteStyle::edge_inset`, 6 points; 0 for Classic and Glass), and the text keeps the same 6 points clear of the bottom corners. It shares the Glass transparency setting, the Reduce Transparency and Increase Contrast handling, the editor instance and the Linux and Windows fallbacks. The paper is a much denser sheet than Glass's: its opacity is 61% plus 33% × tint / 0.5, so 61% at 100% transparency and 94% at 0% (Glass is 30% to 75%), and Increase Contrast raises it to at least 85%. The 61% floor is the lowest at which the readability test passes with the text shade at 40%: a floor of 55% was asked for, but selected text on lagoon over a dark desktop is 4.4:1 there. The strip is the palette edge colour at 72%, over a hairline of the ink at 20%. A white 135° sheen runs from 20% to 0% across the paper, two inset shadows (white at 70%, black at 8%) and the shared rim edge the silhouette, and the strip glyphs have a face of the edge colour at 35% and a line of the ink, darkened only if needed to hold the same ratios against the strip's worst case. The palette ink is the control and title colour. Amber, lagoon and mint ink is only 4.1 to 4.5:1 on its own paper even when opaque, so body text and markers use it taken 40% towards black (`SEA_TEXT_SHADE`); the readability test passes every text role at 4.5:1 or more on all six Sea Glass papers at every transparency, with and without Increase Contrast, and opaque, under the same material model as Glass. That model is an assumption for `UnderWindowBackground`, which has not been measured. The shader's tumbled contour, 3D bulge, moving reflection and procedural grain are not reproduced, and none of this has been checked on a screen.

## Menus and keys

The app menu is Dahlia: About Dahlia, Settings… (⌘,), Services, Hide Dahlia (⌘H), Hide Others (⌥⌘H), Show All and Quit Dahlia (⌘Q). Edit has Undo, Redo, Undo and Redo Delete Note, Cut, Copy, Paste, Paste and Match Style (⌥⇧⌘V; ⇧⌘V also works), Select All, a Find submenu (Find… ⌘F, Find and Replace…, Find Next ⌘G, Find Previous ⇧⌘G, Use Selection for Find ⌘E, Jump to Selection ⌘J), Spelling and Grammar (Check Document Now ⌘;), and Substitutions. Color, Appearance, Format, Font and Window follow Stickies, shortcuts included (read from Stickies 10.3's MainMenu.nib): the colours are ⌘1–⌘6, Window ▸ Collapse or Expand (named for what it will do) is ⌘M, Zoom ⇧⌘M, Collapse All ⌥⌘M, Undo and Redo Arrange ⌥⌘Z and ⌥⇧⌘Z, Float on Top ⌥⌘F and Translucent ⌥⌘T. As in Stickies, Minimize has no shortcut and Find and Replace gives ⌥⌘F to Float on Top; its panel is the Find panel. Help ▸ Dahlia Help is ⌘?. Help ▸ Dahlia Help explains what Stickies users would not find by looking.

Inside a note ⇧⌘C is Inline Code and ⌥⌘C Code Block (notes have no styles for Stickies' Copy Style to copy); headings are ⌥⌘1–⌥⌘6 and Body ⌥⌘0, since ⌘1–⌘6 are the colours. Linux uses Ctrl for ⌘, except where its desktops expect otherwise or reserve the chord: Replace Ctrl+H, Code Block Ctrl+Shift+K, Choose Code Language Ctrl+Shift+L (Ctrl+Alt+L locks KDE, Xfce and Cinnamon), Translucent Ctrl+Shift+T (Ctrl+Alt+T opens GNOME's terminal), Help F1 and the menu F10; Markraft's Mac Emacs keys Ctrl+D, Ctrl+K and Ctrl+P are off there. `keys::tests::shortcuts_follow_the_audit` pins all of this.

Linux and Windows have no menu bar, so the same menus open inside the note (`src/linux_menu.rs`). They are built by the same `app_menus` as the Mac menu bar, so the two never list different items or states. A 12-point menu glyph sits left of zoom in the expanded strip, using the chrome's ink, dimming and focus ring; F10 also opens it, expanding a collapsed note first. The panel opens under the strip at the right, 212 points wide, in the note's paper. Glass gets 6-point corners, a 20% ink border and a soft shadow; Classic gets square corners and a black 1-point border. It shows one level at a time with a back row, 12-point text, check marks, submenu arrows and the shortcut that applies in the note. Arrows, Home/End, Enter/Space/Right, Left/Backspace and first-letter typing navigate it; Esc, F10 or a click outside close it. macOS keeps its native menu bar and never shows the control. Shortcut labels skip Super bindings, so only Ctrl shortcuts appear, and items the platform cannot perform (Services, app hiding, Arrange By with its policy and undo) are left out rather than shown disabled. Alerts on these platforms use `src/prompt.rs`: a card up to 288 points wide that narrows to the window, wraps the message and explanation, scrolls in short windows, starts Return on the first non-Cancel, non-destructive button and answers Esc with Cancel.

## Dahlia's own windows

Settings (the transparency shared by Glass and Sea Glass; the window is titled Dahlia Settings, opened by Settings… and by Appearance ▸ Transparency Settings…, with a Transparency row, Solid and Clear under the ends of the slider and the caption “Applies to Glass and Sea Glass notes.”), Find and Replace, and Recovery use AppKit's control metrics (13-point text, 22-point fields and buttons) and a light or dark palette from the window's appearance, with contrast checked by a test. All close on Escape and ⌘W. While any note is Float on Top (window level 3), they sit one level above it (level 4), so a panel is never hidden under a note it serves; otherwise they are ordinary windows. Find lays out like AppKit's Find panel, with Next as the default button on Return; Recovery uses a segmented mode control (Archived Notes, History, Trash), accent selection while a list has the keyboard and grey when it does not, a Paper Mono preview and Restore as the default button. The panels follow the user's accent colour (`NSColor.controlAccentColor`, redrawn when the system says it changed), falling back to blue `#0064E1`: the default button, selected rows and segments and the slider are the accent darkened just enough for white text at 4.5:1, and the focus ring is that accent, lightened in Dark Mode to 3:1 against the window. A test runs the eight system accents in light and dark. The default button is filled with the accent, so its keyboard focus ring has two tones: white inside against the fill and the text colour outside against the window. Segmented controls (Find's scope, Recovery's mode) are radio buttons that report which segment is on.

## Messages

An error inside a note appears below the text as interface, not note text: the 11-point system font in 85% black over a hairline, with bordered buttons. A failed save offers Try Again; a note another app changed offers Keep Mine as a Copy and Reload; anything else offers Dismiss. Wording follows Clean Copy: what didn't happen, then the next step, with contractions, curly-quoted file names and no storage vocabulary (`src/copy.rs`). Internal causes go to the log and never reach an alert. One word per thing: a note's text (not source), Dahlia's list of notes (not notebook or index), a snapshot (File ▸ Save Snapshot; History is only the name of Recovery's tab), Archived Notes, Trash, and Forget (remove from Dahlia's lists while the files stay, still findable in History). Menu items that open a window needing no further input have no ellipsis (Recovery); Settings… and Transparency Settings… keep theirs, as the Mac does for settings.

## Not yet verified

Paragraph and list spacing have not been matched pixel for pixel against Stickies. Classic's exterior frame pixels and inactive appearance are unmeasured (see `evidence/stickies-native-frame.md`). Font fallback on Linux and Windows, and their GPUI backdrop and opacity fallbacks for Glass, have no native verification.
