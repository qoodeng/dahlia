use crate::{AppState, Frame, Note, NoteError, NoteState, notify::notify, open_note, transfer, update_menus};
use gpui::{prelude::*, *};

pub fn undo_id(cx: &App) -> Option<String> {
    let state = cx.global::<AppState>();
    state
        .undo_deletes
        .iter()
        .rev()
        .find(|id| {
            state
                .notebook
                .borrow()
                .note(id)
                .is_some_and(|n| n.state == NoteState::Trash)
        })
        .cloned()
}
pub fn redo_id(cx: &App) -> Option<String> {
    let state = cx.global::<AppState>();
    state
        .redo_deletes
        .iter()
        .rev()
        .find(|id| {
            state.windows.contains_key(*id)
                && state
                    .notebook
                    .borrow()
                    .note(id)
                    .is_some_and(|n| n.state == NoteState::Active)
        })
        .cloned()
}
pub fn restore(id: &str, expected: NoteState, for_undo: bool, cx: &mut App) {
    restore_many(vec![id.to_owned()], expected, for_undo, cx);
}
/// A window or file picker went away mid-operation. The cause goes to the log.
fn interrupted(error: impl std::fmt::Display) -> String {
    log::warn!("Operation interrupted: {error}");
    "Dahlia couldn’t finish because a window closed. Try again.".to_owned()
}
/// Markdown that can't be written. The renderer's wording goes to the log.
fn unwritable(error: impl std::fmt::Display) -> String {
    log::warn!("Cannot render Markdown: {error}");
    "This note can’t be written as Markdown right now.".to_owned()
}
pub fn restore_many(ids: Vec<String>, expected: NoteState, for_undo: bool, cx: &mut App) {
    if cx.global::<AppState>().quitting {
        return;
    }
    cx.spawn(async move |cx| {
        for id in ids {
            let library = cx.update(|cx| cx.global::<AppState>().notebook.clone());
            let pending = library.borrow_mut().request_restore(&id, expected);
            let result = match pending {
                Ok(pending) => {
                    let completed = pending.wait().await;
                    library.borrow_mut().complete(completed)
                }
                Err(error) => Err(error),
            };
            cx.update(|cx| {
                match result {
                    Ok(_) => {
                        let state = cx.global_mut::<AppState>();
                        state.undo_deletes.retain(|item| item != &id);
                        state.redo_deletes.retain(|item| item != &id);
                        if for_undo {
                            state.redo_deletes.push(id.clone());
                        }
                        if let Err(error) = open_note(&id, cx) {
                            notify(cx, format!("Couldn’t open the restored note. {error}"));
                        }
                    }
                    Err(error) => notify(
                        cx,
                        crate::copy::failed("Couldn’t restore this note.", &error),
                    ),
                }
                update_menus(cx);
                if let Some(handle) = cx.global::<AppState>().recovery_window {
                    let _ = handle.update(cx, |panel, _, cx| panel.refresh(cx));
                }
            });
        }
    })
    .detach();
}
pub fn undo_delete(cx: &mut App) {
    if let Some(id) = undo_id(cx) {
        restore(&id, NoteState::Trash, true, cx);
    }
}
pub fn redo_delete(cx: &mut App) {
    if let Some(id) = redo_id(cx) {
        let handle = cx.global::<AppState>().windows.get(&id).copied();
        if let Some(handle) = handle {
            let redo = cx.global::<AppState>().redo_deletes.clone();
            let _ = handle.update(cx, |note, window, cx| {
                if note.close_task.is_none() {
                    note.redo_after_close = Some(redo);
                    note.finish_close(NoteState::Trash, window, cx);
                }
            });
        }
    }
}

