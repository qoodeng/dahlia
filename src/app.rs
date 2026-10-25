use crate::*;

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{}: {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

pub(crate) struct AppState {
    pub(crate) notebook: Rc<RefCell<Notebook>>,
    pub(crate) windows: BTreeMap<String, WindowHandle<Note>>,
    pub(crate) opening_notes: std::collections::BTreeSet<String>,
    pub(crate) format_menus: BTreeMap<String, FormatMenuState>,
    pub(crate) active: Option<String>,
    pub(crate) fonts: Fonts,
    pub(crate) transparency_window: Option<WindowHandle<settings::GlassControl>>,
    pub(crate) appearance_save: Option<Task<()>>,
    pub(crate) undo_deletes: Vec<String>,
    pub(crate) redo_deletes: Vec<String>,
    pub(crate) import_task: Option<Task<()>>,
    pub(crate) quitting: bool,
    pub(crate) ready_to_quit: Rc<std::cell::Cell<bool>>,
    /// The system asked to terminate and is waiting for `finish_quit` to reply.
    pub(crate) termination_awaiting: Rc<std::cell::Cell<bool>>,
    pub(crate) find_window: Option<WindowHandle<finding::FindControl>>,
    pub(crate) find_query: String,
    pub(crate) recovery_window: Option<WindowHandle<recovery::Recovery>>,
    pub(crate) arrange_undo: Vec<Vec<window_commands::Geometry>>,
    pub(crate) arrange_redo: Vec<Vec<window_commands::Geometry>>,
    /// What the menu bar last showed; see `update_menus`.
    pub(crate) menus: Option<MenuInputs>,
    #[cfg(test)]
    pub(crate) menu_builds: usize,
}
impl Global for AppState {}
impl AppState {
    /// Forget a note window that is going away. If it was the active note, another open
    /// window takes over until the system activates one, so the menus never point at a
    /// note that has closed.
    pub(crate) fn release_window(&mut self, id: &str) {
        self.windows.remove(id);
        self.format_menus.remove(id);
        if self.active.as_deref() == Some(id) {
            self.active = self.windows.keys().next().cloned();
        }
    }
    pub(crate) fn new(notebook: Rc<RefCell<Notebook>>, fonts: Fonts) -> Self {
        Self {
            notebook,
            windows: BTreeMap::new(),
            opening_notes: Default::default(),
            format_menus: BTreeMap::new(),
            active: None,
            fonts,
            transparency_window: None,
            appearance_save: None,
            undo_deletes: Vec::new(),
            redo_deletes: Vec::new(),
            import_task: None,
            quitting: false,
            ready_to_quit: Rc::new(std::cell::Cell::new(false)),
            termination_awaiting: Rc::new(std::cell::Cell::new(false)),
            find_window: None,
            find_query: String::new(),
            recovery_window: None,
            arrange_undo: Vec::new(),
            arrange_redo: Vec::new(),
            menus: None,
            #[cfg(test)]
            menu_builds: 0,
        }
    }
}

pub(crate) fn fonts(cx: &App) -> Fonts {
    if cx.has_global::<AppState>() {
        cx.global::<AppState>().fonts.clone()
    } else {
        Fonts::available(&[])
    }
}
pub(crate) fn glass_tint(cx: &App) -> f32 {
    if cx.has_global::<AppState>() {
        cx.global::<AppState>().notebook.borrow().glass_tint()
    } else {
        appearance::DEFAULT_TINT
    }
}

pub(crate) fn set_glass_tint(tint: f32, cx: &mut App) {
    let tint = appearance::clamp_tint(tint);
    if (glass_tint(cx) - tint).abs() < f32::EPSILON {
        return;
    }
    cx.global::<AppState>()
        .notebook
        .borrow_mut()
        .stage_glass_tint(tint);
    let windows: Vec<_> = cx.global::<AppState>().windows.values().copied().collect();
    for handle in windows {
        let _ = handle.update(cx, |note, _, cx| {
            if note.record.style.frosted() {
                cx.notify();
            }
        });
    }
    if let Some(control) = cx.global::<AppState>().transparency_window {
        let _ = control.update(cx, |control, window, cx| control.sync(tint, window, cx));
    }
    cx.global_mut::<AppState>().appearance_save = Some(cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(400))
            .await;
        cx.update(|cx| {
            let result = cx.global::<AppState>().notebook.borrow_mut().save_index();
            if let Err(error) = result {
                notify(
                    cx,
                    copy::failed("Couldn’t save transparency.", &error),
                );
            }
        });
    }));
    update_menus(cx);
}
/// The active note window, if the app still owns it.
/// Restyle every open note for the system's current accessibility and colour settings, and
/// redraw the panels, which read their colours as they render.
pub(crate) fn refresh_appearance(cx: &mut App) {
    let handles: Vec<_> = cx.global::<AppState>().windows.values().copied().collect();
    for handle in handles {
        let _ = handle.update(cx, |note, window, cx| {
            note.refresh_system_appearance(window, cx)
        });
    }
    cx.refresh_windows();
}
/// Keep Find, Settings and Recovery above Float on Top notes while any note floats.
pub(crate) fn sync_panel_levels(cx: &mut App) {
    let state = cx.global::<AppState>();
    let notes_float = state
        .notebook
        .borrow()
        .notes()
        .iter()
        .any(|note| note.open && note.floating);
    let panels = [
        state.find_window.map(AnyWindowHandle::from),
        state.transparency_window.map(AnyWindowHandle::from),
        state.recovery_window.map(AnyWindowHandle::from),
    ];
    for panel in panels.into_iter().flatten() {
        let _ = panel.update(cx, |_, window, _| platform::set_panel_level(window, notes_float));
    }
}
pub(crate) fn active_window(cx: &App) -> Option<WindowHandle<Note>> {
    let state = cx.global::<AppState>();
    state
        .active
        .as_ref()
        .and_then(|id| state.windows.get(id))
        .copied()
}
const SPELLING_SETTINGS_FAILED: &str = "Couldn’t save the spelling and grammar settings.";
pub(crate) fn propagate_checking(except: &str, checking: checking::Checking, cx: &mut App) {
    let state = cx.global::<AppState>();
    let notebook = state.notebook.clone();
    let handles: Vec<_> = state
        .windows
        .iter()
        .filter(|(id, _)| id.as_str() != except)
        .map(|(id, handle)| (id.clone(), *handle))
        .collect();
    let records: Vec<_> = handles
        .iter()
        .filter_map(|(id, _)| {
            let mut record = notebook.borrow().note(id)?;
            record.style.checking = checking;
            Some(record)
        })
        .collect();
    let pending = notebook.borrow_mut().request_update_many(records);
    match pending {
        Err(error) => notify(cx, copy::failed(SPELLING_SETTINGS_FAILED, &error)),
        Ok(pending) => cx
            .spawn(async move |cx| {
                let completed = pending.wait().await;
                cx.update(|cx| {
                    if let Err(error) = notebook.borrow_mut().complete(completed) {
                        notify(cx, copy::failed(SPELLING_SETTINGS_FAILED, &error));
                        return;
                    }
                    for (_, handle) in handles {
                        let _ = handle.update(cx, |note, _, cx| {
                            note.record.style.checking = checking;
                            note.checking_task =
                                Some(note.checker.refresh(&note.editor, checking, cx));
                            cx.notify();
                        });
                    }
                    update_menus(cx);
                });
            })
            .detach(),
    }
}

