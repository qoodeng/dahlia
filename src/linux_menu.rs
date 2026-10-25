//! The app menus inside each note, for platforms without a global menu bar.
//!
//! macOS shows `app_menus` in its menu bar. GPUI has no menu bar on Linux or Windows, so the
//! same tree opens here from the strip's menu button or F10, drilling into one menu level at a
//! time so it fits a small note. Items dispatch their own actions from the note's previous
//! focus, as a menu bar would to the key window.
use crate::{Note, app_menus, chrome::HEIGHT, fonts};
use gpui::{prelude::*, *};

actions!(dahlia, [ToggleNoteMenu]);

const WIDTH: f32 = 212.;
const ROW: f32 = 20.;

pub struct MenuState {
    open: bool,
    /// Submenu indices from the top level to the shown level.
    path: Vec<usize>,
    selected: Option<usize>,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    scroll: ScrollHandle,
}
impl MenuState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            path: Vec::new(),
            selected: None,
            focus: cx.focus_handle(),
            restore: None,
            scroll: ScrollHandle::new(),
        }
    }
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("f10", ToggleNoteMenu, Some("DahliaNote"))]);
}

/// The items one level down `path`, with that level's title; `None` when `path` went stale.
fn level(menus: Vec<Menu>, path: &[usize]) -> Option<(Option<SharedString>, Vec<MenuItem>)> {
    let mut items: Vec<MenuItem> = menus.into_iter().map(MenuItem::Submenu).collect();
    let mut title = None;
    for &index in path {
        match items.into_iter().nth(index)? {
            MenuItem::Submenu(menu) => {
                title = Some(menu.name);
                items = menu.items;
            }
            _ => return None,
        }
    }
    Some((title, items))
}
fn selectable(item: &MenuItem) -> bool {
    match item {
        MenuItem::Action { disabled, .. } => !disabled,
        MenuItem::Submenu(menu) => !menu.disabled && !menu.items.is_empty(),
        _ => false,
    }
}
/// The next selectable index after `from` in `step` direction, wrapping; `None` if none is.
fn step(items: &[MenuItem], from: Option<usize>, forward: bool) -> Option<usize> {
    let count = items.len();
    (1..=count)
        .map(|offset| match (from, forward) {
            (None, true) => offset - 1,
            (None, false) => count - offset,
            (Some(at), true) => (at + offset) % count,
            (Some(at), false) => (at + count - offset % count) % count,
        })
        .find(|&index| selectable(&items[index]))
}
fn label(item: &MenuItem) -> Option<SharedString> {
    match item {
        MenuItem::Action { name, .. } => Some(name.clone()),
        MenuItem::Submenu(menu) => Some(menu.name.clone()),
        _ => None,
    }
}
/// The binding a Linux menu shows: the highest-precedence one without the Super key.
///
/// The Command bindings shared with macOS also register on Linux as Super, which desktops
/// reserve for their own shortcuts, so menus advertise the Ctrl equivalents instead and show
/// nothing for an action that has only a Super binding.
fn shown_binding(bindings: &[KeyBinding]) -> Option<&KeyBinding> {
    bindings.iter().rev().find(|binding| {
        binding
            .keystrokes()
            .iter()
            .all(|keystroke| !keystroke.modifiers().platform)
    })
}
/// `ctrl-shift-z` as `Ctrl+Shift+Z`.
fn shortcut_text(binding: &KeyBinding) -> Option<String> {
    let [keystroke] = binding.keystrokes() else {
        return None;
    };
    // GPUI prints the platform (Super) modifier as a glyph with no separator.
    let text = keystroke.to_string().replace('❖', "super-");
    let (modifiers, key) = match text.strip_suffix("--") {
        Some(modifiers) => (modifiers, "-"),
        None => text.rsplit_once('-').unwrap_or(("", text.as_str())),
    };
    let mut parts: Vec<String> = modifiers
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| match part {
            "ctrl" => "Ctrl".into(),
            "alt" => "Alt".into(),
            "shift" => "Shift".into(),
            "cmd" | "super" | "win" => "Super".into(),
            other => other.into(),
        })
        .collect();
    parts.push(if key.chars().count() == 1 {
        key.to_uppercase()
    } else {
        let mut chars = key.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    });
    Some(parts.join("+"))
}

