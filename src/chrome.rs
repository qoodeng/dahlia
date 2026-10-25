use gpui::{prelude::*, *};

pub use crate::notebook::STRIP_HEIGHT as HEIGHT;

/// The collapsed strip's title size. San Francisco's ascent and descent at 10 points fill the
/// 12-point strip exactly; the taller Linux and Windows UI faces need a slightly smaller size
/// for their descenders to stay inside it.
pub const TITLE_SIZE: f32 = if cfg!(target_os = "macos") { 10. } else { 9. };

#[derive(Clone, Copy)]
pub enum Control {
    Close,
    Zoom,
    Collapse,
}

/// The keyboard focus ring for Dahlia's own controls: the link ink, at least 5:1 on every
/// paper.
pub const FOCUS_RING: u32 = 0x083b96;

/// How much of the paper Sea Glass's strip veil lets through: its edge colour at this
/// opacity over the pastel.
pub const SEA_STRIP_VEIL: f32 = 0.72;

/// The contrast every strip glyph's line reaches against its strip (WCAG's non-text minimum
/// is 3:1): at rest, with a margin; while the note is not the key window, when the line is
/// lighter but still clear of 3:1; and the same two under Increase Contrast, which asks for
/// a firm line.
const LINE_CONTRAST: f32 = 3.5;
const LINE_CONTRAST_FADED: f32 = 3.1;
const LINE_CONTRAST_INCREASED: f32 = 7.;
const LINE_CONTRAST_INCREASED_FADED: f32 = 4.6;

/// What the glyph line is judged against: Classic and Glass's opaque spine, or Sea Glass's
/// veil over its paper at the slider's clearest, over the darkest backdrop the material
/// lets through (the light material is 60% white over a black desktop).
fn strip_colour(style: &crate::appearance::NoteStyle) -> Rgba {
    use crate::appearance::{SEA_PAPER_FLOOR, over};
    if style.sea_glass() {
        let material = over(Hsla::from(rgb(0xffffff)).opacity(0.6), rgb(0x000000));
        let paper = over(
            Hsla::from(rgb(style.sea_body())).opacity(SEA_PAPER_FLOOR),
            material,
        );
        over(
            Hsla::from(rgb(style.sea_edge())).opacity(SEA_STRIP_VEIL),
            paper,
        )
    } else {
        rgb(style.spine())
    }
}

fn mix(from: Rgba, to: Rgba, amount: f32) -> Rgba {
    Rgba {
        r: from.r + (to.r - from.r) * amount,
        g: from.g + (to.g - from.g) * amount,
        b: from.b + (to.b - from.b) * amount,
        a: 1.,
    }
}

