//! The one channel for messages the user must see.
//!
//! A failure appears in the active note window, where the note offers its way out; with no
//! note window open it becomes an app alert. Nothing user-facing goes only to stderr.

use crate::{AppState, Note, NoteError};
use gpui::*;

/// Show a failure in the active note window, else as an app alert.
pub fn notify(cx: &mut App, message: impl std::fmt::Display) {
    let message = message.to_string();
    log::warn!("{message}");
    if let Some(window) = target(cx) {
        let shown = window.update(cx, |note, _, cx| {
            note.error = Some(NoteError::other(message.clone()));
            cx.notify();
        });
        if shown.is_ok() {
            return;
        }
    }
    drop(app_alert(&message));
}

/// Show an informational result, such as a completed export, the same way.
pub fn inform(cx: &mut App, title: &str, detail: Option<&str>) {
    if let Some(window) = target(cx) {
        let shown = window.update(cx, |_, window, cx| {
            drop(window.prompt(PromptLevel::Info, title, detail, &["OK"], cx));
        });
        if shown.is_ok() {
            return;
        }
    }
    let message = match detail {
        Some(detail) => format!("{title}\n\n{detail}"),
        None => title.to_owned(),
    };
    drop(app_alert(&message));
}

/// The active note window, only while it is still open.
fn target(cx: &App) -> Option<WindowHandle<Note>> {
    let state = cx.global::<AppState>();
    state
        .active
        .as_ref()
        .and_then(|id| state.windows.get(id))
        .copied()
}

/// An application-modal alert that needs no window. Resolves when it is dismissed.
pub fn app_alert(message: &str) -> futures::channel::oneshot::Receiver<()> {
    #[cfg(test)]
    {
        ALERTS.with(|alerts| alerts.borrow_mut().push(message.to_owned()));
        let (sender, receiver) = futures::channel::oneshot::channel();
        let _ = sender.send(());
        receiver
    }
    #[cfg(not(test))]
    crate::platform::alert(message)
}

#[cfg(test)]
thread_local! {
    static ALERTS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
#[cfg(test)]
pub fn take_alerts() -> Vec<String> {
    ALERTS.with(|alerts| std::mem::take(&mut *alerts.borrow_mut()))
}