impl Note {
    pub(crate) fn toggle_note_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.open {
            self.close_note_menu(window, cx);
            return;
        }
        if self.record.collapsed {
            self.collapse(window, cx);
        }
        self.menu.restore = window
            .focused(cx)
            .filter(|focused| focused != &self.menu.focus);
        self.menu.open = true;
        self.enter_level(Vec::new(), cx);
        window.focus(&self.menu.focus, cx);
        cx.notify();
    }
    fn close_note_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu.open = false;
        self.menu.path.clear();
        match self.menu.restore.take() {
            Some(restore) => window.focus(&restore, cx),
            None => window.focus(&self.shell_focus, cx),
        }
        cx.notify();
    }
    fn enter_level(&mut self, path: Vec<usize>, cx: &mut Context<Self>) {
        self.menu.selected =
            level(app_menus(cx), &path).and_then(|(_, items)| step(&items, None, true));
        self.menu.path = path;
        self.menu.scroll.scroll_to_item(0);
    }
    fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.menu.path.pop() {
            self.menu.selected = Some(index);
            self.menu.scroll.scroll_to_item(index);
            cx.notify();
        }
    }
    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, items)) = level(app_menus(cx), &self.menu.path) else {
            self.enter_level(Vec::new(), cx);
            cx.notify();
            return;
        };
        match items.into_iter().nth(index) {
            Some(item @ MenuItem::Submenu(_)) if selectable(&item) => {
                let mut path = self.menu.path.clone();
                path.push(index);
                self.enter_level(path, cx);
                cx.notify();
            }
            Some(MenuItem::Action {
                action,
                disabled: false,
                ..
            }) => {
                self.close_note_menu(window, cx);
                // Deferred by GPUI, from the restored focus, so the note's own listeners run.
                window.dispatch_action(action, cx);
            }
            _ => {}
        }
    }
    fn menu_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, items)) = level(app_menus(cx), &self.menu.path) else {
            self.enter_level(Vec::new(), cx);
            cx.notify();
            return;
        };
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if modifiers.control || modifiers.platform || modifiers.alt {
            return;
        }
        let select = |note: &mut Self, index: Option<usize>| {
            if let Some(index) = index {
                note.menu.selected = Some(index);
                note.menu.scroll.scroll_to_item(index);
            }
        };
        match key {
            "escape" | "f10" => self.close_note_menu(window, cx),
            "down" => select(self, step(&items, self.menu.selected, true)),
            "up" => select(self, step(&items, self.menu.selected, false)),
            "home" | "pageup" => select(self, step(&items, None, true)),
            "end" | "pagedown" => select(self, step(&items, None, false)),
            "left" | "backspace" => self.back(cx),
            "enter" | "space" | "right" => {
                if let Some(index) = self.menu.selected
                    && (key != "right" || matches!(items.get(index), Some(MenuItem::Submenu(_))))
                {
                    self.activate(index, window, cx);
                }
            }
            _ => {
                // Type-to-select: the next selectable item starting with the typed letter.
                let Some(typed) = event.keystroke.key_char.as_deref() else {
                    return;
                };
                let typed = typed.to_lowercase();
                let start = self.menu.selected.map_or(0, |at| at + 1);
                let found = (0..items.len())
                    .map(|offset| (start + offset) % items.len())
                    .find(|&index| {
                        selectable(&items[index])
                            && label(&items[index])
                                .is_some_and(|name| name.to_lowercase().starts_with(&typed))
                    });
                select(self, found);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// The strip's menu control: Stickies has none, so it is one more glyph of the set, a
/// paper box in the Stickies control colour with a pull-down mark, in the same cell and focus ring.
pub fn button(note: &Note, active: bool, cx: &mut Context<Note>) -> Stateful<Div> {
    let fill = crate::chrome::glyph_fill(&note.record.style);
    let ink = crate::chrome::glyph_ink(
        &note.record.style,
        active,
        crate::platform::increase_contrast(),
    );
    crate::chrome::cell("Menu")
        .on_click(cx.listener(|note, _, window, cx| note.toggle_note_menu(window, cx)))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    crate::chrome::paint_glyph(
                        window,
                        bounds,
                        crate::chrome::Glyph::Menu,
                        fill,
                        ink,
                    )
                },
            )
            .absolute()
            .top(px(-1.))
            .left(px(-1.))
            .w(px(12.))
            .h(px(12.)),
        )
}