pub fn import_notes(cx: &mut App) {
    if cx.global::<AppState>().import_task.is_some() {
        return;
    }
    let selected = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: Some("Import".into()),
    });
    cx.global_mut::<AppState>().import_task = Some(cx.spawn(async move |cx| {
        let result: Result<(), String> = async {
            let paths = match selected
                .await
                .map_err(interrupted)?
                .map_err(interrupted)?
            {
                Some(paths) => paths,
                None => return Ok(()),
            };
            let prepared = cx
                .background_executor()
                .spawn(async move { transfer::prepare(paths) })
                .await
                .map_err(|e| crate::copy::plain(&e))?;
            let linked_files = prepared.iter().any(|source| source.linked_files);
            let mut disclosures = prepared
                .iter()
                .flat_map(|source| source.disclosures.clone())
                .collect::<Vec<_>>();
            if linked_files {
                disclosures.push(
                    "Links to other files will still point to their original location.".into(),
                );
            }
            if !disclosures.is_empty() {
                let details = disclosures.join("\n");
                let response = cx.update(|cx| {
                    let handle = crate::active_window(cx)
                        .or_else(|| cx.global::<AppState>().windows.values().next().copied());
                    handle.and_then(|handle| {
                        handle
                            .update(cx, |_, window, cx| {
                                window.prompt(
                                    PromptLevel::Info,
                                    "Import notes?",
                                    Some(&details),
                                    &["Import", "Cancel"],
                                    cx,
                                )
                            })
                            .ok()
                    })
                });
                match response {
                    Some(response) => {
                        if response.await.ok() != Some(0) {
                            return Ok(());
                        }
                    }
                    None => {
                        return Err("Open a note, then choose Import Notes again.".into());
                    }
                }
            }
            let (library, pending) = cx.update(|cx| {
                let library = cx.global::<AppState>().notebook.clone();
                let defaults = library.borrow().defaults().clone();
                let frame = Frame {
                    width: defaults.width,
                    height: defaults.height,
                    ..Frame::default()
                }
                .restored(&crate::displays(cx), false);
                let pending = library
                    .borrow_mut()
                    .request_import(prepared, frame)
                    .map_err(|error| crate::copy::plain(&error))?;
                Ok::<_, String>((library, pending))
            })?;
            let completed = pending.wait().await;
            let imported = library
                .borrow_mut()
                .complete(completed)
                .map_err(|error| crate::copy::plain(&error))?;
            cx.update(|cx| {
                if let Err(partial) = open_imported(&imported, cx) {
                    notify(cx, partial);
                }
            });
            Ok(())
        }
        .await;
        cx.update(|cx| {
            cx.global_mut::<AppState>().import_task.take();
            if let Err(error) = result {
                notify(cx, format!("Couldn’t import notes. {error}"));
            }
            update_menus(cx);
        });
    }));
    update_menus(cx);
}

/// What Export All writes: every note outside Trash. Open notes use their current
/// text; closed and archived notes use their saved source.
type ExportJob = Box<dyn FnOnce() -> Result<transfer::ExportSource, String> + Send>;
fn capture_export_jobs(cx: &mut App) -> Result<Vec<ExportJob>, String> {
    let library = cx.global::<AppState>().notebook.clone();
    let notes = library.borrow().notes().to_vec();
    let root = library.borrow().root().to_owned();
    let mut jobs: Vec<ExportJob> = Vec::new();
    for record in notes
        .into_iter()
        .filter(|note| note.state != NoteState::Trash)
    {
        let window = cx.global::<AppState>().windows.get(&record.id).copied();
        match window {
            Some(handle) => {
                let (title, source, schema, document, base) = handle
                    .update(cx, |note, _, cx| {
                        if note.opening {
                            return Err("An exported note is still opening.".to_owned());
                        }
                        Ok((
                            note.record.title.clone(),
                            note.source.snapshot(),
                            note.schema.clone(),
                            note.editor.read(cx).committed_document().clone(),
                            note.record
                                .source_base
                                .clone()
                                .unwrap_or_else(|| note.asset_directory.clone()),
                        ))
                    })
                    .map_err(interrupted)??;
                jobs.push(Box::new(move || {
                    let text = source
                        .render(&schema, &document)
                        .map_err(unwritable)?;
                    Ok(transfer::ExportSource { title, text, base })
                }));
            }
            None => {
                let root = root.clone();
                jobs.push(Box::new(move || {
                    let title = record.title.clone();
                    let (text, base) = crate::Notebook::read_saved_source(&root, record)
                        .map_err(|error| format!("“{title}”: {}", crate::copy::plain(&error)))?;
                    Ok(transfer::ExportSource { title, text, base })
                }));
            }
        }
    }
    Ok(jobs)
}
#[cfg(test)]
pub fn export_sources(cx: &mut App) -> Result<Vec<transfer::ExportSource>, String> {
    capture_export_jobs(cx)?
        .into_iter()
        .map(|job| job())
        .collect()
}
/// Open freshly imported notes. They are already saved, so a note that cannot be opened
/// is reported as a partial success rather than a failed import.
pub fn open_imported(imported: &[crate::NoteRecord], cx: &mut App) -> Result<(), String> {
    let failures: Vec<_> = imported
        .iter()
        .filter_map(|note| {
            open_note(&note.id, cx)
                .err()
                .map(|error| format!("“{}”: {error}", note.title))
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Imported {}. {} couldn’t be opened. Choose {} in the Window menu to try again.\n\n{}",
        crate::copy::count(imported.len(), "note", "notes"),
        failures.len(),
        if failures.len() == 1 { "it" } else { "them" },
        failures.join("\n")
    ))
}
pub fn export_all(cx: &mut App) {
    if cx.global::<AppState>().import_task.is_some() {
        return;
    }
    let jobs = match capture_export_jobs(cx) {
        Ok(jobs) if jobs.is_empty() => return,
        Ok(jobs) => jobs,
        Err(error) => {
            notify(cx, format!("Couldn’t export notes. {error}"));
            return;
        }
    };
    let directory = std::env::var_os("DAHLIA_EXPORT_DIR")
        .or_else(|| std::env::var_os("ORTENSIA_RUST_EXPORT_DIR"))
        .map(std::path::PathBuf::from)
        .or_else(dirs::document_dir)
        .unwrap_or_else(std::env::temp_dir);
    let picker = cx.prompt_for_new_path(&directory, Some("Dahlia Notes"));
    cx.global_mut::<AppState>().import_task = Some(cx.spawn(async move |cx| {
        let result: Result<Option<transfer::ExportReport>, String> = async {
            let Some(destination) = picker
                .await
                .map_err(interrupted)?
                .map_err(interrupted)?
            else {
                return Ok(None);
            };
            cx.background_executor()
                .spawn(async move {
                    let sources = jobs
                        .into_iter()
                        .map(|job| job())
                        .collect::<Result<Vec<_>, _>>()?;
                    transfer::export_bundle(&destination, &sources)
                        .map_err(|error| crate::copy::plain(&error))
                })
                .await
                .map(Some)
        }
        .await;
        cx.update(|cx| {
            cx.global_mut::<AppState>().import_task.take();
            match result {
                Ok(Some(report)) => {
                    // The scope is the one thing the user can't see: archived notes are
                    // in, the Trash is not.
                    let exported = crate::copy::count(report.files.len(), "note", "notes");
                    let (message, details) = if report.issues.is_empty() {
                        (
                            format!("Exported {exported}."),
                            "Archived notes are included; notes in the Trash aren’t.".to_owned(),
                        )
                    } else {
                        (
                            format!("Exported {exported}, but some files weren’t included."),
                            format!(
                                "{}\n\nSee Export Report.txt in the export folder.",
                                report.issues.join("\n")
                            ),
                        )
                    };
                    crate::notify::inform(cx, &message, Some(&details));
                }
                Err(error) => notify(cx, format!("Couldn’t export notes. {error}")),
                Ok(None) => {}
            }
            update_menus(cx);
        });
    }));
    update_menus(cx);
}