/// The smallest `amount` in `0..=1` for which `holds` is true, given that it is true at 1 and
/// grows with `amount`.
fn smallest(holds: impl Fn(f32) -> bool) -> f32 {
    let (mut low, mut high) = (0., 1.);
    for _ in 0..24 {
        let middle = (low + high) / 2.;
        if holds(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// The strip glyphs' line colour: no black outline, but installed Stickies' own control
/// colour for the note's colour (`appearance::CONTROL`, its `StickiesControl<Color>Color`),
/// taken only as far towards black as it takes to reach `LINE_CONTRAST` on the strip, since
/// the Stickies colours themselves are 1.4 to 1.9:1 and the controls are the only way to
/// close, zoom or collapse a note. Glass uses the same glyphs because its strip keeps the
/// opaque spine colour. While the note is not the key window the line fades towards the
/// strip, as AppKit's title-bar controls dim, for as long as it keeps `LINE_CONTRAST_FADED`.
/// Increase Contrast darkens both further. Sea Glass draws the line in its palette ink and
/// fades towards its pale edge colour.
pub fn glyph_ink(style: &crate::appearance::NoteStyle, active: bool, increase_contrast: bool) -> Hsla {
    use crate::appearance::contrast;
    let strip = strip_colour(style);
    let base = rgb(if style.sea_glass() {
        style.sea_ink()
    } else {
        style.control()
    });
    let (rest, faded) = if increase_contrast {
        (LINE_CONTRAST_INCREASED, LINE_CONTRAST_INCREASED_FADED)
    } else {
        (LINE_CONTRAST, LINE_CONTRAST_FADED)
    };
    let black = rgb(0x000000);
    let line = mix(
        base,
        black,
        smallest(|amount| contrast(mix(base, black, amount), strip) >= rest),
    );
    if active {
        return line.into();
    }
    // Back towards the strip, but at most halfway and opaque: a translucent line would read
    // two-toned where its inner half lies over the paper face and its outer half over the
    // spine.
    let kept = smallest(|kept| contrast(mix(line, strip, 0.5 * (1. - kept)), strip) >= faded);
    mix(line, strip, 0.5 * (1. - kept)).into()
}

/// The glyph's face: the note's paper, as in Stickies. Sea Glass uses a veil of its edge colour.
pub fn glyph_fill(style: &crate::appearance::NoteStyle) -> Hsla {
    if style.sea_glass() {
        Hsla::from(rgb(style.sea_edge())).opacity(0.35)
    } else {
        rgb(style.body()).into()
    }
}

/// Stickies lightens the middle of each glyph's face, brightest a third of the way down:
/// in the capture the paper's blue channel rises from `0x9C` at the edges to about `0xB4`,
/// roughly a fifth of the way to white.
const HIGHLIGHT: f32 = 0.2;
const HIGHLIGHT_AT: f32 = 0.4;

/// Fill `path`'s face with the paper and its highlight, in two bands that meet at the
/// highlight's peak.
fn paint_face(window: &mut Window, points: &[Point<Pixels>], fill: Hsla, top: Pixels, height: Pixels) {
    let build = || {
        let mut path = PathBuilder::fill();
        path.move_to(points[0]);
        for p in &points[1..] {
            path.line_to(*p);
        }
        path.close();
        path.build().ok()
    };
    if let Some(path) = build() {
        window.paint_path(path, fill);
    }
    let white: Hsla = rgb(0xffffff).into();
    let split = top + height * HIGHLIGHT_AT;
    let bands = [
        (top, split, 180., white.opacity(0.), white.opacity(HIGHLIGHT)),
        (split, top + height, 180., white.opacity(HIGHLIGHT), white.opacity(0.)),
    ];
    for (from, to, angle, start, end) in bands {
        let left = points.iter().map(|p| p.x).fold(px(f32::MAX), |a, b| a.min(b));
        let right = points.iter().map(|p| p.x).fold(px(f32::MIN), |a, b| a.max(b));
        let mask = ContentMask {
            bounds: Bounds::from_corners(point(left, from), point(right, to)),
        };
        window.with_content_mask(Some(mask), |window| {
            if let Some(path) = build() {
                window.paint_path(
                    path,
                    linear_gradient(
                        angle,
                        linear_color_stop(start, 0.),
                        linear_color_stop(end, 1.),
                    ),
                );
            }
        });
    }
}

/// Stroke a polyline (closed when `close`) in the glyph ink.
fn paint_line(window: &mut Window, points: &[Point<Pixels>], close: bool, ink: Hsla) {
    let mut path = PathBuilder::stroke(px(1.));
    path.move_to(points[0]);
    for p in &points[1..] {
        path.line_to(*p);
    }
    if close {
        path.close();
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, ink);
    }
}

/// The glyphs all share one shape language: an 8-point box (or the zoom triangle inside it)
/// with a 1-point line in the Stickies control colour around a face of paper. `kind` adds its mark.
pub enum Glyph {
    Box,
    Zoom,
    Collapse,
    /// The Linux in-window menu control.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Menu,
}

/// Paint `glyph` in the 12-point cell at `bounds`.
pub fn paint_glyph(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    glyph: Glyph,
    fill: Hsla,
    ink: Hsla,
) {
    // Line centres sit on half points so a 1-point line covers whole pixels at 1x.
    let x = bounds.origin.x + px(2.5);
    let y = bounds.origin.y + px(2.5);
    let points = match glyph {
        Glyph::Zoom => vec![
            point(x, y + px(7.)),
            point(x + px(7.), y),
            point(x + px(7.), y + px(7.)),
        ],
        _ => vec![
            point(x, y),
            point(x + px(7.), y),
            point(x + px(7.), y + px(7.)),
            point(x, y + px(7.)),
        ],
    };
    paint_face(window, &points, fill, y, px(7.));
    paint_line(window, &points, true, ink);
    match glyph {
        // Stickies' rule sits 2.5 points below the box's top line; at 1x it snaps to 2 so
        // it stays one crisp pixel.
        Glyph::Collapse => {
            let drop = if window.scale_factor() >= 2. { 2.5 } else { 2. };
            paint_line(
                window,
                &[point(x, y + px(drop)), point(x + px(7.), y + px(drop))],
                false,
                ink,
            );
        }
        // A pull-down mark inside the box, so the menu control reads as one of the set
        // and never as a second collapse box.
        Glyph::Menu => {
            let mut path = PathBuilder::fill();
            path.move_to(point(x + px(1.5), y + px(2.5)));
            path.line_to(point(x + px(5.5), y + px(2.5)));
            path.line_to(point(x + px(3.5), y + px(4.5)));
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, ink);
            }
        }
        Glyph::Box | Glyph::Zoom => {}
    }
}

