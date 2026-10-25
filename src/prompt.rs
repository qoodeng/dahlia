//! Alerts drawn inside the window, for platforms without native alert sheets.
//!
//! GPUI's fallback prompt is a fixed 288-point card whose explanation line never wraps, so
//! in a small note the detail ran off the card. This one fits the window, wraps every line,
//! scrolls when the window is shorter than the card, and answers Escape with Cancel and
//! Return with the highlighted button. Return never starts on a destructive button.
use crate::{chrome::FOCUS_RING, fonts};
use gpui::{prelude::*, *};

/// The card's preferred width; narrower windows get a card that fits them.
const WIDTH: f32 = 288.;
const MARGIN: f32 = 8.;

pub fn install(cx: &mut App) {
    cx.set_prompt_builder(build);
}

fn build(
    _level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    buttons: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let buttons = buttons.to_vec();
    let selected = initial(&buttons);
    let view = cx.new(|cx| WindowPrompt {
        message: message.into(),
        detail: detail.map(|detail| detail.to_owned().into()),
        buttons,
        selected,
        focus: cx.focus_handle(),
    });
    handle.with_view(view, window, cx)
}

/// Labels that Return must never answer by default.
fn destructive(label: &str) -> bool {
    matches!(label, "Delete" | "Delete Note" | "Move to Trash")
}
/// The button Return answers first: the first that is neither Cancel nor destructive.
fn initial(buttons: &[PromptButton]) -> Option<usize> {
    buttons
        .iter()
        .position(|button| !button.is_cancel() && !destructive(button.label()))
}
/// What Escape answers: Cancel if there is one, otherwise the only button.
fn cancel(buttons: &[PromptButton]) -> Option<usize> {
    buttons
        .iter()
        .position(PromptButton::is_cancel)
        .or((buttons.len() == 1).then_some(0))
}
fn step(count: usize, from: Option<usize>, forward: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (from, forward) {
        (None, true) => 0,
        (None, false) => count - 1,
        (Some(at), true) => (at + 1) % count,
        (Some(at), false) => (at + count - 1) % count,
    })
}

struct WindowPrompt {
    message: SharedString,
    detail: Option<SharedString>,
    buttons: Vec<PromptButton>,
    selected: Option<usize>,
    focus: FocusHandle,
}
impl EventEmitter<PromptResponse> for WindowPrompt {}
impl Focusable for WindowPrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl WindowPrompt {
    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt {
            return;
        }
        let count = self.buttons.len();
        match event.keystroke.key.as_str() {
            "escape" => {
                if let Some(index) = cancel(&self.buttons) {
                    cx.emit(PromptResponse(index));
                }
            }
            "enter" | "space" => {
                if let Some(index) = self.selected {
                    cx.emit(PromptResponse(index));
                }
            }
            "tab" => self.selected = step(count, self.selected, !modifiers.shift),
            "down" | "right" => self.selected = step(count, self.selected, true),
            "up" | "left" => self.selected = step(count, self.selected, false),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}
impl Render for WindowPrompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let width = (viewport.width - px(2. * MARGIN)).min(px(WIDTH));
        let ink: Hsla = rgb(0x1f1f1f).into();
        let buttons = self.buttons.iter().enumerate().map(|(index, button)| {
            let label = button.label().clone();
            let selected = self.selected == Some(index);
            div()
                .id(index)
                .role(Role::Button)
                .aria_label(label.clone())
                .w_full()
                .min_h(px(22.))
                .px(px(6.))
                .py(px(2.))
                .flex()
                .items_center()
                .justify_center()
                .text_center()
                .border_1()
                .rounded(px(4.))
                .border_color(if selected {
                    rgb(FOCUS_RING).into()
                } else {
                    ink.opacity(0.25)
                })
                .when(selected, |row| row.bg(ink.opacity(0.06)))
                .when(destructive(&label), |row| row.text_color(rgb(0xb3261e)))
                .cursor_pointer()
                .on_mouse_move(cx.listener(move |prompt, _, _, cx| {
                    if prompt.selected != Some(index) {
                        prompt.selected = Some(index);
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(PromptResponse(index));
                    cx.stop_propagation();
                }))
                .child(label)
        });
        let card = div()
            .id("dahlia-prompt")
            .role(Role::AlertDialog)
            .aria_label(self.message.clone())
            .track_focus(&self.focus)
            .key_context("DahliaPrompt")
            .on_key_down(cx.listener(|prompt, event: &KeyDownEvent, _, cx| prompt.key(event, cx)))
            .w(width)
            .max_h(viewport.height - px(2. * MARGIN))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(12.))
            .bg(rgb(0xffffff))
            .text_color(ink)
            .rounded(px(8.))
            .shadow_lg()
            .font_family(fonts(cx).prose)
            .cursor_default()
            .child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.message.clone()),
            )
            .children(self.detail.clone().map(|detail| {
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(12.))
                    .text_color(ink.opacity(0.75))
                    .child(detail)
            }))
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .children(buttons),
            );
        div()
            .size_full()
            .absolute()
            .top_0()
            .left_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x0000004d))
            .occlude()
            .child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::{PromptButton, cancel, initial, step};

    fn buttons(labels: &[&str]) -> Vec<PromptButton> {
        labels.iter().map(|&label| label.into()).collect()
    }

    #[test]
    fn return_never_starts_on_cancel_or_delete() {
        let close = buttons(&["Archive", "Cancel", "Export and Archive…", "Delete"]);
        assert_eq!(initial(&close), Some(0));
        assert_eq!(initial(&buttons(&["Cancel", "Delete"])), None);
        assert_eq!(initial(&buttons(&["Delete", "Keep"])), Some(1));
    }

    #[test]
    fn escape_answers_cancel_or_a_lone_button() {
        assert_eq!(cancel(&buttons(&["Archive", "Cancel"])), Some(1));
        assert_eq!(cancel(&buttons(&["OK"])), Some(0));
        assert_eq!(cancel(&buttons(&["Archive", "Delete"])), None);
    }

    #[test]
    fn arrow_keys_wrap() {
        assert_eq!(step(4, None, true), Some(0));
        assert_eq!(step(4, None, false), Some(3));
        assert_eq!(step(4, Some(3), true), Some(0));
        assert_eq!(step(4, Some(0), false), Some(3));
        assert_eq!(step(0, None, true), None);
    }
}