impl Note {
    pub(crate) fn export(&mut self, archive: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.transfer_task.is_some() {
            return;
        }
        // Exporting the empty stand-in would write an empty file and report success.
        if self.opening {
            self.error = Some(NoteError::other("This note is still opening."));
            cx.notify();
            return;
        }
        let directory = std::env::var_os("DAHLIA_EXPORT_DIR")
            .or_else(|| std::env::var_os("ORTENSIA_RUST_EXPORT_DIR"))
            .map(std::path::PathBuf::from)
            .or_else(dirs::document_dir)
            .unwrap_or_else(std::env::temp_dir);
        let picker =
            cx.prompt_for_new_path(&directory, Some(&transfer::filename(&self.record.title)));
        let note_id = self.record.id.clone();
        let handle = window
            .window_handle()
            .downcast::<Note>()
            .expect("note window");
        self.transfer_task = Some(cx.spawn(async move |_, cx| {
            let result: Result<(), String> = async {
                let path = match picker
                    .await
                    .map_err(interrupted)?
                    .map_err(interrupted)?
                {
                    Some(path) => path,
                    None => return Ok(()),
                };
                let saved = handle
                    .update(cx, |note, window, cx| {
                        note.capture(window);
                        note.request_save(cx)
                    })
                    .map_err(interrupted)?;
                saved.await.map_err(|error| error.message)?;
                let (snapshot, schema, document, base, revision) = handle
                    .update(cx, |note, _, cx| {
                        (
                            note.source.snapshot(),
                            note.schema.clone(),
                            note.editor.read(cx).committed_document().clone(),
                            note.record
                                .source_base
                                .clone()
                                .unwrap_or_else(|| note.asset_directory.clone()),
                            note.generation,
                        )
                    })
                    .map_err(interrupted)?;
                let text = cx
                    .background_executor()
                    .spawn(async move {
                        snapshot
                            .render(&schema, &document)
                            .map_err(unwritable)
                    })
                    .await?;
                let links =
                    transfer::source_has_linked_files(&text).map_err(|error| crate::copy::plain(&error))?;
                let disclosure = if links {
                    Some(
                        handle
                            .update(cx, |_, window, cx| {
                                window.prompt(
                                    PromptLevel::Info,
                                    "Export without linked files?",
                                    Some(
                                        "Links to other files will still point to their original location.",
                                    ),
                                    &["Export", "Cancel"],
                                    cx,
                                )
                            })
                            .map_err(interrupted)?,
                    )
                } else {
                    None
                };
                if let Some(disclosure) = disclosure
                    && disclosure.await.ok() != Some(0)
                {
                    return Ok(());
                }
                cx.background_executor()
                    .spawn(async move { crate::attachments::export(&path, &text, &base) })
                    .await
                    .map_err(|e| crate::copy::plain(&e))?;
                handle
                    .update(cx, |note, window, cx| {
                        // The user already agreed to leave linked files behind, so only an
                        // edit made during the export keeps the note open.
                        if archive {
                            if note.generation != revision {
                                drop(window.prompt(
                                    PromptLevel::Info,
                                    &format!("Exported “{}”.", note.record.title),
                                    Some("The note changed during export, so it wasn’t archived. It’s still open."),
                                    &["OK"],
                                    cx,
                                ));
                            } else {
                                note.finish_close(NoteState::Archived, window, cx);
                            }
                        }
                    })
                    .map_err(interrupted)?;
                Ok(())
            }
            .await;
            let still_open = cx.update(|cx| cx.global::<AppState>().windows.contains_key(&note_id));
            if !still_open {
                return;
            }
            let _ = handle.update(cx, |note, window, cx| {
                note.transfer_task.take();
                if let Err(error) = result {
                    drop(window.prompt(
                        PromptLevel::Warning,
                        "Couldn’t export this note.",
                        Some(&error),
                        &["OK"],
                        cx,
                    ));
                }
            });
        }));
    }
}