/// What a control is called to VoiceOver and in its tooltip.
fn name(kind: Control, collapsed: bool) -> &'static str {
    match kind {
        Control::Close => "Close",
        Control::Zoom => "Zoom",
        Control::Collapse if collapsed => "Expand",
        Control::Collapse => "Collapse",
    }
}

/// A control's tooltip: its name and, on the Mac, the key that does the same.
fn hint(kind: Control, collapsed: bool) -> &'static str {
    match (kind, collapsed, cfg!(target_os = "macos")) {
        (Control::Close, _, true) => "Close ⌘W",
        (Control::Zoom, _, true) => "Zoom ⇧⌘M",
        (Control::Collapse, true, true) => "Expand ⌘M",
        (Control::Collapse, false, true) => "Collapse ⌘M",
        (kind, collapsed, false) => name(kind, collapsed),
    }
}

/// The hover tooltip for a strip control, in the panels' window colours.
struct Tip(&'static str);
impl Render for Tip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let palette = crate::panel::Palette::of(window);
        div()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .border_1()
            .border_color(palette.field_border)
            .bg(palette.background)
            .text_color(palette.text)
            .font_family(".SystemUIFont")
            .text_size(px(11.))
            .child(self.0)
    }
}

/// Installed Stickies: 8pt glyphs in adjacent 12pt cells within the 12pt strip.
pub fn control(
    kind: Control,
    collapsed: bool,
    active: bool,
    style: &crate::appearance::NoteStyle,
) -> Stateful<Div> {
    let fill = glyph_fill(style);
    let ink = glyph_ink(style, active, crate::platform::increase_contrast());
    let label = name(kind, collapsed);
    let glyph = match kind {
        Control::Close => Glyph::Box,
        Control::Zoom => Glyph::Zoom,
        Control::Collapse => Glyph::Collapse,
    };
    cell(label)
        .tooltip(move |_, cx| cx.new(|_| Tip(hint(kind, collapsed))).into())
        .child(
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| paint_glyph(window, bounds, glyph, fill, ink),
        )
        // Over the focus ring's border, so focus never moves the glyph.
        .absolute()
        .top(px(-1.))
        .left(px(-1.))
        .w(px(12.))
        .h(px(12.)),
    )
}

