//! The look shared by Dahlia's own windows (Settings, Find and Replace, Recovery): AppKit's
//! control metrics and colours, light or dark with the system. Notes keep their paper.
use crate::appearance::contrast;
use gpui::{prelude::*, *};

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Hsla,
    pub text: Hsla,
    pub secondary: Hsla,
    pub field: Hsla,
    /// At least 3:1 against the window, so a field can be found.
    pub field_border: Hsla,
    pub button: Hsla,
    pub button_border: Hsla,
    pub pressed: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub focus: Hsla,
    /// The outer ring on the default button, which is filled with the accent: the inner ring
    /// is `on_accent`, 4.5:1 or more against the fill, and this one is at least 3:1 against the
    /// window around it, so the pair shows whatever the accent is.
    pub focus_outer: Hsla,
    pub selected: Hsla,
    pub selected_text: Hsla,
    /// A selection in a list that does not have the keyboard.
    pub unfocused: Hsla,
}
/// `from` moved as little as possible towards `to` until `holds`, which must be true at `to`.
fn nearest(from: Rgba, to: Rgba, holds: impl Fn(Rgba) -> bool) -> Rgba {
    let mix = |amount: f32| Rgba {
        r: from.r + (to.r - from.r) * amount,
        g: from.g + (to.g - from.g) * amount,
        b: from.b + (to.b - from.b) * amount,
        a: 1.,
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..24 {
        let middle = (low + high) / 2.;
        if holds(mix(middle)) {
            high = middle;
        } else {
            low = middle;
        }
    }
    mix(high)
}

const LIGHT: [u32; 14] = [
    0xececec, 0x1d1d1f, 0x5e5e63, 0xffffff, 0x858585, 0xffffff, 0xb4b4b4, 0xdcdcdc, 0x0064e1,
    0xffffff, 0x0064e1, 0x0064e1, 0xffffff, 0xdcdcdc,
];
const DARK: [u32; 14] = [
    0x2b2b2b, 0xececec, 0xa1a1a6, 0x1e1e1e, 0x7a7a7a, 0x4a4a4a, 0x6a6a6a, 0x5c5c5c, 0x0064e1,
    0xffffff, 0x3d95ff, 0x0064e1, 0xffffff, 0x464646,
];
impl Palette {
    pub fn of(window: &Window) -> Self {
        Self::with_accent(
            matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            crate::platform::accent_color(),
        )
    }
    #[cfg(test)]
    pub fn new(dark: bool) -> Self {
        Self::with_accent(dark, None)
    }
    /// The palette with the user's accent colour. The accent fills the default button, rows
    /// and segments under white text, so it is darkened just enough for white to read on it
    /// at 4.5:1; the ring that shows keyboard focus on the window is the accent too, lightened
    /// in Dark Mode until it is 3:1 against the window. Without one, the fixed blue.
    pub fn with_accent(dark: bool, accent: Option<Rgba>) -> Self {
        let mut palette = Self::fixed(dark);
        let Some(accent) = accent else {
            return palette;
        };
        let (white, black) = (rgb(0xffffff), rgb(0x000000));
        let fill = nearest(accent, black, |fill| contrast(white, fill) >= 4.6);
        palette.accent = fill.into();
        palette.selected = fill.into();
        palette.focus = if dark {
            let window: Rgba = palette.background.into();
            nearest(accent, white, |ring| contrast(ring, window) >= 3.1).into()
        } else {
            fill.into()
        };
        palette
    }
    fn fixed(dark: bool) -> Self {
        let c = if dark { DARK } else { LIGHT }.map(|hex| Hsla::from(rgb(hex)));
        Self {
            background: c[0],
            text: c[1],
            secondary: c[2],
            field: c[3],
            field_border: c[4],
            button: c[5],
            button_border: c[6],
            pressed: c[7],
            accent: c[8],
            on_accent: c[9],
            focus: c[10],
            focus_outer: c[1],
            selected: c[11],
            selected_text: c[12],
            unfocused: c[13],
        }
    }
}

/// A window's root: AppKit's window colour and 13-point system text.
pub fn root(palette: Palette) -> Div {
    div()
        .size_full()
        .bg(palette.background)
        .text_color(palette.text)
        .font_family(".SystemUIFont")
        .text_size(px(13.))
}

/// A push button at AppKit's regular size. The default button is filled with the accent and
/// answers Return.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    palette: Palette,
    default: bool,
) -> gpui_base::Button {
    let label = label.into();
    let (fill, text, border) = if default {
        (palette.accent, palette.on_accent, palette.accent)
    } else {
        (palette.button, palette.text, palette.button_border)
    };
    gpui_base::Button::new(id)
        .accessibility_label(label.clone())
        .tab_index(0)
        .h(px(22.))
        .px(px(12.))
        .bg(fill)
        .text_color(text)
        .border_1()
        .border_color(border)
        .rounded(px(5.))
        .focus_visible(move |button| {
            if default {
                // The fill is the accent, so a ring of one colour cannot be seen against
                // both it and the window: white inside, the text colour outside.
                button
                    .border_2()
                    .border_color(palette.on_accent)
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), palette.focus_outer).spread_radius(px(2.)),
                    ])
            } else {
                button.border_2().border_color(palette.focus)
            }
        })
        .active(move |button| {
            if default {
                button.opacity(0.85)
            } else {
                button.bg(palette.pressed)
            }
        })
        .cursor_pointer()
        .child(label)
}

