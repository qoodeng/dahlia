use gpui::{Hsla, Rgba, SharedString, px, rgb, rgba};
use markraft_gpui::EditorStyle;
use serde::{Deserialize, Serialize};

// Paper, strip and control colours are installed Stickies 10.3's named sRGB colours
// (`evidence/stickies-colors.json`, read from its asset catalog), checked by a test below.
pub const BODY: [u32; 6] = [0xfef49c, 0xadf4ff, 0xb2ffa1, 0xffc7c7, 0xb6caff, 0xeeeeee];
pub const SPINE: [u32; 6] = [0xfeea3d, 0x89f0ff, 0x83fe83, 0xffb2b2, 0x9bb6fe, 0xdadada];
/// The strip controls' line: installed Stickies' `StickiesControl<Color>Color`.
pub const CONTROL: [u32; 6] = [0xdbc503, 0x24d0e9, 0x51bb51, 0xf87f7f, 0x7d9beb, 0xa8a8a8];
/// Sea Glass pigments follow the note's colour: amber, lagoon, mint, rose, lilac, neutral.
/// Paper, edge (the strip and the glyph faces) and ink (the controls and, shaded, the text).
pub const SEA_BODY: [u32; 6] = [0xf0d9be, 0xb5d0eb, 0xc0ded0, 0xeeced6, 0xe1cde8, 0xe8ecea];
pub const SEA_EDGE: [u32; 6] = [0xfffce0, 0xd6e9ff, 0xe6fff0, 0xfff0f1, 0xfbe8f7, 0xffffff];
pub const SEA_INK: [u32; 6] = [0x865733, 0x2f647c, 0x396c5a, 0x963f59, 0x71388b, 0x4a5551];
/// Amber, lagoon and mint ink is 4.1–4.5:1 on its own paper even when opaque, and selected
/// text lower still, short of the 4.5:1 every text role must reach. Body text and markers take
/// the ink this far towards black; controls keep the palette ink.
pub const SEA_TEXT_SHADE: f32 = 0.4;
pub const COLORS: [&str; 6] = ["Yellow", "Blue", "Green", "Pink", "Purple", "Gray"];
pub const DEFAULT_TINT: f32 = 0.22;
pub const MAX_TINT: f32 = 0.5;
/// Glass paper opacity at 100% and 0% transparency. The blur under the paper is always at
/// full strength, so the slider only decides how much of the note's colour tints it. The
/// floor keeps every text role readable and the six colours distinguishable.
pub const GLASS_PAPER_FLOOR: f32 = 0.3;
pub const GLASS_PAPER_CEILING: f32 = 0.75;
/// Increase Contrast asks for a near-solid surface.
pub const GLASS_CONTRAST_FLOOR: f32 = 0.85;
/// Sea Glass paper opacity at 100% and 0% transparency. The pastel is a denser sheet than
/// Glass's at every setting, but the slider still has to visibly move it: `floor + (ceiling -
/// floor) * tint / MAX_TINT`, with the same Increase Contrast floor as Glass. 0.61 is the lowest
/// floor at which every text role stays 4.5:1 with the text shaded by `SEA_TEXT_SHADE`
/// (0.55 was asked for; selected text on lagoon over a dark desktop is 4.4:1 there).
pub const SEA_PAPER_FLOOR: f32 = 0.61;
pub const SEA_PAPER_CEILING: f32 = 0.94;