pub(crate) fn open_note(id: &str, cx: &mut App) -> Result<(), String> {
    if cx.global::<AppState>().quitting {
        return Err("Dahlia is quitting.".into());
    }
    if cx.global::<AppState>().opening_notes.contains(id) {
        return Ok(());
    }
    if let Some(window) = cx.global::<AppState>().windows.get(id).copied() {
        window
            .update(cx, |_, window, _| window.activate_window())
            .map_err(|error| {
                log::warn!("Cannot activate a note window: {error}");
                "Couldn’t bring this note to the front.".to_owned()
            })?;
        return Ok(());
    }
    let library = cx.global::<AppState>().notebook.clone();
    let record = library
        .borrow()
        .note(id)
        .ok_or("This note is no longer available.")?
        .restored(&displays(cx));
    if record.state != NoteState::Active {
        return Err(match record.state {
            NoteState::Trash => "Restore this note from File ▸ Trash first.",
            _ => "Restore this note from File ▸ Archived Notes first.",
        }
        .into());
    }
    let opened = library.borrow().open_store(id);
    let (store, text) = match opened {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            archive_missing(id, library, cx);
            return Ok(());
        }
        Err(error) => return Err(copy::plain(&error)),
    };
    if record.source_base.is_some() {
        cx.global_mut::<AppState>()
            .opening_notes
            .insert(id.to_owned());
        open_linked(record, store, text, library, cx);
        return Ok(());
    }
    open_prepared(record, store, text, None, library, cx)
}
/// A note whose file is gone cannot open, and retrying at every launch helps nobody: close
/// it and list it under Archived Notes, where Recovery can forget it or find any copy.
fn archive_missing(id: &str, library: Rc<RefCell<Notebook>>, cx: &mut App) {
    let id = id.to_owned();
    cx.spawn(async move |cx| {
        let mut attempts = 0;
        let result = loop {
            let request = {
                let note = library.borrow().note(&id);
                match note {
                    Some(note) => library.borrow_mut().request_transition(
                        note,
                        NoteState::Active,
                        NoteState::Archived,
                    ),
                    None => return,
                }
            };
            let result = match request {
                Ok(pending) => {
                    let completed = pending.wait().await;
                    library.borrow_mut().complete(completed)
                }
                Err(error) => Err(error),
            };
            // Another change may still be saving, as when several notes are missing at launch.
            match result {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && attempts < 50 => {
                    attempts += 1;
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                }
                result => break result,
            }
        };
        cx.update(|cx| {
            match result {
                Ok(_) => notify(
                    cx,
                    "This note’s file is missing, so it was moved to Archived Notes.",
                ),
                Err(error) => notify(
                    cx,
                    copy::failed(
                        "This note’s file is missing, and Dahlia couldn’t move it to Archived Notes.",
                        &error,
                    ),
                ),
            }
            update_menus(cx);
            if let Some(handle) = cx.global::<AppState>().recovery_window {
                let _ = handle.update(cx, |panel, _, cx| panel.refresh(cx));
            }
        });
    })
    .detach();
}
pub(crate) fn open_linked(
    mut record: NoteRecord,
    store: NoteStore,
    text: String,
    library: Rc<RefCell<Notebook>>,
    cx: &mut App,
) {
    let id = record.id.clone();
    cx.spawn(async move |cx| {
        let result: Result<_, String> = async {
            let base = record.source_base.clone().ok_or("Couldn’t find the folder this note’s images are linked to.")?;
            let (store, text, managed) = cx.background_executor().spawn(async move {
                match attachments::managed_source(&text, &base) {
                    Ok((managed, images)) => {
                        attachments::publish(store.directory(), &images)?;
                        Ok::<_, std::io::Error>((store, text, Ok(managed)))
                    }
                    Err(error) => Ok((store, text, Err(error))),
                }
            }).await.map_err(|error| copy::plain(&error))?;
            let managed = match managed {
                Ok(managed) => managed,
                Err(error) => return Ok((record, store, text, Some(NoteError::other(copy::failed(
                    "Couldn’t copy this note’s images into the note, so it still uses the originals in their folder.", &error
                ))))),
            };
            let linked = record.clone();
            record.source_base = None;
            let pending = library.borrow_mut().request_update_many(vec![record.clone()]).map_err(|error| copy::plain(&error))?;
            let completed = pending.wait().await;
            library.borrow_mut().complete(completed).map_err(|error| copy::plain(&error))?;
            // Keep record-before-source ordering without blocking the UI thread.
            let written = cx.background_executor().spawn(async move {
                let mut store = store;
                let result = store.save(&managed);
                (store, managed, result)
            }).await;
            let (store, managed, result) = written;
            if let Err(error) = result {
                let rollback = library.borrow_mut().request_update_many(vec![linked]);
                let rollback = match rollback {
                    Ok(pending) => { let completed = pending.wait().await; library.borrow_mut().complete(completed) }
                    Err(error) => Err(error),
                };
                return Err(match rollback {
                    Ok(()) => copy::failed("Couldn’t copy this note’s images into the note.", &error),
                    Err(rollback) => {
                        log::warn!("Linked folder not restored: {rollback}");
                        copy::failed("Couldn’t copy this note’s images into the note or restore its link to their folder.", &error)
                    },
                });
            }
            Ok((record, store, managed, None))
        }.await;
        cx.update(|cx| {
            cx.global_mut::<AppState>().opening_notes.remove(&id);
            match result {
                Ok((record, store, text, problem)) => {
                    if let Err(error) = open_prepared(record, store, text, problem, library, cx) { notify(cx, error); }
                }
                Err(error) => notify(cx, error),
            }
        });
    }).detach();
}
pub(crate) fn open_prepared(
    mut record: NoteRecord,
    store: NoteStore,
    text: String,
    upgrade_problem: Option<NoteError>,
    library: Rc<RefCell<Notebook>>,
    cx: &mut App,
) -> Result<(), String> {
    let id = record.id.clone();
    let schema = commonmark_schema();
    // A large note's window opens at once, on an empty stand-in; its source is read in
    // the background.
    let opening = text.len() > BACKGROUND_PARSE_BYTES;
    let parsed = parse_note(&schema, if opening { "" } else { &text });
    let source = match parsed {
        Ok(source) => source,
        Err(error) => {
            // Leave it closed so it does not fail again on every launch.
            if record.open {
                record.open = false;
                let _ = library.borrow_mut().update(record);
            }
            return Err(error);
        }
    };
    record.open = true;
    library
        .borrow_mut()
        .update(record.clone())
        .map_err(|error| copy::plain(&error))?;
    let note_id = id.to_owned();
    let collapsed = record.collapsed;
    let floating = record.floating;
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds_of(record.frame))),
                titlebar: None,
                // GPUI 0.3.6 resolves app-menu targets through its NSWindow subclass,
                // excluding the NSPanel used for Floating. Our adapter sets the level.
                kind: WindowKind::Normal,
                app_owns_titlebar_drag: true,
                window_min_size: Some(size(px(230.), px(HEIGHT))),
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            },
            move |window, cx| {
                platform::configure(window, collapsed, floating, record.style.frosted(), None);
                let note = cx.new(|cx| {
                    if opening {
                        Note::opening(store, source, record, Some(library), cx)
                    } else {
                        Note::new_record(store, source, record, Some(library), cx)
                    }
                });
                note.update(cx, |note, cx| {
                    note.error = upgrade_problem;
                    note.attach(window, cx)
                });
                if !collapsed {
                    window.focus(&note.read(cx).editor.read(cx).focus_handle(cx), cx);
                } else {
                    let focus = note.read(cx).shell_focus.clone();
                    window.focus(&focus, cx);
                }
                let weak = note.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    let _ = weak.update(cx, |note, cx| note.close(window, cx));
                    false
                });
                note
            },
        )
        .map_err(|error| {
            log::warn!("Cannot open a note window: {error}");
            "Couldn’t open this note’s window.".to_owned()
        })?;
    if opening {
        cx.spawn(async move |cx| {
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    let source = parse_note(&schema, &text)?;
                    let prepared = editor_setup(&schema, &source).prepare();
                    Ok::<_, String>((source, prepared))
                })
                .await;
            let _ = window.update(cx, |note, window, cx| match parsed {
                Ok((source, prepared)) => {
                    note.finish_opening_prepared(source, prepared, window, cx)
                }
                Err(error) => note.fail_opening(error, window, cx),
            });
        })
        .detach();
    }
    cx.global_mut::<AppState>()
        .windows
        .insert(note_id.clone(), window);
    cx.global_mut::<AppState>().active = Some(note_id);
    update_menus(cx);
    Ok(())
}
pub(crate) fn create_note(cx: &mut App) {
    if cx.global::<AppState>().quitting {
        return;
    }
    let library = cx.global::<AppState>().notebook.clone();
    let mut frame = cx
        .global::<AppState>()
        .active
        .as_ref()
        .and_then(|id| library.borrow().note(id))
        .map(|n| n.frame)
        .unwrap_or_default();
    frame.width = library.borrow().defaults().width;
    frame.height = library.borrow().defaults().height;
    if !cx.global::<AppState>().windows.is_empty() {
        let taken: Vec<_> = library
            .borrow()
            .notes()
            .iter()
            .filter(|note| note.open && note.state == NoteState::Active)
            .map(|note| note.frame)
            .collect();
        frame = frame.cascaded(&displays(cx), &taken);
    }
    frame = frame.restored(&displays(cx), false);
    let pending = library.borrow_mut().request_create(frame);
    match pending {
        Ok(pending) => cx
            .spawn(async move |cx| {
                let completed = pending.wait().await;
                cx.update(|cx| match library.borrow_mut().complete(completed) {
                    Ok(note) => {
                        cx.defer(move |cx| {
                            if let Err(error) = open_note(&note.id, cx) {
                                notify(cx, error);
                            }
                        });
                    }
                    Err(error) => notify(cx, copy::failed("Couldn’t create a note.", &error)),
                });
            })
            .detach(),
        Err(error) => notify(cx, copy::failed("Couldn’t create a note.", &error)),
    }
}
/// How a system request to terminate is answered. `Cancel` aborts a logout, restart or
/// shutdown outright, so it is only for a quit that cannot start.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Termination {
    Cancel,
    Now,
    Later,
}
/// What a finished quit does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuitEnd {
    /// Menu Quit: nobody is waiting, so quit now.
    Quit,
    /// The system is waiting: tell it whether to terminate.
    Reply(bool),
    /// A failed menu Quit: stay open.
    Stay,
}
/// Answer the system's request to terminate. Until the save is acknowledged the answer is
/// `Later`, and `finish_quit` replies when the save ends. `begin_quit` starts the save, or
/// notes that one is under way, and says whether anything will end up replying.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn answer_termination(
    ready: bool,
    awaiting: &std::cell::Cell<bool>,
    begin_quit: impl FnOnce() -> bool,
) -> Termination {
    if ready {
        Termination::Now
    } else if begin_quit() {
        awaiting.set(true);
        Termination::Later
    } else {
        Termination::Cancel
    }
}
/// What to do now that the quit has saved (`saved`) or failed. A pending system request is
/// answered exactly once, so a menu Quit running alongside it cannot double-reply.
pub(crate) fn finish_quit(awaiting: &std::cell::Cell<bool>, saved: bool) -> QuitEnd {
    if awaiting.replace(false) {
        QuitEnd::Reply(saved)
    } else if saved {
        QuitEnd::Quit
    } else {
        QuitEnd::Stay
    }
}
pub(crate) fn quit_all(cx: &mut App) {
    begin_quit(cx);
}
/// Save everything and then quit. False when it could not start; true when it is under way,
/// whether this call started it or an earlier one did.
fn begin_quit(cx: &mut App) -> bool {
    if cx.global::<AppState>().quitting {
        return true;
    }
    let windows: Vec<_> = cx.global::<AppState>().windows.values().copied().collect();
    if windows.iter().any(|handle| {
        handle
            .read_with(cx, |note, _| note.close_task.is_some() || note.opening)
            .unwrap_or(true)
    }) || !cx.global::<AppState>().opening_notes.is_empty()
    {
        notify(
            cx,
            "A note is still opening or closing. Quit again when it finishes.",
        );
        return false;
    }
    cx.global_mut::<AppState>().quitting = true;
    let mut saves = Vec::new();
    let mut capture_failure = None;
    for handle in &windows {
        match handle.update(cx, |note, window, cx| {
            note.capture(window);
            note.save_task.take();
            note.durability_lock
                .store(true, std::sync::atomic::Ordering::Release);
            let placement = note.persist_record().map_err(|error| {
                copy::failed("Couldn’t save this note’s size and position.", &error)
            });
            (note.request_save(cx), placement)
        }) {
            Ok((pending, placement)) => {
                saves.push(pending);
                if let Err(error) = placement {
                    capture_failure = Some(error);
                }
            }
            Err(error) => {
                log::warn!("Cannot read a note window while quitting: {error}");
                capture_failure = Some("Couldn’t read one of the open notes.".to_owned());
            }
        }
    }
    cx.spawn(async move |cx| {
        let mut failure = capture_failure;
        for saved in saves {
            if let Err(error) = saved.await {
                failure = Some(error.message);
            }
        }
        if failure.is_none() {
            let pending = cx.update(|cx| {
                cx.global::<AppState>()
                    .notebook
                    .borrow_mut()
                    .request_flush()
            });
            match pending {
                Ok(pending) => {
                    let completed = pending.wait().await;
                    if let Err(error) = cx.update(|cx| {
                        cx.global::<AppState>()
                            .notebook
                            .borrow_mut()
                            .complete(completed)
                    }) {
                        failure = Some(copy::plain(&error));
                    }
                }
                Err(error) => failure = Some(copy::plain(&error)),
            }
        }
        let end = cx.update(|cx| {
            let saved = failure.is_none();
            if let Some(error) = failure {
                cx.global_mut::<AppState>().quitting = false;
                for handle in windows {
                    let _ = handle.update(cx, |note, _, cx| {
                        note.durability_lock
                            .store(false, std::sync::atomic::Ordering::Release);
                        cx.notify();
                    });
                }
                notify(
                    cx,
                    format!("Dahlia couldn’t save its notes, so it didn’t quit. {error} Try quitting again."),
                );
            } else {
                cx.global::<AppState>().ready_to_quit.set(true);
            }
            let awaiting = cx.global::<AppState>().termination_awaiting.clone();
            let end = finish_quit(&awaiting, saved);
            if end == QuitEnd::Quit {
                cx.quit();
            }
            end
        });
        // Replying quits straight away, running the quit callbacks, so it happens outside
        // the update above.
        #[cfg(all(target_os = "macos", not(test)))]
        if let QuitEnd::Reply(terminate) = end {
            gpui_macos::reply_to_termination(
                objc2::MainThreadMarker::new().expect("AppKit runs on the main thread"),
                terminate,
            );
        }
        #[cfg(any(not(target_os = "macos"), test))]
        let _ = end;
    })
    .detach();
    true
}
/// The notebook could not be opened: say so in an alert, then quit.
fn launch_failure(cx: &mut App, error: &std::io::Error) {
    log::error!("Cannot open the notebook: {error}");
    cx.activate(true);
    let dismissed = notify::app_alert(&format!(
        "Dahlia can’t open your notes.\n\n{}",
        copy::plain(error)
    ));
    cx.spawn(async move |cx| {
        let _ = dismissed.await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}
pub(crate) fn run() {
    let _ = log::set_logger(&Diagnostics);
    log::set_max_level(log::LevelFilter::Warn);
    let mut notebook = match platform::data_directory().and_then(Notebook::open) {
        Ok(book) => book,
        Err(error) => {
            gpui_platform::application().run(move |cx| launch_failure(cx, &error));
            std::process::exit(1);
        }
    };
    let application = gpui_platform::application();
    application.on_reopen(|cx| {
        if cx.global::<AppState>().windows.is_empty() {
            let id = cx
                .global::<AppState>()
                .notebook
                .borrow()
                .notes()
                .iter()
                .find(|n| n.state == NoteState::Active)
                .map(|n| n.id.clone());
            if let Some(id) = id {
                if let Err(error) = open_note(&id, cx) {
                    notify(cx, error);
                }
            } else {
                create_note(cx);
            }
        }
    });
    application.run(move |cx| {
        gpui_base::init(cx);
        if let Err(error) = cx.text_system().add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/PaperMono-Regular.otf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/PaperMono-Bold.otf")),
        ]) {
            log::warn!(
                "Could not register bundled Paper Mono; using an available monospace face: {error}"
            );
        }
        let fonts = Fonts::available(&cx.text_system().all_font_names());
        log::info!("Prose font: {}; code font: {}", fonts.prose, fonts.code);
        let problems = notebook.take_problems();
        cx.set_global(AppState::new(Rc::new(RefCell::new(notebook)), fonts));
        // Reduce Transparency, Increase Contrast and the accent colour change for every window
        // at once, so restyle every open note and panel, not just the one that becomes key.
        let mut changes = platform::appearance_changes();
        cx.spawn(async move |cx| {
            while futures::StreamExt::next(&mut changes).await.is_some() {
                cx.update(refresh_appearance);
            }
        })
        .detach();
        #[cfg(target_os = "macos")]
        {
            let app = cx.to_async();
            let ready = cx.global::<AppState>().ready_to_quit.clone();
            let awaiting = cx.global::<AppState>().termination_awaiting.clone();
            gpui_macos::set_termination_guard(
                objc2::MainThreadMarker::new().expect("AppKit starts on the main thread"),
                move || {
                    match answer_termination(ready.get(), &awaiting, || app.update(begin_quit)) {
                        Termination::Cancel => gpui_macos::TerminationReply::Cancel,
                        Termination::Now => gpui_macos::TerminationReply::Now,
                        Termination::Later => gpui_macos::TerminationReply::Later,
                    }
                },
            );
        }
        // A system logout, restart or shutdown is answered Later and replied to when the
        // save ends, because Cancel would abort the whole operation. Forced termination
        // skips the guard; this callback still drains source and metadata I/O best-effort.
        cx.on_app_quit(|cx| {
            let library = cx.global::<AppState>().notebook.clone();
            let handles: Vec<_> = cx.global::<AppState>().windows.values().copied().collect();
            let mut saves = Vec::new();
            for handle in handles {
                if let Ok(pending) = handle.update(cx, |note, window, cx| {
                    note.capture(window);
                    let _ = note.persist_record();
                    (note.request_save(cx), note.rescue_unsaved(cx))
                }) {
                    saves.push(pending);
                }
            }
            async move {
                let mut saved = true;
                for (pending, rescue) in saves {
                    if let Err(error) = pending.await {
                        saved = false;
                        log::error!("{}", error.message);
                        rescue();
                    }
                }
                if saved {
                    let requested = library.borrow_mut().request_flush();
                    let result = match requested {
                        Ok(pending) => {
                            let completed = pending.wait().await;
                            library.borrow_mut().complete(completed)
                        }
                        Err(error) => Err(error),
                    };
                    if let Err(error) = result {
                        log::error!("System quit could not save notebook metadata: {error}");
                    }
                }
            }
        })
        .detach();
        // Metadata writes and checkpoints run on their own thread and report here.
        if let Some(mut problems) = problems {
            cx.spawn(async move |cx| {
                while let Some(problem) = futures::StreamExt::next(&mut problems).await {
                    cx.update(|cx| notify(cx, problem));
                }
            })
            .detach();
        }
        // Before the first menus are set, so "Check for Updates…" starts out right.
        updates::start();
        keys::bind(cx);
        #[cfg(not(target_os = "macos"))]
        prompt::install(cx);
        let ids: Vec<_> = cx
            .global::<AppState>()
            .notebook
            .borrow()
            .notes()
            .iter()
            .filter(|n| n.open && n.state == NoteState::Active)
            .map(|n| n.id.clone())
            .collect();
        if cx.global::<AppState>().notebook.borrow().notes().is_empty() {
            create_note(cx);
        } else {
            for id in ids {
                if let Err(error) = open_note(&id, cx) {
                    notify(cx, error);
                }
            }
        }
        update_menus(cx);
        if cx
            .global::<AppState>()
            .notebook
            .borrow()
            .recovery_notice()
            .is_some()
        {
            recovery::show(cx);
        }
        cx.activate(true);
    });
}
#[cfg(test)]
pub(crate) fn settle_io(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking(); // Native writer completions are deliberately nondeterministic.
    for _ in 0..12 {
        cx.run_until_parked();
        let Some((library, handles)) = cx.update(|cx| {
            cx.has_global::<AppState>().then(|| {
                (
                    cx.global::<AppState>().notebook.clone(),
                    cx.global::<AppState>()
                        .windows
                        .values()
                        .copied()
                        .collect::<Vec<_>>(),
                )
            })
        }) else {
            return;
        };
        library.borrow().barrier();
        for handle in handles {
            let _ = handle.read_with(cx, |note, _| note.writer.barrier());
        }
        cx.run_until_parked();
    }
}

#[cfg(test)]
mod tests;