#[cfg(test)]
mod tests {
    use crate::{AppState, Fonts, Frame, NoteRecord, NoteState, Notebook, open_note};
    use gpui::TestAppContext;
    use std::{cell::RefCell, rc::Rc};
    fn library_with_states(
        _label: &str,
        cx: &mut TestAppContext,
    ) -> (Rc<RefCell<Notebook>>, Vec<NoteRecord>, tempfile::TempDir) {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
        cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
        let mut notes = Vec::new();
        for (title, state) in [
            ("Open", NoteState::Active),
            ("Closed", NoteState::Active),
            ("Archived", NoteState::Archived),
            ("Trashed", NoteState::Trash),
        ] {
            let mut record = library.borrow_mut().create(Frame::default()).unwrap();
            let (mut store, _) = library.borrow().open_store(&record.id).unwrap();
            store.save(&format!("{title}\n")).unwrap();
            drop(store);
            record.title = title.into();
            record.open = false;
            library.borrow_mut().update(record.clone()).unwrap();
            if state != NoteState::Active {
                record = library
                    .borrow_mut()
                    .transition(record, NoteState::Active, state)
                    .unwrap();
            }
            notes.push(record);
        }
        (library, notes, _root_fixture)
    }
    #[gpui::test]
    fn export_all_includes_closed_and_archived_notes_but_not_trash(cx: &mut TestAppContext) {
        let (_library, notes, _root) = library_with_states("export-all", cx);
        cx.update(|cx| open_note(&notes[0].id, cx)).unwrap();
        let sources = cx.update(super::export_sources).unwrap();
        let mut texts: Vec<_> = sources.iter().map(|source| source.text.clone()).collect();
        texts.sort();
        assert_eq!(texts, vec!["Archived\n", "Closed\n", "Open\n"]);
    }
    #[gpui::test]
    fn import_reports_partial_success_after_committing(cx: &mut TestAppContext) {
        let (library, notes, fixture) = library_with_states("import-partial", cx);
        let root = fixture.path();
        let imported = vec![notes[0].clone(), notes[1].clone()];
        // One imported source becomes unreadable before its window opens.
        let source = root.join("notes").join(&notes[1].id).join("note.md");
        std::fs::remove_file(&source).unwrap();
        std::fs::create_dir(&source).unwrap();
        let error = cx
            .update(|cx| super::open_imported(&imported, cx))
            .unwrap_err();
        assert!(error.starts_with("Imported 2 notes"), "{error}");
        assert!(error.contains("1 couldn’t be opened. Choose it in the Window menu"), "{error}");
        assert!(cx.update(|cx| cx.global::<AppState>().windows.contains_key(&notes[0].id)));
        assert_eq!(library.borrow().notes().len(), 4);
    }
}
