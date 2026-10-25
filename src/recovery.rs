use crate::{
    AppState,
    notebook::{NoteRecord, NoteState, Notebook, RecoveryEntry, RecoveryKind},
    panel::{self, Palette},
};
use gpui::{prelude::*, *};
use gpui_base::{Button, SelectableText, TextSelectionLayer};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Archived,
    Snapshots,
    Trash,
}
#[derive(Clone, Copy)]
enum Command {
    Mode(Mode),
    Restore,
    ExportNotes,
    ExportSnapshot,
    Reveal,
    Forget,
}

/// How much of a source the preview shows; restoring and exporting always use all of it.
const PREVIEW_LIMIT: usize = 12_000;

pub struct Recovery {
    focus: FocusHandle,
    entry_focus: FocusHandle,
    note_focus: FocusHandle,
    note_cursor: Option<usize>,
    entry_scroll: UniformListScrollHandle,
    note_scroll: UniformListScrollHandle,
    mode: Mode,
    entries: Vec<RecoveryEntry>,
    entry: Option<usize>,
    records: Vec<NoteRecord>,
    selected: BTreeSet<String>,
    preview: String,
    /// Whether `preview` is only the start of the source.
    preview_truncated: bool,
    status: String,
    /// The status VoiceOver was last told about.
    announced: crate::platform::Announced,
    loading: bool,
    busy: bool,
    revision: u64,
    load_task: Option<Task<()>>,
    preview_task: Option<Task<()>>,
    action_task: Option<Task<()>>,
    /// The note list the entries were built from, so returning to the window refreshes
    /// them only when something changed.
    seen: Vec<(String, NoteState, u64, String)>,
    palette: Palette,
    entries_focused: bool,
    notes_focused: bool,
    _activation: Subscription,
    _appearance: Subscription,
}
impl Recovery {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let notebook = cx.global::<AppState>().notebook.borrow();
        let status = notebook.recovery_notice().unwrap_or_default().to_owned();
        let mode = if status.is_empty() {
            Mode::Archived
        } else {
            Mode::Snapshots
        };
        drop(notebook);
        let mut recovery = Self {
            focus,
            entry_focus: cx.focus_handle(),
            note_focus: cx.focus_handle(),
            note_cursor: None,
            entry_scroll: UniformListScrollHandle::default(),
            note_scroll: UniformListScrollHandle::default(),
            mode,
            entries: vec![],
            entry: None,
            records: vec![],
            selected: BTreeSet::new(),
            preview: String::new(),
            preview_truncated: false,
            status,
            announced: crate::platform::Announced::default(),
            loading: false,
            busy: false,
            revision: 0,
            load_task: None,
            preview_task: None,
            action_task: None,
            seen: Vec::new(),
            palette: Palette::of(window),
            entries_focused: false,
            notes_focused: false,
            // The lists follow the notebook without a Refresh button.
            _activation: cx.observe_window_activation(window, |recovery, window, cx| {
                if window.is_window_active() && recovery.seen != Self::seen(cx) {
                    recovery.refresh(cx);
                }
            }),
            _appearance: cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        };
        recovery.refresh(cx);
        recovery
    }
    fn seen(cx: &App) -> Vec<(String, NoteState, u64, String)> {
        let notebook = cx.global::<AppState>().notebook.borrow();
        notebook
            .notes()
            .iter()
            .map(|note| {
                (
                    note.id.clone(),
                    note.state,
                    note.state_changed_at,
                    note.title.clone(),
                )
            })
            .collect()
    }
    fn root(cx: &App) -> PathBuf {
        cx.global::<AppState>().notebook.borrow().root().to_owned()
    }
    fn clear(&mut self) {
        self.revision += 1;
        self.load_task.take();
        self.preview_task.take();
        self.entry = None;
        self.records.clear();
        self.note_cursor = None;
        self.selected.clear();
        self.preview.clear();
        self.preview_truncated = false;
    }
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.clear();
        self.seen = Self::seen(cx);
        let root = Self::root(cx);
        let notes = cx.global::<AppState>().notebook.borrow().notes().to_vec();
        if self.mode != Mode::Snapshots {
            let state = if self.mode == Mode::Trash {
                NoteState::Trash
            } else {
                NoteState::Archived
            };
            self.entries = notes
                .into_iter()
                .filter(|note| note.state == state)
                .map(|note| {
                    let path = if note.id == "primary" {
                        root.join("note.md")
                    } else {
                        root.join("notes").join(&note.id).join("note.md")
                    };
                    RecoveryEntry {
                        title: note.title.clone(),
                        path,
                        modified_at: note.state_changed_at,
                        kind: RecoveryKind::Source,
                        note: Some(note),
                    }
                })
                .collect();
            self.loading = false;
            cx.notify();
            return;
        }
        self.entries.clear();
        self.loading = true;
        let revision = self.revision;
        self.load_task = Some(cx.spawn(async move |recovery, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { Notebook::inventory(&root, &notes) })
                .await;
            let _ = recovery.update(cx, |recovery, cx| {
                if recovery.revision != revision {
                    return;
                }
                recovery.loading = false;
                match result {
                    Ok(entries) => recovery.entries = entries,
                    Err(error) => recovery.status = crate::copy::plain(&error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn choose_entry(&mut self, index: usize, extend: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(entry) = self.entries.get(index).cloned() else {
            return;
        };
        if self.mode != Mode::Snapshots {
            self.revision += 1;
            self.load_task.take();
            self.preview_task.take();
            self.entry = Some(index);
            self.records = self
                .entries
                .iter()
                .filter_map(|entry| entry.note.clone())
                .collect();
            self.status.clear();
            self.loading = false;
            if let Some(note) = &entry.note {
                self.select_note(note.id.clone(), extend, cx);
            }
            self.entry_scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
            return;
        }
        self.clear();
        self.entry = Some(index);
        self.status = if entry.kind == RecoveryKind::Metadata {
            "Restoring from a note list brings back each note with its current text.".into()
        } else {
            String::new()
        };
        self.entry_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        self.loading = true;
        let root = Self::root(cx);
        let revision = self.revision;
        self.load_task = Some(cx.spawn(async move |recovery, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { Notebook::recovery_records(&root, &entry) })
                .await;
            let _ = recovery.update(cx, |recovery, cx| {
                if recovery.revision != revision {
                    return;
                }
                recovery.loading = false;
                match result {
                    Ok(records) => {
                        recovery.selected = records.iter().map(|note| note.id.clone()).collect();
                        recovery.records = records;
                        recovery.show_preview(cx);
                    }
                    Err(error) => recovery.status = crate::copy::plain(&error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn show_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_task.take();
        self.preview.clear();
        self.preview_truncated = false;
        let record = self
            .note_cursor
            .and_then(|index| self.records.get(index))
            .filter(|record| self.selected.contains(&record.id))
            .or_else(|| {
                self.records
                    .iter()
                    .find(|record| self.selected.contains(&record.id))
            })
            .cloned();
        let Some(record) = record else {
            cx.notify();
            return;
        };
        let entry = if self.mode == Mode::Snapshots {
            self.entry.and_then(|index| self.entries.get(index))
        } else {
            self.entries
                .iter()
                .find(|entry| entry.note.as_ref().is_some_and(|note| note.id == record.id))
        }
        .cloned();
        let Some(entry) = entry else {
            return;
        };
        let root = Self::root(cx);
        let revision = self.revision;
        let selection = self.selected.clone();
        self.preview_task = Some(cx.spawn(async move |recovery, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { Notebook::preview_source(&root, &entry, &record.id) })
                .await;
            let _ = recovery.update(cx, |recovery, cx| {
                if recovery.revision != revision || recovery.selected != selection {
                    return;
                }
                match result {
                    Ok(text) => {
                        recovery.preview_truncated = text.chars().count() > PREVIEW_LIMIT;
                        recovery.preview = text.chars().take(PREVIEW_LIMIT).collect();
                    }
                    Err(error) => recovery.status = crate::copy::plain(&error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn select_note(&mut self, id: String, extend: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.note_cursor = self.records.iter().position(|record| record.id == id);
        if !extend {
            self.selected.clear();
        }
        if !self.selected.insert(id.clone()) {
            self.selected.remove(&id);
        }
        self.show_preview(cx);
    }
    fn selected_sources(&self) -> Vec<(RecoveryEntry, String)> {
        self.selected
            .iter()
            .filter_map(|id| {
                let entry = if self.mode == Mode::Snapshots {
                    self.entry.and_then(|index| self.entries.get(index))
                } else {
                    self.entries
                        .iter()
                        .find(|entry| entry.note.as_ref().is_some_and(|note| note.id == *id))
                };
                entry.cloned().map(|entry| (entry, id.clone()))
            })
            .collect()
    }
    fn command(&mut self, command: Command, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match command {
            Command::Mode(mode) => {
                self.mode = mode;
                self.refresh(cx);
            }
            Command::Reveal => {
                let path = self
                    .entry
                    .and_then(|index| self.entries.get(index))
                    .map(|entry| entry.path.clone())
                    .unwrap_or_else(|| Self::root(cx));
                cx.reveal_path(&path);
            }
            Command::Restore => self.restore(cx),
            Command::Forget => self.forget(cx),
            Command::ExportNotes => self.export(false, cx),
            Command::ExportSnapshot => self.export(true, cx),
        }
    }
    /// What the list on screen is called, as in "Removed 2 notes from Trash".
    fn list_name(&self) -> &'static str {
        match self.mode {
            Mode::Trash => "Trash",
            _ => "Archived Notes",
        }
    }
    /// Remove the selected notes from the list while keeping their files, which History
    /// still offers. This also clears a note whose file is gone.
    fn forget(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.mode == Mode::Snapshots || self.selected.is_empty() {
            return;
        }
        let ids: Vec<_> = self.selected.iter().cloned().collect();
        let requested = ids.len();
        self.busy = true;
        cx.spawn(async move |recovery, cx| {
            let library = cx.update(|cx| cx.global::<AppState>().notebook.clone());
            let mut forgotten = Vec::new();
            let mut failure = None;
            for id in ids {
                let pending = library.borrow_mut().request_forget(&id);
                let result = match pending {
                    Ok(pending) => {
                        let completed = pending.wait().await;
                        library.borrow_mut().complete(completed)
                    }
                    Err(error) => Err(error),
                };
                match result {
                    Ok(()) => forgotten.push(id),
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            let _ = recovery.update(cx, |recovery, cx| {
                recovery.busy = false;
                let state = cx.global_mut::<AppState>();
                state.undo_deletes.retain(|id| !forgotten.contains(id));
                state.redo_deletes.retain(|id| !forgotten.contains(id));
                recovery.status = match failure {
                    None => {
                        let files = if forgotten.len() == 1 {
                            "Its file is"
                        } else {
                            "Their files are"
                        };
                        format!(
                            "Removed {} from {}. {files} still in History.",
                            crate::copy::count(forgotten.len(), "note", "notes"),
                            recovery.list_name()
                        )
                    }
                    Some(error) => crate::copy::failed(
                        if requested == 1 {
                            "Couldn’t forget this note."
                        } else {
                            "Couldn’t forget these notes."
                        },
                        &error,
                    ),
                };
                crate::update_menus(cx);
                recovery.refresh(cx);
            });
        })
        .detach();
        cx.notify();
    }
    fn restore(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.selected.is_empty() {
            return;
        }
        if self.mode != Mode::Snapshots {
            let expected = if self.mode == Mode::Trash {
                NoteState::Trash
            } else {
                NoteState::Archived
            };
            crate::operations::restore_many(
                self.selected.iter().cloned().collect(),
                expected,
                false,
                cx,
            );
            self.refresh(cx);
            return;
        }
        let Some(entry) = self
            .entry
            .and_then(|index| self.entries.get(index))
            .cloned()
        else {
            return;
        };
        let ids = self.selected.clone();
        let root = Self::root(cx);
        self.busy = true;
        cx.spawn(async move |recovery, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    ids.into_iter()
                        .map(|id| Notebook::read_recovery_note(&root, &entry, &id))
                        .collect::<std::io::Result<Vec<_>>>()
                })
                .await;
            let result = match result {
                Ok(sources) => {
                    let requested = cx.update(|cx| {
                        let library = cx.global::<AppState>().notebook.clone();
                        let pending = library.borrow_mut().request_restore_copies(sources)?;
                        Ok::<_, std::io::Error>((library, pending))
                    });
                    match requested {
                        Ok((library, pending)) => {
                            let completed = pending.wait().await;
                            library.borrow_mut().complete(completed)
                        }
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(error),
            };
            let _ = recovery.update(cx, |recovery, cx| {
                recovery.busy = false;
                match result {
                    Ok(notes) => {
                        recovery.open_restored(&notes, cx);
                        crate::update_menus(cx);
                        recovery.refresh(cx);
                    }
                    Err(error) => {
                        recovery.status =
                            crate::copy::failed("Couldn’t restore the selected items.", &error)
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    /// Open restored copies; the status keeps any copy that could not be opened.
    fn open_restored(&mut self, notes: &[crate::NoteRecord], cx: &mut Context<Self>) {
        let failures: Vec<_> = notes
            .iter()
            .filter_map(|note| {
                crate::open_note(&note.id, cx)
                    .err()
                    .map(|error| format!("“{}”: {error}", note.title))
            })
            .collect();
        let restored = match notes.len() {
            1 => "Restored 1 note as a copy.".to_owned(),
            count => format!("Restored {count} notes as copies."),
        };
        self.status = if failures.is_empty() {
            restored
        } else {
            let count = failures.len();
            let failed = if count == 1 {
                "1 couldn’t be opened. Choose it".to_owned()
            } else {
                format!("{count} couldn’t be opened. Choose them")
            };
            format!(
                "{restored} {failed} in the Window menu to try again. {}",
                failures.join(" ")
            )
        };
    }
    fn export(&mut self, snapshot: bool, cx: &mut Context<Self>) {
        if self.loading || (!snapshot && self.selected.is_empty()) {
            return;
        }
        let Some(entry) = self
            .entry
            .and_then(|index| self.entries.get(index))
            .cloned()
        else {
            return;
        };
        if snapshot && !entry.kind.is_snapshot() {
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export".into()),
        });
        let sources = self.selected_sources();
        let root = Self::root(cx);
        self.busy = true;
        self.action_task = Some(cx.spawn(async move |recovery, cx| {
            let result: Result<Option<PathBuf>, String> = async {
                let Some(paths) = picker
                    .await
                    .map_err(|error| {
                        log::warn!("Cannot use the folder picker: {error}");
                        "Couldn’t open the folder picker.".to_owned()
                    })?
                    .map_err(|error| {
                        log::warn!("Cannot use the folder picker: {error}");
                        "Couldn’t open the folder picker.".to_owned()
                    })?
                else {
                    return Ok(None);
                };
                let folder = paths.first().ok_or("Choose a folder.")?.clone();
                let name = export_name(snapshot);
                cx.background_executor()
                    .spawn(async move {
                        crate::transfer::safe_directory(&folder)?;
                        let destination = folder.join(name);
                        export_recovered(&root, &entry, sources, snapshot, &destination)?;
                        Ok::<_, std::io::Error>(Some(destination))
                    })
                    .await
                    .map_err(|error| crate::copy::plain(&error))
            }
            .await;
            let _ = recovery.update(cx, |recovery, cx| {
                recovery.busy = false;
                match result {
                    Ok(Some(path)) => {
                        recovery.status = format!("Exported to {}.", crate::copy::file(&path))
                    }
                    Ok(None) => {}
                    Err(error) => {
                        recovery.status = format!(
                            "{} {error}",
                            if snapshot {
                                "Couldn’t export this snapshot."
                            } else {
                                "Couldn’t export the selected notes."
                            }
                        )
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn entry_keys(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.entries.is_empty() || self.busy {
            return;
        }
        if self.mode != Mode::Snapshots
            && event.keystroke.modifiers.secondary()
            && event.keystroke.key == "a"
        {
            self.records = self
                .entries
                .iter()
                .filter_map(|entry| entry.note.clone())
                .collect();
            self.selected = self.records.iter().map(|note| note.id.clone()).collect();
            self.show_preview(cx);
            cx.stop_propagation();
            return;
        }
        let current = self.entry.unwrap_or(0);
        let next = match event.keystroke.key.as_str() {
            "down" => {
                if self.entry.is_none() {
                    0
                } else {
                    (current + 1).min(self.entries.len() - 1)
                }
            }
            "up" => current.saturating_sub(1),
            "home" => 0,
            "end" => self.entries.len() - 1,
            "enter" | "space" => current,
            _ => return,
        };
        self.choose_entry(
            next,
            event.keystroke.modifiers.shift || event.keystroke.modifiers.secondary(),
            cx,
        );
        cx.stop_propagation();
    }
    fn note_keys(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.records.is_empty() || self.busy {
            return;
        }
        if event.keystroke.modifiers.secondary() && event.keystroke.key == "a" {
            self.selected = self
                .records
                .iter()
                .map(|record| record.id.clone())
                .collect();
            self.show_preview(cx);
        } else {
            let current = self.note_cursor.unwrap_or(0);
            let next = match event.keystroke.key.as_str() {
                "down" => (current + 1).min(self.records.len() - 1),
                "up" => current.saturating_sub(1),
                "home" => 0,
                "end" => self.records.len() - 1,
                "enter" | "space" => current,
                _ => return,
            };
            let id = self.records[next].id.clone();
            self.select_note(
                id,
                event.keystroke.modifiers.shift || event.keystroke.modifiers.secondary(),
                cx,
            );
            self.note_scroll
                .scroll_to_item(next, ScrollStrategy::Nearest);
        }
        cx.stop_propagation();
    }
    fn entry_rows(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        range
            .filter_map(|index| self.entries.get(index).map(|entry| (index, entry)))
            .map(|(index, entry)| {
                Button::new(("recovery-entry", index))
                    .role(Role::ListBoxOption)
                    .focusable(false)
                    .tab_stop(false)
                    .accessibility_label(entry.title.clone())
                    .aria_selected(if self.mode == Mode::Snapshots {
                        self.entry == Some(index)
                    } else {
                        entry
                            .note
                            .as_ref()
                            .is_some_and(|note| self.selected.contains(&note.id))
                    })
                    .when(self.entry == Some(index), |row| {
                        row.aria_active_descendant()
                    })
                    .h(px(24.))
                    .w_full()
                    .px(px(8.))
                    .justify_start()
                    .overflow_hidden()
                    .map(|row| {
                        let selected = (self.mode == Mode::Snapshots && self.entry == Some(index))
                            || (self.mode != Mode::Snapshots
                                && entry
                                    .note
                                    .as_ref()
                                    .is_some_and(|note| self.selected.contains(&note.id)));
                        self.row_colours(row, selected, self.entries_focused)
                    })
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap(px(12.))
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .child(entry.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .opacity(0.8)
                                    .font_features(FontFeatures(std::sync::Arc::new(vec![(
                                        "tnum".into(),
                                        1,
                                    )])))
                                    .child(local_date(entry.modified_at)),
                            ),
                    )
                    .on_click(
                        cx.listener(move |recovery, event: &ClickEvent, window, cx| {
                            window.focus(&recovery.entry_focus, cx);
                            recovery.choose_entry(
                                index,
                                event.modifiers().secondary() || event.modifiers().shift,
                                cx,
                            );
                        }),
                    )
                    .into_any_element()
            })
            .collect()
    }
    fn note_rows(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        range
            .filter_map(|index| self.records.get(index).map(|record| (index, record)))
            .map(|(index, record)| {
                let id = record.id.clone();
                Button::new(SharedString::from(format!("recovery-note-{id}")))
                    .role(Role::ListBoxOption)
                    .focusable(false)
                    .tab_stop(false)
                    .accessibility_label(record.title.clone())
                    .aria_selected(self.selected.contains(&id))
                    .when(self.note_cursor.unwrap_or(0) == index, |row| {
                        row.aria_active_descendant()
                    })
                    .h(px(24.))
                    .w_full()
                    .px(px(8.))
                    .justify_start()
                    .overflow_hidden()
                    .map(|row| {
                        self.row_colours(row, self.selected.contains(&id), self.notes_focused)
                    })
                    .child(record.title.clone())
                    .on_click(
                        cx.listener(move |recovery, event: &ClickEvent, window, cx| {
                            window.focus(&recovery.note_focus, cx);
                            recovery.select_note(
                                id.clone(),
                                event.modifiers().secondary() || event.modifiers().shift,
                                cx,
                            );
                        }),
                    )
                    .into_any_element()
            })
            .collect()
    }
    /// A selected row is the accent with white text while its list has the keyboard, and
    /// grey otherwise, as in every AppKit list.
    fn row_colours(&self, row: Button, selected: bool, focused: bool) -> Button {
        let palette = self.palette;
        match (selected, focused) {
            (true, true) => row.bg(palette.selected).text_color(palette.selected_text),
            (true, false) => row.bg(palette.unfocused).text_color(palette.text),
            _ => row.bg(transparent_black()).text_color(palette.text),
        }
    }
    fn button(
        &self,
        label: &'static str,
        id: &'static str,
        command: Command,
        enabled: bool,
        default: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let recovery = cx.entity().downgrade();
        let enabled = enabled && !self.busy;
        panel::button(id, label, self.palette, default && enabled)
            .disabled(!enabled)
            .when(!enabled, |button| {
                button.text_color(self.palette.secondary).cursor_default()
            })
            .on_click(move |_, window, cx| {
                let _ = recovery.update(cx, |recovery, cx| recovery.command(command, window, cx));
            })
    }
    /// One segment of the mode control.
    fn mode(
        &self,
        label: &'static str,
        id: &'static str,
        mode: Mode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let recovery = cx.entity().downgrade();
        let current = self.mode == mode;
        let palette = self.palette;
        panel::segment(
            panel::button(id, label, palette, false).debug_selector(|| id.into()),
            current,
            move |window, cx| {
                let _ = recovery.update(cx, |recovery, cx| {
                    recovery.command(Command::Mode(mode), window, cx)
                });
            },
        )
        .rounded(px(0.))
        .when(current, |segment| {
            segment
                .bg(palette.selected)
                .text_color(palette.selected_text)
                .border_color(palette.selected)
        })
    }
}
impl Render for Recovery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The status line is not a live region in GPUI, so say each new status.
        self.announced.say(&self.status);
        self.palette = Palette::of(window);
        self.entries_focused = self.entry_focus.is_focused(window);
        self.notes_focused = self.note_focus.is_focused(window);
        let palette = self.palette;
        let list = move |list: Stateful<Div>| {
            list.overflow_hidden()
                .bg(palette.field)
                .rounded(px(5.))
                .border_1()
                .border_color(palette.field_border)
                .focus_visible(move |style| style.border_2().border_color(palette.focus))
        };
        let entries_view = cx.entity().downgrade();
        let notes_view = cx.entity().downgrade();
        let entries = uniform_list(
            "recovery-entry-list",
            self.entries.len(),
            move |range, _, cx| {
                entries_view
                    .update(cx, |recovery, cx| recovery.entry_rows(range, cx))
                    .unwrap_or_default()
            },
        )
        .size_full()
        .track_scroll(&self.entry_scroll);
        let records = uniform_list(
            "recovery-note-list",
            self.records.len(),
            move |range, _, cx| {
                notes_view
                    .update(cx, |recovery, cx| recovery.note_rows(range, cx))
                    .unwrap_or_default()
            },
        )
        .size_full()
        .track_scroll(&self.note_scroll);
        let snapshot = self.mode == Mode::Snapshots;
        let selected = !self.selected.is_empty() && !self.loading;
        let entry_snapshot = self
            .entry
            .and_then(|index| self.entries.get(index))
            .is_some_and(|entry| entry.kind.is_snapshot());
        let empty = empty_list(self.mode, self.loading);
        let preview_hint = (self.preview.is_empty() && !self.entries.is_empty() && !self.loading)
            .then_some(PREVIEW_HINT);
        let notes_hint = (self.records.is_empty() && !self.loading).then_some(NOTES_HINT);
        panel::root(palette)
            .relative()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .track_focus(&self.focus)
            // Escape and Command-W close the window; Return restores, unless a list has the
            // keyboard, where it selects.
            .capture_key_down(|event: &KeyDownEvent, window, cx| {
                if panel::bare(event, "escape") {
                    window.remove_window();
                    cx.stop_propagation();
                }
            })
            .on_key_down(cx.listener(|recovery, event: &KeyDownEvent, window, cx| {
                if panel::bare(event, "enter") && !recovery.selected.is_empty() {
                    recovery.command(Command::Restore, window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_action(|_: &crate::CloseNote, window, _| window.remove_window())
            .child(
                div().flex().justify_center().child(
                    div()
                        .id("recovery-mode")
                        .role(Role::RadioGroup)
                        .aria_label("View")
                        .flex()
                        .rounded(px(5.))
                        .overflow_hidden()
                        .child(self.mode("Archived Notes", "recovery-archived", Mode::Archived, cx))
                        .child(self.mode("History", "recovery-snapshots", Mode::Snapshots, cx))
                        .child(self.mode("Trash", "recovery-trash", Mode::Trash, cx)),
                ),
            )
            .child(
                list(
                    div()
                        .id("recovery-entries")
                        .debug_selector(|| "recovery-entries".into())
                        .role(Role::ListBox)
                        .aria_label(match self.mode {
                            Mode::Archived => "Archived Notes",
                            Mode::Snapshots => "History",
                            Mode::Trash => "Trash",
                        })
                        .track_focus(&self.entry_focus)
                        .tab_index(0)
                        .on_key_down(cx.listener(|recovery, event: &KeyDownEvent, _, cx| {
                            recovery.entry_keys(event, cx)
                        }))
                        .h(px(150.)),
                )
                .child(if self.entries.is_empty() {
                    // In place of the list, which would fill the box and push this below it.
                    div()
                        .debug_selector(|| "recovery-empty".into())
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(16.))
                        .text_center()
                        .text_color(palette.secondary)
                        .child(empty)
                        .into_any_element()
                } else {
                    entries.into_any_element()
                }),
            )
            .when(snapshot, |container| {
                container.child("Notes").child(
                    list(
                        div()
                            .id("recovery-notes")
                            .role(Role::ListBox)
                            .aria_label("Notes in selected snapshot")
                            .track_focus(&self.note_focus)
                            .tab_index(0)
                            .on_key_down(cx.listener(|recovery, event: &KeyDownEvent, _, cx| {
                                recovery.note_keys(event, cx)
                            }))
                            .h(px(115.)),
                    )
                    .child(if let Some(hint) = notes_hint {
                        hint_text(hint, palette).into_any_element()
                    } else {
                        records.into_any_element()
                    }),
                )
            })
            .child(
                div()
                    .id("recovery-preview")
                    .role(Role::Group)
                    .aria_label("Preview")
                    .flex_1()
                    .min_h(px(90.))
                    .overflow_y_scroll()
                    .p(px(8.))
                    .bg(palette.field)
                    .rounded(px(5.))
                    .border_1()
                    .border_color(palette.field_border)
                    .font_family(crate::fonts(cx).code)
                    .text_size(px(12.))
                    .child(if let Some(hint) = preview_hint {
                        hint_text(hint, palette).into_any_element()
                    } else {
                        SelectableText::new("recovery-preview-text", self.preview.clone())
                            .into_any_element()
                    }),
            )
            .when(self.preview_truncated, |root| {
                root.child(
                    div()
                        .text_size(px(11.))
                        .text_color(palette.secondary)
                        .child(
                            "Showing the first 12,000 characters. Restoring or exporting keeps the whole note.",
                        ),
                )
            })
            .child(
                div()
                    .id("recovery-status")
                    .role(Role::Status)
                    .min_h(px(14.))
                    .text_size(px(11.))
                    .text_color(palette.secondary)
                    .child(if self.busy {
                        "Working…".to_owned()
                    } else if self.loading {
                        "Loading…".to_owned()
                    } else {
                        self.status.clone()
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.button(
                        crate::platform::REVEAL_LABEL,
                        "recovery-reveal",
                        Command::Reveal,
                        true,
                        false,
                        cx,
                    ))
                    .when(!snapshot, |row| {
                        row.child(self.button(
                            "Forget Note",
                            "recovery-forget",
                            Command::Forget,
                            selected,
                            false,
                            cx,
                        ))
                    })
                    .child(div().flex_1())
                    .when(snapshot, |row| {
                        row.child(self.button(
                            "Export Snapshot…",
                            "recovery-export-snapshot",
                            Command::ExportSnapshot,
                            entry_snapshot && !self.loading,
                            false,
                            cx,
                        ))
                    })
                    .child(self.button(
                        "Export Notes…",
                        "recovery-export",
                        Command::ExportNotes,
                        selected,
                        false,
                        cx,
                    ))
                    .child(self.button(
                        if snapshot {
                            "Restore as Copies"
                        } else {
                            "Restore"
                        },
                        "recovery-restore",
                        Command::Restore,
                        selected,
                        true,
                        cx,
                    )),
            )
            .child(TextSelectionLayer)
    }
}

/// What an empty list says: what the place is, and how things get into it.
fn empty_list(mode: Mode, loading: bool) -> &'static str {
    match mode {
        _ if loading => "Loading…",
        Mode::Archived => "No archived notes. Notes you archive when closing appear here.",
        Mode::Snapshots => "No snapshots yet. Choose File ▸ Save Snapshot to add one.",
        Mode::Trash => "Trash is empty. Notes you delete appear here.",
    }
}
const PREVIEW_HINT: &str = "Choose a note to preview it.";
const NOTES_HINT: &str = "Choose a snapshot to see its notes.";
/// A line of guidance in the middle of an otherwise empty box.
fn hint_text(text: &'static str, palette: Palette) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_center()
        .text_color(palette.secondary)
        .child(text)
}
fn local_date(milliseconds: u64) -> String {
    if milliseconds == 0 {
        return String::new();
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSDate, NSDateFormatter, NSDateFormatterStyle};
        let date = NSDate::dateWithTimeIntervalSince1970(milliseconds as f64 / 1000.);
        NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(
            &date,
            NSDateFormatterStyle::ShortStyle,
            NSDateFormatterStyle::ShortStyle,
        )
        .to_string()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let minutes = crate::storage::timestamp().saturating_sub(milliseconds) / 60000;
        if minutes < 1 {
            "Just now".into()
        } else if minutes < 60 {
            format!(
                "{} ago",
                crate::copy::count(minutes as usize, "minute", "minutes")
            )
        } else if minutes < 1440 {
            format!(
                "{} ago",
                crate::copy::count((minutes / 60) as usize, "hour", "hours")
            )
        } else {
            format!(
                "{} ago",
                crate::copy::count((minutes / 1440) as usize, "day", "days")
            )
        }
    }
}

/// A Recovery export's folder, named the way Finder names screenshots:
/// "Dahlia Notes 2026-10-14 at 20.31.05".
/// Write the snapshot, or the selected notes, as a new folder at `destination`. The folder
/// is built beside it and renamed into place, so a failure leaves no partial export.
fn export_recovered(
    root: &std::path::Path,
    entry: &RecoveryEntry,
    sources: Vec<(RecoveryEntry, String)>,
    snapshot: bool,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    crate::transfer::publish_directory(destination, |staged| {
        if snapshot {
            return Notebook::export_snapshot(root, entry, staged);
        }
        let sources = sources
            .into_iter()
            .map(|(entry, id)| Notebook::read_recovery_note(root, &entry, &id))
            .collect::<std::io::Result<Vec<_>>>()?;
        std::fs::create_dir(staged)?;
        for source in sources {
            crate::attachments::publish(staged, &source.images)?;
            let filename = format!(
                "{}-{}.md",
                crate::transfer::filename(&source.record.title).trim_end_matches(".md"),
                source.record.id
            );
            crate::transfer::export_new(&staged.join(filename), &source.text)?;
        }
        Ok(())
    })
}
fn export_name(snapshot: bool) -> String {
    let kind = if snapshot { "Snapshot" } else { "Notes" };
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSDate, NSDateFormatter, NSString};
        let formatter = NSDateFormatter::new();
        formatter.setDateFormat(Some(&NSString::from_str("yyyy-MM-dd 'at' HH.mm.ss")));
        format!("Dahlia {kind} {}", formatter.stringFromDate(&NSDate::now()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        format!("Dahlia {kind} {}", crate::storage::timestamp())
    }
}

pub fn show(cx: &mut App) {
    if let Some(handle) = cx.global::<AppState>().recovery_window {
        if handle
            .update(cx, |recovery, window, cx| {
                recovery.refresh(cx);
                window.activate_window();
            })
            .is_ok()
        {
            return;
        }
        cx.global_mut::<AppState>().recovery_window = None;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(820.), px(650.)),
            cx,
        ))),
        window_min_size: Some(size(px(790.), px(600.))),
        titlebar: Some(TitlebarOptions {
            title: Some("Recovery".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| cx.new(|cx| Recovery::new(window, cx))) {
        Ok(handle) => {
            cx.global_mut::<AppState>().recovery_window = Some(handle);
            crate::app::sync_panel_levels(cx);
        }
        Err(error) => {
            log::warn!("Cannot open the Recovery window: {error}");
            crate::notify::notify(cx, "Couldn’t open Recovery.");
        }
    }
}
pub fn checkpoint(cx: &mut App) {
    let handles = cx
        .global::<AppState>()
        .windows
        .values()
        .copied()
        .collect::<Vec<_>>();
    let mut saves = Vec::new();
    for handle in handles {
        let pending = handle.update(cx, |note, window, cx| {
            note.capture(window);
            let placement = note.persist_record();
            (note.request_save(cx), placement)
        });
        match pending {
            Ok((pending, Ok(()))) => saves.push(pending),
            _ => {
                crate::notify::notify(
                    cx,
                    "Couldn’t save a snapshot because a note couldn’t be saved.",
                );
                return;
            }
        }
    }
    cx.spawn(async move |cx| {
        let mut saved = true;
        for pending in saves {
            saved &= pending.await.is_ok();
        }
        if !saved {
            cx.update(|cx| {
                crate::notify::notify(
                    cx,
                    "Couldn’t save a snapshot because a note couldn’t be saved.",
                )
            });
            return;
        }
        let checkpoint = cx.update(|cx| {
            cx.global::<AppState>()
                .notebook
                .borrow()
                .request_checkpoint()
        });
        let result = checkpoint.await;
        cx.update(|cx| {
            match result {
                // The snapshot appears in Recovery’s History; success needs no alert.
                Ok(_) => {}
                Err(error) => crate::notify::notify(
                    cx,
                    crate::copy::failed("Couldn’t save a snapshot.", &error),
                ),
            }

            if let Some(handle) = cx.global::<AppState>().recovery_window {
                let _ = handle.update(cx, |recovery, _, cx| recovery.refresh(cx));
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::{Mode, Recovery, empty_list, export_recovered};
    use crate::{
        AppState, NoteState, Notebook,
        notebook::{RecoveryEntry, RecoveryKind},
    };
    use gpui::{AnyWindowHandle, TestAppContext};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn empty_lists_say_what_the_place_is_and_how_to_fill_it() {
        assert_eq!(empty_list(Mode::Archived, true), "Loading…");
        for mode in [Mode::Archived, Mode::Snapshots, Mode::Trash] {
            let text = empty_list(mode, false);
            assert!(text.ends_with('.') && text.contains(' '), "{text}");
        }
        assert_eq!(
            empty_list(Mode::Trash, false),
            "Trash is empty. Notes you delete appear here."
        );
    }

    /// The list fills its box, so the message has to replace it: beside it, the message
    /// falls below the bottom edge and is clipped away.
    #[gpui::test]
    fn an_empty_list_shows_its_message_inside_the_box(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let _root_fixture = tempfile::tempdir().unwrap();
        let library = Rc::new(RefCell::new(
            Notebook::open(_root_fixture.path().to_path_buf()).unwrap(),
        ));
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(AppState::new(
                library.clone(),
                crate::appearance::Fonts::available(&[]),
            ));
        });
        let (recovery, cx) = cx.add_window_view(Recovery::new);
        for mode in [Mode::Archived, Mode::Trash, Mode::Snapshots] {
            recovery.update(cx, |recovery, cx| {
                recovery.mode = mode;
                recovery.refresh(cx);
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            recovery.read_with(cx, |recovery, _| {
                assert!(recovery.entries.is_empty() && !recovery.loading);
            });
            let list = cx.debug_bounds("recovery-entries").unwrap();
            let message = cx.debug_bounds("recovery-empty").unwrap();
            assert!(list.contains(&message.center()), "{list:?} {message:?}");
            assert!(message.bottom() <= list.bottom(), "{list:?} {message:?}");
        }
        recovery.update(cx, |recovery, _| {
            recovery.preview_task.take();
            recovery.load_task.take();
        });
        cx.update(|_, cx| {
            cx.remove_global::<AppState>();
        });
    }

    #[gpui::test]
    fn the_mode_segments_switch_the_list_when_pressed(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let _root_fixture = tempfile::tempdir().unwrap();
        let library = Rc::new(RefCell::new(
            Notebook::open(_root_fixture.path().to_path_buf()).unwrap(),
        ));
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(AppState::new(
                library.clone(),
                crate::appearance::Fonts::available(&[]),
            ));
        });
        let (recovery, cx) = cx.add_window_view(Recovery::new);
        for (id, mode) in [
            ("recovery-trash", Mode::Trash),
            ("recovery-snapshots", Mode::Snapshots),
            ("recovery-archived", Mode::Archived),
        ] {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let segment = cx.debug_bounds(id).unwrap();
            cx.simulate_click(segment.center(), gpui::Modifiers::default());
            cx.run_until_parked();
            assert!(recovery.read_with(cx, |recovery, _| recovery.mode == mode));
        }
        recovery.update(cx, |recovery, _| {
            recovery.preview_task.take();
            recovery.load_task.take();
        });
        cx.update(|_, cx| {
            cx.remove_global::<AppState>();
        });
    }

    #[gpui::test]
    fn restore_as_copies_keeps_open_failures_visible(cx: &mut TestAppContext) {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
        let mut notes = Vec::new();
        for _ in 0..2 {
            let mut note = library
                .borrow_mut()
                .create(crate::notebook::Frame::default())
                .unwrap();
            note.open = false;
            library.borrow_mut().update(note.clone()).unwrap();
            notes.push(note);
        }
        // A source that can no longer be read keeps its copy from opening.
        let source = root.join("notes").join(&notes[1].id).join("note.md");
        std::fs::remove_file(&source).unwrap();
        std::fs::create_dir(&source).unwrap();
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(AppState::new(
                library.clone(),
                crate::appearance::Fonts::available(&[]),
            ));
        });
        let handle = cx.add_window(Recovery::new);
        handle
            .update(cx, |recovery, _, cx| {
                recovery.open_restored(&notes, cx);
                assert!(
                    recovery.status.starts_with("Restored 2 notes as copies"),
                    "{}",
                    recovery.status
                );
                assert!(
                    recovery
                        .status
                        .contains("1 couldn’t be opened. Choose it in the Window menu to try again."),
                    "{}",
                    recovery.status
                );
                recovery.refresh(cx);
                assert!(recovery.status.contains("1 couldn’t be opened"));
            })
            .unwrap();
    }
    #[gpui::test]
    fn recovery_lists_draw_and_multiple_archived_selections_keep_their_sources(
        cx: &mut TestAppContext,
    ) {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
        let a = library
            .borrow_mut()
            .create(crate::notebook::Frame::default())
            .unwrap();
        let b = library
            .borrow_mut()
            .create(crate::notebook::Frame::default())
            .unwrap();
        for (note, text) in [(&a, "A exact\r\n"), (&b, "B exact\r\n")] {
            let (mut store, _) = library.borrow().open_store(&note.id).unwrap();
            store.save(text).unwrap();
            drop(store);
            library
                .borrow_mut()
                .transition(note.clone(), NoteState::Active, NoteState::Archived)
                .unwrap();
        }
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(AppState::new(
                library.clone(),
                crate::appearance::Fonts::available(&[]),
            ));
        });
        let handle = cx.add_window(Recovery::new);
        handle
            .update(cx, |recovery, _, cx| {
                recovery.choose_entry(0, false, cx);
                recovery.choose_entry(1, true, cx);
                assert_eq!(recovery.selected.len(), 2);
                let sources = recovery.selected_sources();
                let root = library.borrow().root().to_owned();
                let mut texts = sources
                    .iter()
                    .map(|(entry, id)| Notebook::read_recovery_note(&root, entry, id).unwrap().text)
                    .collect::<Vec<_>>();
                texts.sort();
                assert_eq!(texts, vec!["A exact\r\n", "B exact\r\n"]);
            })
            .unwrap();
        AnyWindowHandle::from(handle)
            .update(cx, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();
        handle
            .update(cx, |recovery, _, _| {
                recovery.mode = Mode::Snapshots;
                recovery.entries = library.borrow().recovery_entries().unwrap();
                recovery.entry = None;
                recovery.records.clear();
                recovery.selected.clear();
            })
            .unwrap();
        AnyWindowHandle::from(handle)
            .update(cx, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();
        handle
            .update(cx, |recovery, _, cx| {
                recovery.mode = Mode::Trash;
                recovery.refresh(cx);
            })
            .unwrap();
        AnyWindowHandle::from(handle)
            .update(cx, |_, window, cx| {
                window.draw(cx).clear(cx);
            })
            .unwrap();
        handle
            .update(cx, |recovery, window, _| {
                recovery.preview_task.take();
                recovery.load_task.take();
                window.remove_window();
            })
            .unwrap();
        cx.update(|cx| {
            cx.remove_global::<AppState>();
        });
        drop(library);
    }

    #[gpui::test]
    fn forget_removes_archived_notes_from_the_list_and_keeps_their_files(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
        let mut ids = Vec::new();
        for text in ["kept file\n", "other\n"] {
            let note = library
                .borrow_mut()
                .create(crate::notebook::Frame::default())
                .unwrap();
            let (mut store, _) = library.borrow().open_store(&note.id).unwrap();
            store.save(text).unwrap();
            drop(store);
            library
                .borrow_mut()
                .transition(note.clone(), NoteState::Active, NoteState::Archived)
                .unwrap();
            ids.push(note.id);
        }
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(AppState::new(
                library.clone(),
                crate::appearance::Fonts::available(&[]),
            ));
        });
        let handle = cx.add_window(Recovery::new);
        handle
            .update(cx, |recovery, _, cx| {
                recovery.choose_entry(0, false, cx);
                recovery.forget(cx);
                assert!(recovery.busy);
            })
            .unwrap();
        crate::settle_io(cx);
        let status = handle.update(cx, |recovery, _, _| recovery.status.clone());
        let forgotten: Vec<_> = ids
            .iter()
            .filter(|id| library.borrow().note(id).is_none())
            .collect();
        assert_eq!(forgotten.len(), 1, "{status:?}");
        assert_eq!(
            status.unwrap(),
            "Removed 1 note from Archived Notes. Its file is still in History."
        );
        assert!(root.join("notes").join(forgotten[0]).join("note.md").exists());
        handle
            .update(cx, |recovery, window, _| {
                assert!(!recovery.busy);
                assert_eq!(recovery.entries.len(), 1);
                recovery.preview_task.take();
                recovery.load_task.take();
                window.remove_window();
            })
            .unwrap();
        cx.update(|cx| {
            cx.remove_global::<AppState>();
        });
        drop(library);
    }

    #[test]
    fn a_partial_notes_export_removes_the_folder_it_started() {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().join("library");
        let mut library = Notebook::open(root.clone()).unwrap();
        let note = library
            .create(crate::notebook::Frame::default())
            .unwrap();
        let (mut store, _) = library.open_store(&note.id).unwrap();
        store.save("A exact\n").unwrap();
        drop(store);
        let note = library
            .transition(note, NoteState::Active, NoteState::Archived)
            .unwrap();
        let path = library.root().join("notes").join(&note.id).join("note.md");
        let entry = RecoveryEntry {
            title: note.title.clone(),
            path,
            modified_at: 0,
            kind: RecoveryKind::Source,
            note: Some(note.clone()),
        };
        let folder = _root_fixture.path().join("out");
        std::fs::create_dir(&folder).unwrap();
        let destination = folder.join("Exported");
        // The same note twice writes its file, then collides with it.
        let doubled = vec![(entry.clone(), note.id.clone()); 2];
        assert!(export_recovered(library.root(), &entry, doubled, false, &destination).is_err());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        let single = vec![(entry.clone(), note.id.clone())];
        export_recovered(library.root(), &entry, single, false, &destination).unwrap();
        let exported: Vec<_> = std::fs::read_dir(&destination)
            .unwrap()
            .map(|file| std::fs::read_to_string(file.unwrap().path()).unwrap())
            .collect();
        assert_eq!(exported, ["A exact\n"]);
        library.finish_for_test();
    }
}