/// The open menu level, anchored under the strip at the right; `None` while closed.
pub fn panel(note: &Note, window: &mut Window, cx: &mut Context<Note>) -> Option<AnyElement> {
    if !note.menu.open {
        return None;
    }
    let (title, items) = level(app_menus(cx), &note.menu.path)?;
    let glass = note.record.style.frosted();
    let ink: Hsla = rgb(0x000000).into();
    let faint = ink.opacity(0.35);
    let hover = ink.opacity(0.08);
    let restore = note.menu.restore.clone();
    let max_height = window.viewport_size().height - px(HEIGHT + 6.);
    let rows = items.into_iter().enumerate().map(|(index, item)| {
        let selected = note.menu.selected == Some(index);
        let enabled = selectable(&item);
        let (name, checked, trailing) = match &item {
            MenuItem::SystemMenu(_) => return div().id(index).into_any_element(),
            MenuItem::Separator => {
                return div()
                    .id(index)
                    .h(px(7.))
                    .flex()
                    .items_center()
                    .child(div().mx(px(8.)).h(px(1.)).w_full().bg(ink.opacity(0.15)))
                    .into_any_element();
            }
            MenuItem::Submenu(menu) => (menu.name.clone(), false, Some("›".to_string())),
            MenuItem::Action {
                name,
                action,
                checked,
                ..
            } => {
                let bindings = match &restore {
                    Some(focus) => window.bindings_for_action_in(&**action, focus),
                    None => window.bindings_for_action(&**action),
                };
                (name.clone(), *checked, shown_binding(&bindings).and_then(shortcut_text))
            }
        };
        div()
            .id(index)
            .role(if checked {
                Role::MenuItemCheckBox
            } else {
                Role::MenuItem
            })
            .aria_label(name.clone())
            .h(px(ROW))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(px(if glass { 4. } else { 0. }))
            .text_color(if enabled { ink } else { faint })
            .when(selected, |row| row.bg(hover))
            .when(enabled, |row| {
                row.cursor_pointer()
                    .on_mouse_move(cx.listener(move |note, _, _, cx| {
                        if note.menu.selected != Some(index) {
                            note.menu.selected = Some(index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |note, _, window, cx| {
                        note.activate(index, window, cx);
                    }))
            })
            .child(
                div()
                    .w(px(12.))
                    .flex_shrink_0()
                    .child(if checked { "✓" } else { "" }),
            )
            .child(div().flex_1().min_w_0().truncate().child(name))
            .when_some(trailing, |row, trailing| {
                row.child(div().flex_shrink_0().text_color(faint).child(trailing))
            })
            .into_any_element()
    });
    let header = title.map(|title| {
        div()
            .id("menu-back")
            .role(Role::Button)
            .aria_label("Back")
            .h(px(ROW))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .text_color(ink.opacity(0.6))
            .border_b_1()
            .border_color(ink.opacity(0.12))
            .on_click(cx.listener(|note, _, _, cx| note.back(cx)))
            .child(div().w(px(12.)).child("‹"))
            .child(title)
    });
    Some(
        div()
            .id("dahlia-note-menu")
            .track_focus(&note.menu.focus)
            .key_context("DahliaMenu")
            .role(Role::Menu)
            .aria_label("Menu")
            .absolute()
            .top(px(HEIGHT + 2.))
            .right(px(4.))
            .w(px(WIDTH))
            .max_h(max_height)
            .flex()
            .flex_col()
            .p(px(3.))
            .bg(rgb(note.record.style.paper()))
            .border_1()
            .border_color(if glass { ink.opacity(0.2) } else { ink })
            .rounded(px(if glass { 6. } else { 0. }))
            .when(glass, |panel| panel.shadow_md())
            .font_family(fonts(cx).prose)
            .text_size(px(12.))
            .occlude()
            .on_key_down(cx.listener(|note, event: &KeyDownEvent, window, cx| {
                note.menu_key(event, window, cx);
            }))
            .on_mouse_down_out(cx.listener(|note, event: &MouseDownEvent, window, cx| {
                // The strip's own button toggles; anywhere else outside dismisses.
                if event.position.y > px(HEIGHT) {
                    note.close_note_menu(window, cx);
                }
            }))
            .children(header)
            .child(
                div()
                    .id("menu-items")
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&note.menu.scroll)
                    .children(rows),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::{label, level, shortcut_text, shown_binding, step};
    use crate::{Quit as A, Save as B};
    use gpui::{KeyBinding, Menu, MenuItem};

    fn items() -> Vec<MenuItem> {
        vec![
            MenuItem::action("Alpha", A).disabled(true),
            MenuItem::separator(),
            MenuItem::action("Beta", B),
            MenuItem::submenu(Menu::new("Empty")),
            MenuItem::submenu(Menu::new("Gamma").items([MenuItem::action("Delta", A)])),
        ]
    }

    #[test]
    fn steps_skip_separators_disabled_items_and_empty_submenus_and_wrap() {
        let items = items();
        assert_eq!(step(&items, None, true), Some(2));
        assert_eq!(step(&items, Some(2), true), Some(4));
        assert_eq!(step(&items, Some(4), true), Some(2));
        assert_eq!(step(&items, None, false), Some(4));
        assert_eq!(step(&items, Some(2), false), Some(4));
        assert_eq!(step(&[MenuItem::separator()], None, true), None);
    }

    #[test]
    fn levels_follow_submenu_paths_and_reject_stale_ones() {
        let menus = || vec![Menu::new("File").items(items())];
        let (title, top) = level(menus(), &[]).unwrap();
        assert!(title.is_none());
        assert_eq!(top.len(), 1);
        let (title, gamma) = level(menus(), &[0, 4]).unwrap();
        assert_eq!(title.as_deref(), Some("Gamma"));
        assert_eq!(label(&gamma[0]).as_deref(), Some("Delta"));
        assert!(level(menus(), &[0, 2]).is_none());
        assert!(level(menus(), &[3]).is_none());
    }

    #[test]
    fn shortcuts_read_like_linux_menus() {
        let text = |keys| shortcut_text(&KeyBinding::new(keys, A, None)).unwrap();
        assert_eq!(text("ctrl-shift-z"), "Ctrl+Shift+Z");
        assert_eq!(text("ctrl--"), "Ctrl+-");
        assert_eq!(text("ctrl-alt-["), "Ctrl+Alt+[");
        assert_eq!(text("f10"), "F10");
        assert_eq!(text("ctrl-enter"), "Ctrl+Enter");
        let bindings = [
            KeyBinding::new("ctrl-m", A, None),
            KeyBinding::new("cmd-m", A, None),
        ];
        assert_eq!(
            shown_binding(&bindings).and_then(shortcut_text).as_deref(),
            Some("Ctrl+M")
        );
        assert!(shown_binding(&bindings[1..]).is_none());
        let super_ctrl = text("cmd-ctrl-1");
        assert!(
            super_ctrl.contains("Super")
                && super_ctrl.contains("Ctrl")
                && super_ctrl.ends_with("+1")
        );
    }
}