/// One segment of a segmented control. VoiceOver reads it as a radio button that is on while
/// `current`, since a Button's `selected` styling alone says nothing to assistive technology.
/// `select` runs when the segment is clicked or pressed with Return or Space, and when
/// assistive technology presses it: GPUI answers that by clicking the middle of the segment,
/// which misses whenever something else is hit there, so the segment answers it itself.
pub fn segment(
    button: gpui_base::Button,
    current: bool,
    select: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui_base::Button {
    let select = std::rc::Rc::new(select);
    let press = select.clone();
    button
        .selected(current)
        .role(Role::RadioButton)
        .aria_toggled(if current {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        })
        .on_click(move |_, window, cx| select(window, cx))
        .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
            press(window, cx)
        })
}

/// A text field's frame, 22 points tall, with a focus ring while `focused`.
pub fn field(palette: Palette, focused: bool) -> Div {
    div()
        .h(px(22.))
        .overflow_hidden()
        .bg(palette.field)
        .rounded(px(5.))
        .border_1()
        .border_color(palette.field_border)
        .when(focused, |field| {
            field.border_2().border_color(palette.focus)
        })
}

/// The style of a single-line editor inside [`field`].
pub fn input_style(palette: Palette, font: SharedString) -> markraft_gpui::EditorStyle {
    markraft_gpui::EditorStyle {
        padding: px(3.),
        body_size: px(13.),
        line_height_ratio: 1.2,
        font_family: font,
        background: transparent_black(),
        text: palette.text,
        marker: palette.text,
        selection: palette.accent.opacity(0.35),
        selection_inactive: palette.secondary.opacity(0.3),
        ..Default::default()
    }
}

/// Whether a key press is a bare Return or Escape, which panels answer like AppKit's.
pub fn bare(event: &KeyDownEvent, key: &str) -> bool {
    let modifiers = event.keystroke.modifiers;
    event.keystroke.key == key
        && !modifiers.control
        && !modifiers.alt
        && !modifiers.shift
        && !modifiers.platform
}

#[cfg(test)]
mod tests {
    use super::{Palette, button, segment};
    use gpui::rgb;
    use crate::appearance::contrast;
    use gpui::{Element as _, Hsla, IntoElement as _, RenderOnce as _, TestAppContext, accesskit};
    use std::sync::{Arc, Mutex};