/// A strip control's 12-point cell with its focus ring and pressed state.
pub fn cell(label: &'static str) -> Stateful<Div> {
    div()
        .id(label)
        .role(Role::Button)
        .aria_label(label)
        .tab_index(0)
        .w(px(12.))
        .h(px(12.))
        .cursor_pointer()
        // The ring's border is always there, transparent until focused, so focus never
        // moves the glyph; the glyph is placed over it.
        .border_1()
        .border_color(transparent_black())
        .rounded(px(2.))
        .focus_visible(|style| style.border_color(rgb(FOCUS_RING)))
        // Feedback the instant the control is pressed; the action still waits for release.
        .active(|style| style.bg(rgba(0x00000024)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Paint over content, so the perimeter never adds an inset or intercepts input.
pub fn perimeter() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |bounds, (), window, _| {
            window.paint_quad(outline(bounds, rgb(0x000000), BorderStyle::Solid));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A Glass note's edge: a light inner line where the material catches the light and a faint
/// dark outer one, so a pale note still has a shape over a pale desktop. Increase Contrast
/// asks for one firm line instead. `radius` is the note's corner radius in points.
pub fn rim(increase_contrast: bool, radius: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let radius = px(radius);
            if increase_contrast {
                window.paint_quad(
                    outline(bounds, rgba(0x00000080), BorderStyle::Solid).corner_radii(radius),
                );
                return;
            }
            window.paint_quad(
                outline(bounds, rgba(0x0000001f), BorderStyle::Solid).corner_radii(radius),
            );
            let inner = Bounds::new(
                bounds.origin + point(px(1.), px(1.)),
                size(bounds.size.width - px(2.), bounds.size.height - px(2.)),
            );
            window.paint_quad(
                outline(inner, rgba(0xffffff73), BorderStyle::Solid).corner_radii(radius - px(1.)),
            );
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}


#[cfg(test)]
mod tests {
    use super::{Control, SEA_STRIP_VEIL, glyph_ink, hint, name, strip_colour};
    use crate::appearance::{
        Appearance, COLORS, ColorIndex, GLASS_CONTRAST_FLOOR, NoteStyle, SEA_PAPER_CEILING,
        SEA_PAPER_FLOOR, contrast, over,
    };
    use gpui::{Hsla, Rgba, rgb};

    fn styles() -> impl Iterator<Item = (NoteStyle, String)> {
        [Appearance::Classic, Appearance::Glass, Appearance::SeaGlass]
            .into_iter()
            .flat_map(|appearance| {
                ColorIndex::ALL.into_iter().map(move |color| {
                    let style = NoteStyle {
                        appearance,
                        color,
                        ..NoteStyle::default()
                    };
                    (style, format!("{appearance:?} {}", COLORS[color.get()]))
                })
            })
    }

    #[test]
    fn strip_controls_are_named_and_their_tooltips_say_the_shortcut() {
        let all = [
            (Control::Close, false),
            (Control::Zoom, false),
            (Control::Collapse, false),
            (Control::Collapse, true),
        ];
        let names = all.map(|(kind, collapsed)| name(kind, collapsed));
        assert_eq!(names, ["Close", "Zoom", "Collapse", "Expand"]);
        for (&(kind, collapsed), name) in all.iter().zip(names) {
            assert!(hint(kind, collapsed).starts_with(name));
        }
        if cfg!(target_os = "macos") {
            let keys = all.map(|(kind, collapsed)| hint(kind, collapsed));
            assert_eq!(keys, ["Close ⌘W", "Zoom ⇧⌘M", "Collapse ⌘M", "Expand ⌘M"]);
        }
    }

    #[test]
    fn every_strip_control_line_reaches_3_to_1_on_its_strip() {
        for (style, name) in styles() {
            let strip = strip_colour(&style);
            for increase in [false, true] {
                for active in [true, false] {
                    let line: Rgba = glyph_ink(&style, active, increase).into();
                    let ratio = contrast(line, strip);
                    let floor = if increase { 4.5 } else { 3. };
                    assert!(
                        ratio >= floor,
                        "{name} line (active: {active}, increase contrast: {increase}) is {ratio:.2}:1"
                    );
                    assert_eq!(line.a, 1., "{name}: an opaque line stays one tone");
                }
                let active: Rgba = glyph_ink(&style, true, increase).into();
                let inactive: Rgba = glyph_ink(&style, false, increase).into();
                assert!(
                    contrast(active, strip) >= contrast(inactive, strip),
                    "{name}: a key window's line is never fainter"
                );
            }
            // Increase Contrast is firmer than the default at rest.
            let ratio = |increase| contrast(glyph_ink(&style, true, increase).into(), strip);
            assert!(ratio(true) >= 6.9 && ratio(true) >= ratio(false), "{name}");
            // The line also reads against the glyph's own face of paper.
            if !style.sea_glass() {
                let face: Rgba = rgb(style.body());
                let line: Rgba = glyph_ink(&style, false, false).into();
                assert!(contrast(line, face) >= 3., "{name} line on its face");
            }
        }
    }

    #[test]
    fn sea_glass_lines_hold_3_to_1_on_the_veil_over_any_desktop() {
        for (style, name) in styles().filter(|(style, _)| style.sea_glass()) {
            for backdrop in [0xffffff, 0x808080, 0x1e1e1e, 0x000000] {
                let material = over(Hsla::from(rgb(0xffffff)).opacity(0.6), rgb(backdrop));
                for (alpha, increase) in [
                    (SEA_PAPER_FLOOR, false),
                    (SEA_PAPER_CEILING, false),
                    (GLASS_CONTRAST_FLOOR, true),
                    (1., false),
                ] {
                    let paper = over(Hsla::from(rgb(style.sea_body())).opacity(alpha), material);
                    let strip = over(Hsla::from(rgb(style.sea_edge())).opacity(SEA_STRIP_VEIL), paper);
                    for active in [true, false] {
                        let line: Rgba = glyph_ink(&style, active, increase).into();
                        let ratio = contrast(line, strip);
                        assert!(
                            ratio >= 3.,
                            "{name} line (active: {active}) is {ratio:.2}:1 over {backdrop:06x} at {alpha}"
                        );
                    }
                }
            }
        }
    }
}