/// A validated palette index. Serialized numbers stay compatible with existing notebooks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "usize", into = "usize")]
pub struct ColorIndex(u8);
impl ColorIndex {
    pub const ALL: [Self; 6] = [Self(0), Self(1), Self(2), Self(3), Self(4), Self(5)];
    pub fn get(self) -> usize {
        usize::from(self.0)
    }
}
#[derive(Debug, thiserror::Error)]
#[error("Invalid note color: {0}")]
pub struct InvalidColor(usize);
impl TryFrom<usize> for ColorIndex {
    type Error = InvalidColor;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if value < BODY.len() {
            Ok(Self(value as u8))
        } else {
            Err(InvalidColor(value))
        }
    }
}
impl From<ColorIndex> for usize {
    fn from(value: ColorIndex) -> Self {
        value.get()
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Classic,
    #[default]
    Glass,
    SeaGlass,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NoteStyle {
    pub appearance: Appearance,
    pub color: ColorIndex,
    pub translucent: bool,
    pub reading_size: f32,
    pub checking: crate::checking::Checking,
}
impl Default for NoteStyle {
    fn default() -> Self {
        Self {
            appearance: Appearance::Glass,
            color: ColorIndex::default(),
            translucent: false,
            reading_size: 12.,
            checking: crate::checking::Checking::default(),
        }
    }
}
impl NoteStyle {
    pub fn legacy() -> Self {
        Self {
            appearance: Appearance::Classic,
            ..Self::default()
        }
    }
    pub fn valid(&self) -> bool {
        self.reading_size.is_finite() && (9. ..=48.).contains(&self.reading_size)
    }
    /// Plain Glass only; the shared frosted look is `frosted`.
    #[cfg(test)]
    pub fn glass(&self) -> bool {
        self.appearance == Appearance::Glass
    }
    pub fn sea_glass(&self) -> bool {
        self.appearance == Appearance::SeaGlass
    }
    /// Both Glass and Sea Glass sit over the native translucent material.
    pub fn frosted(&self) -> bool {
        matches!(self.appearance, Appearance::Glass | Appearance::SeaGlass)
    }
    /// The window's corner radius in points.
    pub fn corner_radius(&self) -> f32 {
        match self.appearance {
            Appearance::Classic => 0.,
            Appearance::Glass => 6.,
            Appearance::SeaGlass => 18.,
        }
    }
    /// Extra horizontal inset for the strip controls, collapsed title and text, so they clear
    /// a corner rounder than Glass's 6 points: half of the additional radius.
    pub fn edge_inset(&self) -> f32 {
        (self.corner_radius() - 6.).max(0.) / 2.
    }
    pub fn body(&self) -> u32 {
        BODY[self.color.get()]
    }
    pub fn spine(&self) -> u32 {
        SPINE[self.color.get()]
    }
    pub fn control(&self) -> u32 {
        CONTROL[self.color.get()]
    }
    pub fn sea_body(&self) -> u32 {
        SEA_BODY[self.color.get()]
    }
    pub fn sea_edge(&self) -> u32 {
        SEA_EDGE[self.color.get()]
    }
    pub fn sea_ink(&self) -> u32 {
        SEA_INK[self.color.get()]
    }
    /// The paper the note's text sits on, whatever the appearance.
    pub fn paper(&self) -> u32 {
        if self.sea_glass() {
            self.sea_body()
        } else {
            self.body()
        }
    }
    /// Body text and markers: pure black, except in Sea Glass where the shaded palette ink.
    pub fn text_ink(&self) -> u32 {
        if self.sea_glass() {
            let shade = |shift: u32| {
                let channel = ((self.sea_ink() >> shift) & 0xff) as f32;
                (channel * (1. - SEA_TEXT_SHADE)).round() as u32
            };
            shade(16) << 16 | shade(8) << 8 | shade(0)
        } else {
            0x000000
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Defaults {
    pub style: NoteStyle,
    pub width: f32,
    pub height: f32,
    pub floating: bool,
}
impl Default for Defaults {
    fn default() -> Self {
        Self {
            style: NoteStyle::default(),
            width: 360.,
            height: 350.,
            floating: false,
        }
    }
}

pub fn clamp_tint(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0., MAX_TINT)
    } else {
        DEFAULT_TINT
    }
}
pub fn transparency(tint: f32) -> f32 {
    (1. - clamp_tint(tint) / MAX_TINT) * 100.
}
pub fn tint_from_transparency(percent: f32) -> f32 {
    clamp_tint((1. - percent / 100.) * MAX_TINT)
}
/// How opaque a Glass note's paper is drawn over the blur.
pub fn glass_paper_alpha(tint: f32, increase_contrast: bool) -> f32 {
    let alpha = GLASS_PAPER_FLOOR
        + (GLASS_PAPER_CEILING - GLASS_PAPER_FLOOR) * (clamp_tint(tint) / MAX_TINT);
    if increase_contrast {
        alpha.max(GLASS_CONTRAST_FLOOR)
    } else {
        alpha
    }
}

/// How opaque a Sea Glass note's paper is drawn over the blur.
pub fn sea_paper_alpha(tint: f32, increase_contrast: bool) -> f32 {
    let alpha = SEA_PAPER_FLOOR
        + (SEA_PAPER_CEILING - SEA_PAPER_FLOOR) * (clamp_tint(tint) / MAX_TINT);
    if increase_contrast {
        alpha.max(GLASS_CONTRAST_FLOOR)
    } else {
        alpha
    }
}

/// Black ink at `alpha`: secondary text, fills and rules take the paper's own hue this way,
/// on every colour and over Glass alike.
pub fn ink(alpha: f32) -> Hsla {
    Hsla::from(rgb(0x000000)).opacity(alpha)
}
/// The selection fill when no system highlight colour applies.
const DEFAULT_SELECTION: u32 = 0x0064e1;
const SELECTION_ALPHA: f32 = 0.3;
/// The user's highlight colour, rebuilt as a translucent fill that shows exactly that colour
/// over white and keeps the paper's hue elsewhere. A highlight too close to the paper (a
/// yellow highlight on a Yellow note) falls back to blue so the selection never disappears.
pub fn selection_fill(highlight: Option<Rgba>, paper: u32) -> Hsla {
    let fallback = || Hsla::from(rgb(DEFAULT_SELECTION)).opacity(SELECTION_ALPHA);
    let Some(highlight) = highlight else {
        return fallback();
    };
    const ALPHA: f32 = 0.45;
    let channel = |c: f32| ((c - (1. - ALPHA)) / ALPHA).clamp(0., 1.);
    let fill = Rgba {
        r: channel(highlight.r),
        g: channel(highlight.g),
        b: channel(highlight.b),
        a: ALPHA,
    };
    let paper: Rgba = rgb(paper);
    if contrast(over(fill, paper), paper) < 1.25 {
        fallback()
    } else {
        fill.into()
    }
}
/// WCAG relative luminance of an opaque colour.
pub fn luminance(color: Rgba) -> f32 {
    let channel = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}
/// WCAG contrast ratio between two opaque colours.
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
/// A translucent colour composited over an opaque one.
pub fn over(fill: impl Into<Rgba>, background: Rgba) -> Rgba {
    let fill = fill.into();
    let blend = |f: f32, b: f32| f * fill.a + b * (1. - fill.a);
    Rgba {
        r: blend(fill.r, background.r),
        g: blend(fill.g, background.g),
        b: blend(fill.b, background.b),
        a: 1.,
    }
}

#[derive(Clone)]
pub struct Fonts {
    pub prose: SharedString,
    pub code: SharedString,
}
impl Fonts {
    pub fn available(names: &[String]) -> Self {
        let pick = |candidates: &[&str], fallback: &str| -> SharedString {
            candidates
                .iter()
                .find(|name| names.iter().any(|n| n == **name))
                .copied()
                .unwrap_or(fallback)
                .to_owned()
                .into()
        };
        Self {
            prose: pick(
                &[
                    "Helvetica Neue",
                    "HelveticaNeue",
                    "Arial",
                    "Liberation Sans",
                ],
                ".SystemUIFont",
            ),
            code: pick(
                &[
                    "Paper Mono",
                    "PaperMono-Regular",
                    "Menlo",
                    "Consolas",
                    "Liberation Mono",
                ],
                ".AppleSystemUIFontMonospaced",
            ),
        }
    }
}
/// Heading sizes as ems of the reading size: Title, Heading, Subheading, then three
/// smaller levels (20, 16, 14, 13, 12 and 12 points at the default 12).
pub const HEADING_SCALE: [f32; 6] = [1.67, 1.33, 1.17, 1.08, 1., 1.];
/// Code syntax colours on a code well, in Markraft's tone order: text, keyword, name,
/// string, comment. Dark enough for AA on every paper, in Classic, Glass (down to its paper
/// floor over a dark desktop) and Sea Glass.
pub const SYNTAX_TONES: [u32; 5] = [0x000000, 0x2c08bb, 0x053684, 0x0f4425, 0x393b3e];

pub fn editor_style(style: &NoteStyle, fonts: &Fonts) -> EditorStyle {
    let size = style.reading_size;
    // Every distance is an em of the reading size, rounded to whole points so edges stay
    // on device pixels: 9 pt and 48 pt are the same note, scaled.
    let em = |k: f32| px((size * k).round());
    let half = |k: f32| px((size * k * 2.).round() / 2.);
    let cap = px(style.corner_radius().max(6.));
    let text = rgb(style.text_ink());
    EditorStyle {
        background: Hsla::from(rgb(style.paper())).opacity(0.),
        // With the note's 6-point inset: Stickies' 10-point margin at the sides, and the
        // same 10 points under the last line.
        padding: px(4.),
        bottom_overlay: px(6.),
        // A zoomed note keeps a readable measure instead of 250-character lines.
        max_line_width: Some(em(36.)),
        font_family: fonts.prose.clone(),
        code_font_family: fonts.code.clone(),
        body_size: px(size),
        task_marker_size: Some(em(0.8)),
        heading_sizes: HEADING_SCALE.map(em),
        // 1.25 leading, rounded to a whole-point line so every baseline lands on the same
        // pixel phase; headings are set tighter, code a little looser.
        line_height_ratio: (size * 1.25).round() / size,
        heading_line_height_ratio: Some(1.1),
        code_line_height_ratio: Some(1.3),
        paragraph_gap: em(0.6),
        list_gap: em(0.25),
        heading_top_gaps: [1., 0.83, 0.67, 0.5, 0.5, 0.5].map(em),
        heading_bottom_gap: em(0.33),
        list_indent: em(1.5),
        // Quote text shares the list text's column.
        quote_indent: em(1.5),
        // Markraft reserves this row for a table toolbar; Dahlia draws none.
        table_toolbar_room: px(0.),
        code_size: Some(half(0.875).max(px(9.))),
        code_padding: em(0.67),
        code_inset: em(0.5),
        // Never rounder than the window's corners that contain them (Classic keeps Glass's 6).
        code_block_radius: em(0.5).min(cap),
        code_radius: em(0.3).min(cap),
        inline_code_padding: em(0.25),
        cell_padding_x: em(0.6),
        cell_padding_y: em(0.33),
        quote_bar: Some(ink(0.25)),
        quote_bar_width: em(0.2).clamp(px(2.), px(6.)),
        rule_thickness: half(0.08).max(px(1.)),
        decoration_thickness: px((size * 0.06).max(1.)),
        syntax_tones: Some(SYNTAX_TONES.map(|tone| rgb(tone).into())),
        // Body text and markers stay pure black, Stickies' ink and full strength for Glass;
        // Sea Glass takes its palette's shaded ink.
        text: text.into(),
        marker: text.into(),
        // Every other role is either a fixed dark ink or translucent black, chosen so each
        // passes WCAG AA on all six papers in Classic and over Glass (see the tests).
        muted_text: ink(0.65),
        link: rgb(0x083b96).into(),
        selection: selection_fill(crate::platform::highlight_color(), style.paper()),
        selection_inactive: ink(0.12),
        find: rgba(0xff9f0a80).into(),
        highlight: rgba(0xff9f0a66).into(),
        code_background: ink(0.05),
        inline_code_background: ink(0.06),
        inline_code_text: ink(0.8),
        table_header_background: ink(0.05),
        // Rules and grid lines are separators, not content: quiet, in the paper's hue.
        rule: ink(0.3),
        ..EditorStyle::notes()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn palette_is_installed_stickies_named_colours() {
        let evidence: serde_json::Value =
            serde_json::from_str(include_str!("../evidence/stickies-colors.json")).unwrap();
        let hex = |components: &serde_json::Value| {
            components.as_array().unwrap()[..3].iter().fold(0u32, |hex, component| {
                hex << 8 | (component.as_f64().unwrap() * 255.).round() as u32
            })
        };
        for (index, name) in super::COLORS.iter().enumerate() {
            let color = &evidence["colors"][name];
            assert_eq!(super::BODY[index], hex(&color["paper"]), "{name} paper");
            assert_eq!(super::SPINE[index], hex(&color["spine"]), "{name} strip");
            assert_eq!(super::CONTROL[index], hex(&color["control"]), "{name} control");
        }
    }
    use super::*;
    #[test]
    fn old_styles_receive_conservative_checking_defaults_and_new_styles_round_trip() {
        let old: NoteStyle = serde_json::from_str(r#"{"appearance":"classic","color":2}"#).unwrap();
        assert_eq!(old.checking, crate::checking::Checking::default());
        assert!(old.checking.spelling && old.checking.grammar);
        assert!(!old.checking.correction && !old.checking.quotes && !old.checking.dashes);
        let mut defaults = Defaults::default();
        defaults
            .style
            .checking
            .toggle(crate::checking::Setting::Quotes);
        let decoded: Defaults =
            serde_json::from_slice(&serde_json::to_vec(&defaults).unwrap()).unwrap();
        assert_eq!(decoded, defaults);
        assert_eq!(NoteStyle::legacy().checking, NoteStyle::default().checking);
        let sea = NoteStyle {
            appearance: Appearance::SeaGlass,
            color: ColorIndex::ALL[4],
            ..NoteStyle::default()
        };
        let encoded = serde_json::to_string(&sea).unwrap();
        assert!(encoded.contains("\"seaglass\""));
        assert_eq!(serde_json::from_str::<NoteStyle>(&encoded).unwrap(), sea);
        assert!(sea.frosted() && sea.sea_glass() && !sea.glass());
        assert!(NoteStyle::default().frosted() && !NoteStyle::legacy().frosted());
        assert_eq!(sea.corner_radius(), 18.);
        assert_eq!(NoteStyle::default().corner_radius(), 6.);
        assert_eq!(NoteStyle::legacy().corner_radius(), 0.);
        // Only Sea Glass's rounder corner moves the controls and text inward.
        assert_eq!(sea.edge_inset(), 6.);
        assert_eq!(NoteStyle::default().edge_inset(), 0.);
        assert_eq!(NoteStyle::legacy().edge_inset(), 0.);
    }
    #[test]
    fn sea_glass_palette_follows_the_note_colour_and_shades_only_the_text() {
        for (index, color) in ColorIndex::ALL.into_iter().enumerate() {
            let note = NoteStyle {
                appearance: Appearance::SeaGlass,
                color,
                ..NoteStyle::default()
            };
            assert_eq!(note.sea_body(), SEA_BODY[index]);
            assert_eq!(note.sea_edge(), SEA_EDGE[index]);
            assert_eq!(note.sea_ink(), SEA_INK[index]);
            assert_eq!(note.paper(), SEA_BODY[index]);
            assert_ne!(note.text_ink(), note.sea_ink());
            assert!(
                contrast(rgb(note.text_ink()), rgb(note.sea_body()))
                    > contrast(rgb(note.sea_ink()), rgb(note.sea_body())),
                "text is the palette ink taken towards black"
            );
            let plain = NoteStyle {
                color,
                ..NoteStyle::default()
            };
            assert_eq!((plain.paper(), plain.text_ink()), (BODY[index], 0x000000));
            let fonts = Fonts::available(&[]);
            let style = editor_style(&note, &fonts);
            assert_eq!(style.text, Hsla::from(rgb(note.text_ink())));
            assert_eq!(style.marker, style.text);
        }
    }
    #[test]
    fn swift_transparency_mapping_endpoints_and_fallback() {
        assert!((transparency(DEFAULT_TINT) - 56.).abs() < 0.001);
        assert_eq!(tint_from_transparency(100.), 0.);
        assert_eq!(tint_from_transparency(0.), 0.5);
        assert_eq!(clamp_tint(f32::NAN), DEFAULT_TINT);
        assert_eq!(clamp_tint(20.), 0.5);
        assert_eq!(clamp_tint(-2.), 0.);
        let fonts = Fonts::available(&["Helvetica Neue".into(), "Paper Mono".into()]);
        assert_eq!(fonts.code.as_ref(), "Paper Mono");
        let style = editor_style(&NoteStyle::default(), &fonts);
        assert_eq!(style.text.a, 1.);
        assert_eq!(style.background.a, 0.);
    }

    /// AppKit's light material keeps its own light fill over any desktop. The model assumes
    /// it lightens what is behind it by 60%, white over the darkest backdrop tested.
    const MATERIAL_FROST: f32 = 0.6;
    /// Every surface a note's text can sit on: the opaque paper (Classic, or Reduce Transparency), and the paper at
    /// `alpha` over the material over a white and a near-black (#1E1E1E) desktop.
    fn surfaces(paper: u32, alpha: f32) -> Vec<(String, Rgba)> {
        let paper_rgb: Rgba = rgb(paper);
        let mut surfaces = vec![(format!("Opaque {paper:06x}"), paper_rgb)];
        for backdrop in [0xffffff, 0x1e1e1e] {
            let material = over(
                Hsla::from(rgb(0xffffff)).opacity(MATERIAL_FROST),
                rgb(backdrop),
            );
            surfaces.push((
                format!("Frosted {paper:06x} over {backdrop:06x}"),
                over(Hsla::from(paper_rgb).opacity(alpha), material),
            ));
        }
        surfaces
    }
    fn assert_text(name: &str, ratio: f32, floor: f32, surface: &str) {
        assert!(
            ratio >= floor,
            "{name} is {ratio:.2}:1 on {surface}, below {floor}:1"
        );
    }

    /// Every text role on one note's paper at `alpha` over the material, and the fills that
    /// carry state against the opaque paper.
    fn assert_note_readable(note: &NoteStyle, alpha: f32) {
        let fonts = Fonts::available(&[]);
        let paper = note.paper();
        let style = editor_style(note, &fonts);
        for (surface, bg) in surfaces(paper, alpha) {
            let on = |fill: Hsla| over(fill, bg);
            let ink_on = |text: Hsla, fill: Rgba| contrast(over(text, fill), fill);
            assert_text("Body text", ink_on(style.text, bg), 4.5, &surface);
            assert_text("Muted text", ink_on(style.muted_text, bg), 4.5, &surface);
            assert_text("Link", ink_on(style.link, bg), 4.5, &surface);
            assert_text("Marker", ink_on(style.marker, bg), 4.5, &surface);
            let code = on(style.code_background);
            assert_text("Code text", ink_on(style.text, code), 4.5, &surface);
            for tone in style.syntax_tones.unwrap() {
                assert_text("Syntax colour", ink_on(tone, code), 4.5, &surface);
            }
            let pill = on(style.inline_code_background);
            assert_text(
                "Inline code",
                ink_on(style.inline_code_text, pill),
                4.5,
                &surface,
            );
            let header = on(style.table_header_background);
            assert_text("Table header", ink_on(style.text, header), 4.5, &surface);
            for (name, fill) in [
                ("Selected text", style.selection),
                ("Unfocused selection", style.selection_inactive),
                ("Find hit", style.find),
                ("Highlight", style.highlight),
            ] {
                assert_text(name, ink_on(style.text, on(fill)), 4.5, &surface);
            }
        }
        // Fills that carry state must stand apart from the paper, not just carry text.
        let paper_rgb: Rgba = rgb(paper);
        for (name, fill, floor) in [
            ("Selection", style.selection, 1.3),
            ("Unfocused selection", style.selection_inactive, 1.2),
            ("Find hit", style.find, 1.15),
            ("Code well", style.code_background, 1.08),
        ] {
            let ratio = contrast(over(fill, paper_rgb), paper_rgb);
            assert!(
                ratio >= floor,
                "{name} separates {ratio:.2}:1 from {paper:06x}"
            );
        }
        // A separator is visible on every paper without being the loudest mark.
        let rule = contrast(over(style.rule, paper_rgb), paper_rgb);
        assert!(
            (1.7..3.).contains(&rule),
            "Rule is {rule:.2}:1 on {paper:06x}"
        );
    }

    #[test]
    fn every_text_role_is_readable_on_all_six_papers_in_classic_and_glass() {
        let defaults = glass_paper_alpha(DEFAULT_TINT, false);
        for color in ColorIndex::ALL {
            let note = NoteStyle {
                color,
                ..NoteStyle::default()
            };
            assert_note_readable(&note, defaults);
            // The slider's clearest setting lets the most of the desktop through.
            assert_note_readable(&note, GLASS_PAPER_FLOOR);
        }
    }

    #[test]
    fn every_text_role_is_readable_on_all_six_sea_glass_papers_at_every_transparency() {
        for color in ColorIndex::ALL {
            let note = NoteStyle {
                appearance: Appearance::SeaGlass,
                color,
                ..NoteStyle::default()
            };
            assert_ne!(note.paper(), note.body());
            for step in 0..=10 {
                let tint = MAX_TINT * step as f32 / 10.;
                assert_note_readable(&note, sea_paper_alpha(tint, false));
                assert_note_readable(&note, sea_paper_alpha(tint, true));
            }
            // Reduce Transparency paints the paper opaque.
            assert_note_readable(&note, 1.);
        }
    }

    #[test]
    fn sea_glass_paper_follows_the_slider_between_its_floor_and_ceiling() {
        assert_eq!(sea_paper_alpha(0., false), SEA_PAPER_FLOOR);
        assert_eq!(sea_paper_alpha(MAX_TINT, false), SEA_PAPER_CEILING);
        assert!(sea_paper_alpha(0., true) >= GLASS_CONTRAST_FLOOR);
        assert!(sea_paper_alpha(DEFAULT_TINT, false) > SEA_PAPER_FLOOR);
    }

    #[test]
    fn layout_scales_with_the_reading_size_and_headings_stay_distinct() {
        let fonts = Fonts::available(&[]);
        let at = |size: f32| {
            editor_style(
                &NoteStyle {
                    reading_size: size,
                    ..NoteStyle::default()
                },
                &fonts,
            )
        };
        let twelve = at(12.);
        assert_eq!(
            twelve.heading_sizes,
            [20., 16., 14., 13., 12., 12.].map(px),
            "the Format menu's Title, Heading and Subheading scale"
        );
        assert_eq!(twelve.body_size * twelve.line_height_ratio, px(15.));
        for size in 9..=48 {
            let style = at(size as f32);
            let body = style.body_size;
            for pair in style.heading_sizes.windows(2).take(4) {
                assert!(
                    pair[0] > pair[1],
                    "heading levels must step down at {size} pt"
                );
            }
            assert!(style.heading_sizes[4] >= body);
            for gap in style.heading_top_gaps {
                assert!(
                    gap > style.heading_bottom_gap,
                    "a heading binds to its text"
                );
            }
            // Gaps and indents keep their proportion to the text at every size.
            for (value, em) in [
                (style.paragraph_gap, 0.6),
                (style.list_indent, 1.5),
                (style.code_padding, 0.67),
                (style.cell_padding_x, 0.6),
            ] {
                assert!(
                    (value - body * em).abs() <= px(0.5),
                    "{value:?} at {size} pt"
                );
            }
            // Task boxes, bullets and numbers share one column inside the list indent.
            assert!(style.task_marker_size.unwrap() * 1.5 <= style.list_indent);
            assert!(style.code_size.unwrap() >= px(9.));
            assert!(style.code_block_radius <= px(6.) && style.code_radius <= px(6.));
            let sea = editor_style(
                &NoteStyle {
                    appearance: Appearance::SeaGlass,
                    reading_size: size as f32,
                    ..NoteStyle::default()
                },
                &fonts,
            );
            assert!(sea.code_block_radius <= px(18.) && sea.code_radius <= px(18.));
            // Whole-point lines keep every baseline on the same pixel phase.
            let line = body * style.line_height_ratio;
            assert!((line - line.round()).abs() < px(0.001));
        }
    }

    #[test]
    fn glass_paper_has_a_floor_and_body_text_stays_readable_at_full_transparency() {
        assert_eq!(glass_paper_alpha(0., false), GLASS_PAPER_FLOOR);
        assert_eq!(glass_paper_alpha(MAX_TINT, false), GLASS_PAPER_CEILING);
        assert!(glass_paper_alpha(0., true) >= GLASS_CONTRAST_FLOOR);
        let style = editor_style(&NoteStyle::default(), &Fonts::available(&[]));
        for paper in BODY {
            for (surface, bg) in surfaces(paper, GLASS_PAPER_FLOOR) {
                for (name, role) in [
                    ("Body text", style.text),
                    ("Muted text", style.muted_text),
                    ("Link", style.link),
                ] {
                    assert_text(name, contrast(over(role, bg), bg), 4.5, &surface);
                }
            }
        }
    }

    #[test]
    fn selection_uses_the_highlight_colour_unless_it_vanishes_on_the_paper() {
        let blue_highlight = Rgba {
            r: 0.702,
            g: 0.843,
            b: 1.,
            a: 1.,
        };
        let fill: Rgba = selection_fill(Some(blue_highlight), BODY[3]).into();
        // Over white the translucent fill reproduces the opaque highlight.
        let white = over(fill, rgb(0xffffff));
        assert!((white.r - blue_highlight.r).abs() < 0.01);
        assert!((white.g - blue_highlight.g).abs() < 0.01);
        let yellow_highlight = Rgba {
            r: 1.,
            g: 0.96,
            b: 0.62,
            a: 1.,
        };
        assert_eq!(
            selection_fill(Some(yellow_highlight), BODY[0]),
            selection_fill(None, BODY[0])
        );
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;
    #[test]
    fn color_index_preserves_numeric_storage_and_rejects_invalid_values() {
        for (index, color) in ColorIndex::ALL.into_iter().enumerate() {
            assert_eq!(color.get(), index);
            assert_eq!(serde_json::to_string(&color).unwrap(), index.to_string());
            assert_eq!(
                serde_json::from_str::<ColorIndex>(&index.to_string()).unwrap(),
                color
            );
            let style: NoteStyle = serde_json::from_str(&format!("{{\"color\":{index}}}")).unwrap();
            assert_eq!(style.body(), BODY[index]);
            assert_eq!(style.spine(), SPINE[index]);
        }
        for invalid in [6, 256, usize::MAX] {
            assert!(ColorIndex::try_from(invalid).is_err());
            assert!(serde_json::from_str::<ColorIndex>(&invalid.to_string()).is_err());
        }
        assert!(serde_json::from_str::<NoteStyle>(r#"{"color":-1}"#).is_err());
    }
}
