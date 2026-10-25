use crate::{AppState, appearance, glass_tint, panel, set_glass_tint};
use gpui::{prelude::*, *};
use gpui_base::slider::{
    Slider, SliderIndicator, SliderState, SliderThumb, SliderTrack, SliderValue,
};

/// The window's title, which the app menu's “Settings…” and Appearance's “Transparency
/// Settings…” both open.
pub(crate) const TITLE: &str = "Dahlia Settings";
const LABEL: &str = "Transparency";
const CAPTION: &str = "Applies to Glass and Sea Glass notes.";

pub struct GlassControl {
    focus: FocusHandle,
    slider: Entity<SliderState>,
    _changes: Subscription,
    _appearance: Subscription,
}
impl GlassControl {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let initial = appearance::transparency(glass_tint(cx));
        let slider = cx.new(|_| SliderState::new().default_value(initial));
        let changes = cx.observe(&slider, |_, slider, cx| {
            if let SliderValue::Single(percent) = slider.read(cx).value() {
                let tint = appearance::tint_from_transparency(percent);
                cx.defer(move |cx| set_glass_tint(tint, cx));
                cx.notify();
            }
        });
        Self {
            focus,
            slider,
            _changes: changes,
            _appearance: cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        }
    }
    pub fn sync(&mut self, tint: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.slider.update(cx, |slider, cx| {
            slider.set_value(appearance::transparency(tint), window, cx)
        });
        cx.notify();
    }
}
impl Render for GlassControl {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = panel::Palette::of(window);
        let percent = appearance::transparency(glass_tint(cx));
        let tabular = FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]));
        panel::root(palette)
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(6.))
            // A settings window has nothing to confirm: Return, Escape and Command-W close it.
            .capture_key_down(|event: &KeyDownEvent, window, cx| {
                if panel::bare(event, "escape") || panel::bare(event, "enter") {
                    window.remove_window();
                    cx.stop_propagation();
                }
            })
            .on_action(|_: &crate::CloseNote, window, _| window.remove_window())
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(LABEL)
                    .child(
                        div()
                            .font_features(tabular)
                            .text_color(palette.secondary)
                            .child(format!("{}%", percent.round())),
                    ),
            )
            .child(
                div()
                    .id("glass-transparency-control")
                    .track_focus(&self.focus)
                    .tab_index(0)
                    .role(Role::Slider)
                    .aria_label("Transparency for all Glass and Sea Glass notes")
                    .aria_numeric_value(percent.round() as f64)
                    .aria_min_numeric_value(0.)
                    .aria_max_numeric_value(100.)
                    .rounded(px(4.))
                    .border_2()
                    .border_color(transparent_black())
                    .focus_visible(move |style| style.border_color(palette.focus))
                    .on_key_down(cx.listener(|control, event: &KeyDownEvent, window, cx| {
                        let SliderValue::Single(value) = control.slider.read(cx).value() else {
                            return;
                        };
                        let next = match event.keystroke.key.as_str() {
                            "left" | "down" => (value - 1.).max(0.),
                            "right" | "up" => (value + 1.).min(100.),
                            "home" => 0.,
                            "end" => 100.,
                            _ => return,
                        };
                        control
                            .slider
                            .update(cx, |slider, cx| slider.set_value(next, window, cx));
                        cx.stop_propagation();
                    }))
                    .child(
                        Slider::new(&self.slider).horizontal().w_full().child(
                            SliderTrack::new(&self.slider)
                                .w_full()
                                .h(px(20.))
                                .relative()
                                .child(
                                    SliderIndicator::new(&self.slider)
                                        .w_full()
                                        .h(px(4.))
                                        .absolute()
                                        .top(px(8.))
                                        .rounded_full()
                                        .bg(palette.secondary.opacity(0.35)),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .top(px(8.))
                                        .h(px(4.))
                                        .w(relative(percent / 100.))
                                        .rounded_full()
                                        .bg(palette.accent),
                                )
                                .child(
                                    SliderThumb::new(&self.slider)
                                        .absolute()
                                        .top(px(0.))
                                        .left(relative(percent / 100.))
                                        .ml(px(-10.))
                                        .size(px(20.))
                                        .rounded_full()
                                        .bg(rgb(0xffffff))
                                        .border_1()
                                        .border_color(rgba(0x00000033))
                                        .shadow(vec![BoxShadow {
                                            color: hsla(0., 0., 0., 0.25),
                                            offset: point(px(0.), px(1.)),
                                            blur_radius: px(2.),
                                            spread_radius: px(0.),
                                            inset: false,
                                        }]),
                                ),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(palette.secondary)
                    .child("Solid")
                    .child("Clear"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(11.))
                    .text_color(palette.secondary)
                    .child(CAPTION),
            )
    }
}
pub fn show(cx: &mut App) {
    if let Some(handle) = cx.global::<AppState>().transparency_window {
        if handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        cx.global_mut::<AppState>().transparency_window = None;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(300.), px(136.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(TITLE.into()),
            ..Default::default()
        }),
        is_resizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        cx.new(|cx| GlassControl::new(window, cx))
    }) {
        Ok(handle) => {
            cx.global_mut::<AppState>().transparency_window = Some(handle);
            crate::app::sync_panel_levels(cx);
        }
        Err(error) => {
            log::warn!("Cannot open the settings window: {error}");
            crate::notify::notify(cx, "Couldn’t open Dahlia Settings.");
        }
    }
}

#[cfg(test)]
mod tests;