    #[gpui::test]
    fn segments_expose_their_selected_state_as_radio_buttons(cx: &mut TestAppContext) {
        type Segments = Arc<Mutex<Vec<(Option<accesskit::Role>, accesskit::Node)>>>;
        struct Probe(Segments);
        impl gpui::Render for Probe {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                let nodes = self.0.clone();
                gpui::canvas(
                    move |_, window, cx| {
                        let mut nodes = nodes.lock().unwrap();
                        nodes.clear();
                        for (id, current) in [("on", true), ("off", false)] {
                            let mut node = accesskit::Node::new(accesskit::Role::Button);
                            let element = segment(
                                button(id, "Trash", Palette::new(false), false),
                                current,
                                |_, _| {},
                            )
                                .render(window, cx)
                                .into_element();
                            element.write_a11y_info(&mut node);
                            nodes.push((element.a11y_role(), node));
                        }
                    },
                    |_, _, _, _| {},
                )
            }
        }
        let nodes = Arc::new(Mutex::new(Vec::new()));
        let (_, cx) = cx.add_window_view({
            let nodes = nodes.clone();
            move |_, _| Probe(nodes)
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let nodes = nodes.lock().unwrap();
        assert_eq!(
            nodes.iter().map(|(role, _)| *role).collect::<Vec<_>>(),
            [Some(accesskit::Role::RadioButton); 2]
        );
        assert_eq!(
            nodes.iter().map(|(_, node)| node.toggled()).collect::<Vec<_>>(),
            [Some(accesskit::Toggled::True), Some(accesskit::Toggled::False)]
        );
        assert_eq!(nodes[0].1.label(), Some("Trash"));
        // Assistive technology presses a segment as it would a button.
        assert!(
            nodes
                .iter()
                .all(|(_, node)| node.supports_action(accesskit::Action::Click))
        );
    }

    /// The fixed blue, then the system's eight accent colours (Blue, Purple, Pink, Red,
    /// Orange, Yellow, Green, Graphite) in their light-appearance values.
    const ACCENTS: [Option<u32>; 9] = [
        None,
        Some(0x007aff),
        Some(0x953d96),
        Some(0xf74f9e),
        Some(0xff5257),
        Some(0xf7821b),
        Some(0xffc600),
        Some(0x62ba46),
        Some(0x989898),
    ];

    #[test]
    fn panel_colours_meet_contrast_in_light_and_dark_with_any_accent() {
        for dark in [false, true] {
            for accent in ACCENTS {
                let p = Palette::with_accent(dark, accent.map(rgb));
                let name = format!("dark: {dark}, accent: {accent:06x?}");
                let ratio = |a: Hsla, b: Hsla| contrast(a.into(), b.into());
                assert!(ratio(p.text, p.background) >= 7., "text, {name}");
                assert!(
                    ratio(p.secondary, p.background) >= 4.5,
                    "secondary, {name}"
                );
                assert!(ratio(p.text, p.field) >= 7., "field text, {name}");
                assert!(
                    ratio(p.field_border, p.background) >= 3.,
                    "field border, {name}"
                );
                assert!(ratio(p.focus, p.background) >= 3., "focus ring, {name}");
                assert!(
                    ratio(p.on_accent, p.accent) >= 4.5,
                    "default button, {name}"
                );
                assert!(
                    ratio(p.selected_text, p.selected) >= 4.5,
                    "selection, {name}"
                );
                assert!(
                    ratio(p.text, p.unfocused) >= 4.5,
                    "unfocused row, {name}"
                );
                assert!(ratio(p.text, p.button) >= 4.5, "button, {name}");
                // The default button is filled with the accent, so its ring is two tones:
                // white against the fill (above), and the text colour against the window.
                assert!(
                    ratio(p.focus_outer, p.background) >= 3.,
                    "outer ring on the default button, {name}"
                );
            }
        }
    }

    #[test]
    fn the_accent_colour_reaches_the_panels_and_none_leaves_the_fixed_blue() {
        let fixed = Palette::new(false);
        assert_eq!(Palette::with_accent(false, None).accent, fixed.accent);
        let pink = Palette::with_accent(false, Some(rgb(0xf74f9e)));
        assert_ne!(pink.accent, fixed.accent);
        assert_eq!(pink.selected, pink.accent);
        let hue = |color: Hsla| color.h;
        assert!((hue(pink.accent) - hue(rgb(0xf74f9e).into())).abs() < 0.03);
    }
}
