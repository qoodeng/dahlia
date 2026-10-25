use super::{Appearance, ColorIndex, Note, NoteStore};
use gpui::TestAppContext;
use markraft_core::{commands, history};
use std::time::Duration;
/// A closed note from the linked-source prototype whose image lives in `folder`.
fn linked_source_note(
    _label: &str,
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    std::rc::Rc<std::cell::RefCell<super::Notebook>>,
    super::NoteRecord,
) {
    use super::*;
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let folder = root.join("linked");
    std::fs::create_dir_all(&folder).unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    std::fs::write(folder.join("a.png"), png.get_ref()).unwrap();
    let mut record = library.borrow_mut().create(Frame::default()).unwrap();
    let (mut store, _) = library.borrow().open_store(&record.id).unwrap();
    store.save("![a](a.png)\n").unwrap();
    drop(store);
    record.open = false;
    record.source_base = Some(folder);
    library.borrow_mut().update_durable(record.clone()).unwrap();
    (_root_fixture, library, record)
}
#[gpui::test]
fn linked_source_upgrade_commits_the_record_before_rewriting_the_source(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, library, record) = linked_source_note("upgrade-order", cx);
    let root = _fixture.path();
    let source = root.join("notes").join(&record.id).join("note.md");
    // The record cannot be committed (its index cannot be read), so the source must stay as it was.
    let index = root.join("index.json");
    let held = root.join("index.held");
    std::fs::rename(&index, &held).unwrap();
    std::fs::create_dir(&index).unwrap();
    cx.update(|cx| open_note(&record.id, cx)).unwrap();
    super::settle_io(cx);
    assert!(!cx.update(|cx| cx.global::<AppState>().windows.contains_key(&record.id)));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "![a](a.png)\n");
    // With metadata writable the upgrade completes: managed image, no linked folder.
    std::fs::remove_dir(&index).unwrap();
    std::fs::rename(&held, &index).unwrap();
    cx.update(|cx| open_note(&record.id, cx)).unwrap();
    super::settle_io(cx);
    let upgraded = std::fs::read_to_string(&source).unwrap();
    assert!(upgraded.contains("attachments/"), "{upgraded}");
    assert_eq!(library.borrow().note(&record.id).unwrap().source_base, None);
}
#[gpui::test]
fn menus_rebuild_only_when_what_they_show_changes_and_cap_the_window_list(cx: &mut TestAppContext) {
    use super::*;
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
    let sources = (0..WINDOW_MENU_NOTES + 20)
        .map(|n| transfer::Imported {
            text: format!("Listed {n}\n"),
            title: format!("Listed {n}"),
            linked_files: false,
            images: BTreeMap::new(),
            disclosures: Vec::new(),
        })
        .collect();
    let notes = library
        .borrow_mut()
        .import_sources(sources, Frame::default())
        .unwrap();
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let listed = |cx: &mut TestAppContext| {
        cx.update(|cx| {
                let menus = cx.get_menus().unwrap();
                let window = menus.iter().find(|menu| menu.name == "Window").unwrap();
                window
                    .items
                    .iter()
                    .filter(|item| {
                        matches!(item, OwnedMenuItem::Action { name, .. } if name.starts_with("Listed"))
                    })
                    .count()
            })
    };
    cx.update(update_menus);
    assert_eq!(listed(cx), WINDOW_MENU_NOTES);
    // An autosave that changes nothing the menus show does not rebuild them.
    cx.update(update_menus);
    assert_eq!(cx.update(|cx| cx.global::<AppState>().menu_builds), 1);
    let mut renamed = notes[0].clone();
    renamed.title = "Renamed".into();
    library.borrow_mut().update(renamed).unwrap();
    cx.update(update_menus);
    assert_eq!(cx.update(|cx| cx.global::<AppState>().menu_builds), 2);
    drop(library);
}
#[gpui::test]
fn large_notes_open_their_window_first_and_never_save_the_empty_stand_in(cx: &mut TestAppContext) {
    use super::*;
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let record = library.borrow_mut().create(Frame::default()).unwrap();
    let text = "Large note body\n\n".repeat(BACKGROUND_PARSE_BYTES / 16 + 1);
    let (mut store, _) = library.borrow().open_store(&record.id).unwrap();
    store.save(&text).unwrap();
    drop(store);
    cx.update(|cx| open_note(&record.id, cx)).unwrap();
    let window = cx
        .update(|cx| cx.global::<AppState>().windows.get(&record.id).copied())
        .unwrap();
    // The window is open before the source has been read.
    window
        .update(cx, |note, _, cx| {
            assert!(note.opening);
            assert!(note.source_text(cx).is_err());
            // Typing into the empty stand-in is refused; saving leaves the file alone.
            let edit = commands::insert_text("lost")(note.editor.read(cx).state()).unwrap();
            assert!(
                !note
                    .editor
                    .update(cx, |editor, cx| editor.dispatch([edit], cx))
            );
            assert!(note.save(cx));
        })
        .unwrap();
    let path = root.join("notes").join(&record.id).join("note.md");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    super::settle_io(cx);
    window
        .update(cx, |note, _, cx| {
            assert!(!note.opening);
            assert_eq!(note.source_text(cx).unwrap(), text);
            assert_eq!(note.record.title, "Large note body");
        })
        .unwrap();
    drop(library);
}
/// A library with one large note whose window is open but still parsing in the background.
fn opening_large_note(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    std::rc::Rc<std::cell::RefCell<super::Notebook>>,
    gpui::WindowHandle<Note>,
) {
    use super::*;
    let fixture = tempfile::tempdir().unwrap();
    let library = Rc::new(RefCell::new(
        Notebook::open(fixture.path().to_path_buf()).unwrap(),
    ));
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let record = library.borrow_mut().create(Frame::default()).unwrap();
    let (mut store, _) = library.borrow().open_store(&record.id).unwrap();
    store
        .save(&"Large note body\n\n".repeat(BACKGROUND_PARSE_BYTES / 16 + 1))
        .unwrap();
    drop(store);
    cx.update(|cx| open_note(&record.id, cx)).unwrap();
    let window = cx
        .update(|cx| cx.global::<AppState>().windows.get(&record.id).copied())
        .unwrap();
    (fixture, library, window)
}
#[gpui::test]
fn closing_or_exporting_a_note_that_is_still_opening_is_refused(cx: &mut TestAppContext) {
    let (_fixture, _library, window) = opening_large_note(cx);
    for export in [false, true] {
        window
            .update(cx, |note, window, cx| {
                assert!(note.opening);
                note.error = None;
                if export {
                    note.export(false, window, cx);
                } else {
                    note.close(window, cx);
                }
                // No prompt or file picker is waiting, and the user is told why.
                assert!(note.close_task.is_none() && note.transfer_task.is_none());
                assert_eq!(
                    note.error.as_ref().map(|error| error.message.as_str()),
                    Some("This note is still opening.")
                );
            })
            .unwrap();
    }
    super::settle_io(cx);
}
#[gpui::test]
fn a_note_whose_file_is_missing_is_archived_once_instead_of_failing_every_launch(
    cx: &mut TestAppContext,
) {
    use super::*;
    let fixture = tempfile::tempdir().unwrap();
    let library = Rc::new(RefCell::new(
        Notebook::open(fixture.path().to_owned()).unwrap(),
    ));
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let ghost = library.borrow_mut().create(Frame::default()).unwrap();
    let kept = library.borrow_mut().create(Frame::default()).unwrap();
    std::fs::remove_file(fixture.path().join("notes").join(&ghost.id).join("note.md")).unwrap();
    crate::notify::take_alerts();
    cx.update(|cx| open_note(&ghost.id, cx)).unwrap();
    super::settle_io(cx);
    let record = library.borrow().note(&ghost.id).unwrap();
    assert_eq!(record.state, NoteState::Archived);
    assert!(!record.open);
    assert!(!cx.update(|cx| cx.global::<AppState>().windows.contains_key(&ghost.id)));
    let alerts = crate::notify::take_alerts();
    assert!(
        alerts
            .iter()
            .any(|message| message.contains("file is missing, so it was moved to Archived Notes")),
        "{alerts:?}"
    );
    // Other notes are untouched, and the change is on disk so the next launch skips the ghost.
    assert_eq!(library.borrow().note(&kept.id).unwrap().state, NoteState::Active);
    library.borrow_mut().flush().unwrap();
    let disk: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.path().join("index.json")).unwrap(),
    )
    .unwrap();
    let saved = disk["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|note| note["id"] == ghost.id.as_str())
        .unwrap();
    assert_eq!(saved["state"], "archived");
    assert_eq!(saved["open"], false);
}
#[gpui::test]
fn failed_linked_source_upgrade_is_reported_not_swallowed(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, library, record) = linked_source_note("upgrade-error", cx);
    let root = _fixture.path();
    std::fs::remove_dir_all(root.join("linked")).unwrap();
    cx.update(|cx| open_note(&record.id, cx)).unwrap();
    super::settle_io(cx);
    let window = cx
        .update(|cx| cx.global::<AppState>().windows.get(&record.id).copied())
        .unwrap();
    let error = window.read_with(cx, |note, _| note.error.clone()).unwrap();
    assert!(
        error.is_some_and(|error| error.message.contains("Couldn’t copy this note’s images")),
        "the note says why its images were not copied"
    );
    assert!(
        library
            .borrow()
            .note(&record.id)
            .unwrap()
            .source_base
            .is_some()
    );
}
/// A library with two notes open in test windows; the second is active.
fn two_open_notes(
    _label: &str,
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    std::rc::Rc<std::cell::RefCell<super::Notebook>>,
    Vec<gpui::WindowHandle<Note>>,
) {
    use super::*;
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let library = Rc::new(RefCell::new(Notebook::open(root).unwrap()));
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let mut windows = Vec::new();
    for _ in 0..2 {
        let record = library.borrow_mut().create(Frame::default()).unwrap();
        let (store, text) = library.borrow().open_store(&record.id).unwrap();
        let id = record.id.clone();
        let window = cx.add_window(|_, cx| {
            Note::new_record(
                store,
                parse_note(&commonmark_schema(), &text).unwrap(),
                record,
                Some(library.clone()),
                cx,
            )
        });
        cx.update(|cx| {
            let state = cx.global_mut::<AppState>();
            state.windows.insert(id.clone(), window);
            state.active = Some(id);
        });
        windows.push(window);
    }
    (_root_fixture, library, windows)
}
#[gpui::test]
fn collapse_menu_label_follows_each_note_transition(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, windows) = two_open_notes("collapse-menu", cx);
    let label = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let menus = cx.get_menus().unwrap();
            let menu = menus.iter().find(|menu| menu.name == "Window").unwrap();
            let OwnedMenuItem::Action { name, .. } = &menu.items[0] else {
                panic!("Window must start with the note's collapse/expand action");
            };
            name.to_string()
        })
    };
    cx.update(update_menus);
    assert_eq!(label(cx), "Collapse");
    for expected in ["Expand", "Collapse", "Expand", "Collapse"] {
        windows[1]
            .update(cx, |note, window, cx| note.collapse(window, cx))
            .unwrap();
        assert_eq!(
            windows[1]
                .read_with(cx, |note, _| note.record.collapsed)
                .unwrap(),
            expected == "Expand"
        );
        assert_eq!(label(cx), expected);
    }
}
#[gpui::test]
fn close_keeps_window_and_edits_until_real_writer_and_metadata_acknowledge(
    cx: &mut TestAppContext,
) {
    use super::*;
    let (_fixture, library, windows) = two_open_notes("async-close", cx);
    let window = windows[0];
    let id = window
        .read_with(cx, |note, _| note.record.id.clone())
        .unwrap();
    window
        .update(cx, |note, _, cx| {
            note.editor.update(cx, |editor, cx| {
                assert!(editor.dispatch(
                    [commands::insert_text("retained")(editor.state()).unwrap()],
                    cx
                ));
            });
        })
        .unwrap();
    let (entered, waiting) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let pending = window
        .read_with(cx, |note, _| {
            note.writer.save_with(move || {
                entered.send(()).unwrap();
                gate.recv().unwrap();
                Ok("retained".into())
            })
        })
        .unwrap();
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    // This handler must return while the actual writer is still blocked above.
    window
        .update(cx, |note, window, cx| {
            note.finish_close(NoteState::Archived, window, cx)
        })
        .unwrap();
    assert!(cx.update(|cx| cx.global::<AppState>().windows.contains_key(&id)));
    assert_eq!(library.borrow().note(&id).unwrap().state, NoteState::Active);
    window
        .update(cx, |note, _, cx| {
            assert!(note.close_task.is_some());
            assert_eq!(note.source_text(cx).unwrap(), "retained");
            note.editor.update(cx, |editor, cx| {
                let edit = commands::insert_text("rejected while closing")(editor.state()).unwrap();
                assert!(!editor.dispatch([edit], cx));
            });
        })
        .unwrap();
    release.send(()).unwrap();
    futures::executor::block_on(pending).unwrap().unwrap();
    super::settle_io(cx);
    assert_eq!(
        library.borrow().note(&id).unwrap().state,
        NoteState::Archived
    );
    assert!(!cx.update(|cx| cx.global::<AppState>().windows.contains_key(&id)));
    assert_eq!(library.borrow().saved_source(&id).unwrap().0, "retained");
}
#[gpui::test]
fn notify_targets_the_active_note_and_otherwise_alerts(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, windows) = two_open_notes("notify", cx);
    notify::take_alerts();
    cx.update(|cx| notify(cx, "Something failed."));
    let errors: Vec<_> = windows
        .iter()
        .map(|window| window.read_with(cx, |note, _| note.error.clone()).unwrap())
        .collect();
    assert_eq!(errors[0], None, "never an arbitrary other window");
    assert_eq!(errors[1], Some(NoteError::other("Something failed.")));
    // With no open note window, the app alerts instead of writing only to stderr.
    cx.update(|cx| cx.global_mut::<AppState>().windows.clear());
    cx.update(|cx| notify(cx, "Nothing is open."));
    assert_eq!(notify::take_alerts(), vec!["Nothing is open.".to_owned()]);
}
#[gpui::test]
fn a_note_error_is_announced_once_and_again_after_it_clears(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, windows) = two_open_notes("announce", cx);
    let window = windows[1];
    cx.run_until_parked();
    platform::take_announcements();
    let show = |cx: &mut TestAppContext, error: Option<NoteError>| {
        window.update(cx, |note, _, cx| {
            note.error = error;
            cx.notify();
        })
        .unwrap();
        cx.run_until_parked();
    };
    show(cx, Some(NoteError::save("Couldn’t save this note.")));
    // Drawn again with the same message: still one announcement.
    show(cx, Some(NoteError::save("Couldn’t save this note.")));
    assert_eq!(
        platform::take_announcements(),
        vec!["Couldn’t save this note.".to_owned()]
    );
    // A different message is a new announcement; clearing and repeating is too.
    show(cx, Some(NoteError::other("Couldn’t open the image picker.")));
    show(cx, None);
    show(cx, Some(NoteError::other("Couldn’t open the image picker.")));
    assert_eq!(
        platform::take_announcements(),
        vec![
            "Couldn’t open the image picker.".to_owned(),
            "Couldn’t open the image picker.".to_owned()
        ]
    );
}
#[gpui::test]
fn a_collapsed_note_keeps_its_error_banner_and_buttons_out_of_the_tree(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, windows) = two_open_notes("collapsed-banner", cx);
    let window = windows[1];
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let set = |visual: &mut VisualTestContext, collapsed: bool| {
        window
            .update(visual, |note, _, cx| {
                note.record.collapsed = collapsed;
                note.error = Some(NoteError::save("Could not save: disk full"));
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
    };
    set(&mut visual, false);
    assert!(visual.debug_bounds("error-banner").is_some());
    set(&mut visual, true);
    assert!(
        visual.debug_bounds("error-banner").is_none(),
        "no banner, so no clipped Try Again button to tab to"
    );
}
#[gpui::test]
fn a_system_appearance_change_restyles_every_open_note_not_just_the_key_one(
    cx: &mut TestAppContext,
) {
    use super::*;
    let (_fixture, _library, windows) = two_open_notes("appearance-change", cx);
    // Stand in for a setting that only the system's own values would change: make both
    // notes' editors stale and see which ones the refresh reaches.
    for window in &windows {
        window
            .update(cx, |note, _, _| note.record.style.reading_size = 20.)
            .unwrap();
    }
    let sizes = |cx: &mut TestAppContext| {
        windows
            .iter()
            .map(|window| {
                window
                    .read_with(cx, |note, cx| note.editor.read(cx).style().body_size)
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(sizes(cx), [px(12.); 2]);
    cx.update(refresh_appearance);
    assert_eq!(sizes(cx), [px(20.); 2]);
}
#[gpui::test]
fn the_settings_window_has_one_name_in_the_menus_and_on_its_title(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, _windows) = two_open_notes("settings-name", cx);
    fn names(items: &[MenuItem], found: &mut Vec<String>) {
        for item in items {
            match item {
                MenuItem::Action { name, action, .. }
                    if action.as_any().downcast_ref::<GlassTransparency>().is_some() =>
                {
                    found.push(name.to_string())
                }
                MenuItem::Submenu(menu) => names(&menu.items, found),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    for menu in cx.update(|cx| app_menus(cx)) {
        names(&menu.items, &mut found);
    }
    assert_eq!(found, ["Settings…", "Transparency Settings…"]);
    assert_eq!(settings::TITLE, "Dahlia Settings");
}
#[gpui::test]
fn check_for_updates_follows_about_and_is_disabled_without_sparkle(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, _windows) = two_open_notes("update-menu", cx);
    let menus = cx.update(|cx| app_menus(cx));
    let items = &menus[0].items;
    let name = |item: &MenuItem| match item {
        MenuItem::Action { name, .. } => name.to_string(),
        MenuItem::Separator => "-".to_owned(),
        _ => "other".to_owned(),
    };
    let names: Vec<_> = items.iter().take(4).map(name).collect();
    if cfg!(target_os = "macos") {
        assert_eq!(
            names,
            ["About Dahlia", "Check for Updates…", "-", "Settings…"]
        );
        // The tests run unbundled, so Sparkle is not loaded and the item cannot be used.
        let MenuItem::Action {
            action, disabled, ..
        } = &items[1]
        else {
            panic!("Check for Updates… is an action");
        };
        assert!(*disabled);
        assert!(action.as_any().downcast_ref::<CheckForUpdates>().is_none());
    } else {
        assert_eq!(names, ["About Dahlia", "-", "Settings…", "-"]);
    }
    assert_eq!(
        items
            .iter()
            .filter(|item| name(item) == updates::CHECK_LABEL)
            .count(),
        usize::from(cfg!(target_os = "macos"))
    );
}
#[gpui::test]
fn successful_autosave_keeps_unrelated_errors_and_clears_save_errors(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, windows) = two_open_notes("autosave-errors", cx);
    let window = windows[1];
    let edit_and_autosave = |cx: &mut TestAppContext| {
        window
            .update(cx, |note, _, cx| {
                note.editor.update(cx, |editor, cx| {
                    assert!(
                        editor.dispatch([commands::insert_text("x")(editor.state()).unwrap()], cx)
                    );
                });
            })
            .unwrap();
        super::settle_io(cx);
        cx.executor().advance_clock(Duration::from_millis(500));
        super::settle_io(cx);
    };
    window
        .update(cx, |note, _, _| {
            note.error = Some(NoteError::other("Could not select images."))
        })
        .unwrap();
    edit_and_autosave(cx);
    assert_eq!(
        window.read_with(cx, |note, _| note.error.clone()).unwrap(),
        Some(NoteError::other("Could not select images.")),
    );
    window
        .update(cx, |note, _, _| {
            note.error = Some(NoteError::save("Could not save: disk full"))
        })
        .unwrap();
    edit_and_autosave(cx);
    assert_eq!(
        window.read_with(cx, |note, _| note.error.clone()).unwrap(),
        None
    );
}
#[gpui::test]
fn retry_is_offered_only_for_save_failures_and_collapsed_strips_show_errors(
    cx: &mut TestAppContext,
) {
    use super::*;
    let (_fixture, _library, windows) = two_open_notes("error-actions", cx);
    windows[0]
        .update(cx, |note, _, _| {
            for (error, actions) in [
                (NoteError::save("Could not save: disk full"), vec![RETRY]),
                (
                    NoteError {
                        message: "Could not save: changed".into(),
                        origin: ErrorSource::Conflict,
                        cause: None,
                    },
                    vec![KEEP_MINE, RELOAD],
                ),
                (NoteError::other("Could not select images."), vec![DISMISS]),
            ] {
                note.error = Some(error);
                assert_eq!(note.error_actions().to_vec(), actions);
            }
            note.record.title = "Groceries".into();
            note.error = None;
            assert_eq!(note.strip_label(), "Groceries");
            note.record.collapsed = true;
            note.error = Some(NoteError::save("Could not save: disk full"));
            assert!(note.strip_label().contains("Could not save: disk full"));
        })
        .unwrap();
}
#[gpui::test]
fn check_document_survives_autosave_and_never_claims_no_issues_after_an_interrupted_scan(
    cx: &mut TestAppContext,
) {
    let (_fixture, _library, windows) = two_open_notes("check-document", cx);
    let window = windows[1];
    window
        .update(cx, |note, window, cx| {
            note.check_document(window, cx);
            // The autosave timer refreshes checking while the command's scan runs.
            note.checking_task = Some(note.checker.refresh(
                &note.editor,
                note.record.style.checking,
                cx,
            ));
        })
        .unwrap();
    super::settle_io(cx);
    let prompt = cx.pending_prompt();
    assert!(prompt.is_some(), "the command must not be silently dropped");
    let (title, detail) = prompt.unwrap();
    assert!(title.contains("changed while"), "{title}");
    assert!(detail.contains("Check Document Now to try again"), "{detail}");
}
#[gpui::test]
fn launch_failure_alerts_before_quitting(cx: &mut TestAppContext) {
    notify_take_alerts_reset();
    let error = std::io::Error::from(std::io::ErrorKind::WouldBlock);
    cx.update(|cx| super::launch_failure(cx, &error));
    super::settle_io(cx);
    let alerts = super::notify::take_alerts();
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert!(alerts[0].starts_with("Dahlia can’t open your notes."));
    assert!(
        alerts[0].contains("open in another copy of Dahlia"),
        "{alerts:?}"
    );
    assert!(!alerts[0].contains("WouldBlock"), "no Debug text");
}
fn notify_take_alerts_reset() {
    super::notify::take_alerts();
}
/// A library with one note open in a test window, edited to "Mine Original" and changed on disk.
fn conflicted_note(
    _label: &str,
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    std::rc::Rc<std::cell::RefCell<super::Notebook>>,
    super::NoteRecord,
    gpui::WindowHandle<Note>,
) {
    use super::*;
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
    let record = library.borrow_mut().create(Frame::default()).unwrap();
    let (mut store, _) = library.borrow().open_store(&record.id).unwrap();
    store.save("Original").unwrap();
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let id = record.id.clone();
    let opened = record.clone();
    let window = cx.add_window(|_, cx| {
        Note::new_record(
            store,
            parse_note(&commonmark_schema(), "Original").unwrap(),
            opened,
            Some(library.clone()),
            cx,
        )
    });
    cx.update(|cx| {
        cx.global_mut::<AppState>().windows.insert(id, window);
    });
    window
        .update(cx, |note, _, cx| {
            note.editor.update(cx, |editor, cx| {
                assert!(editor.dispatch(
                    [commands::insert_text("Mine ")(editor.state()).unwrap()],
                    cx
                ));
            });
        })
        .unwrap();
    let path = root.join("notes").join(&record.id).join("note.md");
    std::fs::write(&path, "Theirs").unwrap();
    // Saving now fails, as every later save did before the fix.
    assert!(!window.update(cx, |note, _, cx| note.save(cx)).unwrap());
    assert_eq!(
        window
            .read_with(cx, |note, _| note.error.as_ref().map(|error| error.origin))
            .unwrap(),
        Some(ErrorSource::Conflict)
    );
    (_root_fixture, library, record, window)
}
#[gpui::test]
fn quit_failure_keeps_windows_and_refuses_native_termination(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, record, window) = conflicted_note("quit-conflict", cx);
    cx.update(|cx| cx.global_mut::<AppState>().active = Some(record.id.clone()));
    cx.update(quit_all);
    super::settle_io(cx);
    cx.update(|cx| {
        let app = cx.global::<AppState>();
        assert!(!app.quitting, "a failed quit can be retried");
        assert!(
            !app.ready_to_quit.get(),
            "AppKit must keep refusing termination"
        );
        assert!(app.windows.contains_key(&record.id));
    });
    window
        .read_with(cx, |note, cx| {
            assert!(note.source_text(cx).unwrap().contains("Mine Original"));
            assert!(
                !note
                    .durability_lock
                    .load(std::sync::atomic::Ordering::Acquire)
            );
            assert!(note.error.as_ref().unwrap().message.contains("didn’t quit"));
        })
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(
            _fixture
                .path()
                .join("notes")
                .join(record.id)
                .join("note.md")
        )
        .unwrap(),
        "Theirs"
    );
}
#[gpui::test]
fn a_refused_quit_save_rescues_the_unsaved_text_into_recovery(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, library, record, window) = conflicted_note("quit-rescue", cx);
    let rescue = window
        .read_with(cx, |note, cx| note.rescue_unsaved(cx))
        .unwrap();
    rescue();
    // The note on disk is not touched; the text it could not take is kept beside it.
    let root = _fixture.path();
    assert_eq!(
        std::fs::read_to_string(root.join("notes").join(&record.id).join("note.md")).unwrap(),
        "Theirs"
    );
    let entry = library
        .borrow()
        .recovery_entries()
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Unsaved text from a failed save")
        .expect("the rescued text is listed in Recovery");
    let rescued = Notebook::read_recovery_note(root, &entry, &record.id).unwrap();
    assert!(rescued.text.contains("Mine Original"), "{}", rescued.text);
}
#[gpui::test]
fn conflict_keep_mine_as_copy_saves_edits_and_unblocks_close(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, library, record, window) = conflicted_note("keep-mine", cx);
    let root = _fixture.path();
    window
        .update(cx, |note, window, cx| note.keep_mine_as_copy(window, cx))
        .unwrap();
    super::settle_io(cx);
    let notes = library.borrow().notes().to_vec();
    assert_eq!(notes.len(), 2);
    let copy = notes.iter().find(|n| n.id != record.id).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("notes").join(&copy.id).join("note.md")).unwrap(),
        "Mine Original"
    );
    assert!(
        cx.update(|cx| cx.global::<AppState>().windows.contains_key(&copy.id)),
        "the copy opens in its own window"
    );
    let path = root.join("notes").join(&record.id).join("note.md");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Theirs");
    // The original reopened from disk and closes normally.
    let reopened = cx
        .update(|cx| cx.global::<AppState>().windows.get(&record.id).copied())
        .unwrap();
    reopened
        .update(cx, |note, window, cx| {
            assert_eq!(note.source_text(cx).unwrap(), "Theirs");
            assert_eq!(note.error, None);
            note.finish_close(NoteState::Archived, window, cx)
        })
        .unwrap();
    super::settle_io(cx);
    assert_eq!(
        library.borrow().note(&record.id).unwrap().state,
        NoteState::Archived
    );
}
#[gpui::test]
fn conflict_reload_from_disk_keeps_edits_in_history_and_unblocks_close(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, library, record, window) = conflicted_note("reload", cx);
    let root = _fixture.path();
    window
        .update(cx, |note, window, cx| note.reload_from_disk(window, cx))
        .unwrap();
    super::settle_io(cx);
    assert_eq!(library.borrow().notes().len(), 1);
    let directory = root.join("notes").join(&record.id);
    assert_eq!(
        std::fs::read_to_string(directory.join("note.md")).unwrap(),
        "Theirs"
    );
    let preserved = std::fs::read_dir(directory.join("history"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| std::fs::read_to_string(entry.path()).unwrap() == "Mine Original");
    assert!(preserved, "discarded edits stay recoverable");
    let reopened = cx
        .update(|cx| cx.global::<AppState>().windows.get(&record.id).copied())
        .unwrap();
    reopened
        .update(cx, |note, window, cx| {
            assert_eq!(note.source_text(cx).unwrap(), "Theirs");
            note.finish_close(NoteState::Archived, window, cx)
        })
        .unwrap();
    super::settle_io(cx);
    assert_eq!(
        library.borrow().note(&record.id).unwrap().state,
        NoteState::Archived
    );
}
#[gpui::test]
fn a_reload_that_cannot_reopen_leaves_no_active_note_behind(cx: &mut TestAppContext) {
    use super::*;
    let (_fixture, _library, record, window) = conflicted_note("reload-fails", cx);
    cx.update(|cx| cx.global_mut::<AppState>().active = Some(record.id.clone()));
    // The file on disk can no longer be read, so the reopened window never appears.
    let path = _fixture.path().join("notes").join(&record.id).join("note.md");
    std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
    window
        .update(cx, |note, window, cx| note.reload_from_disk(window, cx))
        .unwrap();
    super::settle_io(cx);
    cx.update(|cx| {
        let app = cx.global::<AppState>();
        assert!(app.windows.is_empty());
        assert!(app.format_menus.is_empty());
        assert_eq!(app.active, None, "active must not name a window that is gone");
    });
    assert!(!crate::notify::take_alerts().is_empty(), "the failure is reported");
}
#[gpui::test]
fn deleting_restoring_one_note_keeps_other_undo_and_failed_close_keeps_window(
    cx: &mut TestAppContext,
) {
    use super::*;
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
    let a = library.borrow_mut().create(Frame::default()).unwrap();
    let b = library.borrow_mut().create(Frame::default()).unwrap();
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let mut handles = Vec::new();
    for record in [a.clone(), b.clone()] {
        let (store, _) = library.borrow().open_store(&record.id).unwrap();
        let id = record.id.clone();
        let window = cx.add_window(|_, cx| {
            Note::new_record(
                store,
                parse_note(&commonmark_schema(), "Original").unwrap(),
                record,
                Some(library.clone()),
                cx,
            )
        });
        cx.update(|cx| {
            cx.global_mut::<AppState>().windows.insert(id, window);
        });
        window
            .update(cx, |note, _, cx| {
                note.editor.update(cx, |editor, cx| {
                    assert!(editor.dispatch(
                        [commands::insert_text("Edited ")(editor.state()).unwrap()],
                        cx
                    ));
                });
                assert!(note.save(cx));
            })
            .unwrap();
        handles.push(window);
    }
    handles[0]
        .update(cx, |note, window, cx| {
            note.finish_close(NoteState::Trash, window, cx)
        })
        .unwrap();
    super::settle_io(cx);
    assert_eq!(
        library.borrow().note(&a.id).unwrap().state,
        NoteState::Trash
    );
    cx.update(operations::undo_delete);
    super::settle_io(cx);
    assert_eq!(
        library.borrow().note(&a.id).unwrap().state,
        NoteState::Active
    );
    assert_eq!(
        library.borrow().open_store(&a.id).err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
    handles[1]
        .update(cx, |note, _, cx| {
            note.editor.update(cx, |editor, cx| {
                assert!(editor.dispatch([history::undo(editor.state()).unwrap()], cx));
            });
            assert_eq!(note.source_text(cx).unwrap(), "Original");
        })
        .unwrap();
    cx.update(operations::redo_delete);
    super::settle_io(cx);
    assert_eq!(
        library.borrow().note(&a.id).unwrap().state,
        NoteState::Trash
    );
    std::fs::write(
        root.join("notes").join(&b.id).join("note.md"),
        "external change",
    )
    .unwrap();
    handles[1]
        .update(cx, |note, window, cx| {
            note.finish_close(NoteState::Archived, window, cx)
        })
        .unwrap();
    super::settle_io(cx);
    assert_eq!(
        library.borrow().note(&b.id).unwrap().state,
        NoteState::Active
    );
    assert!(
        handles[1]
            .read_with(cx, |note, _| note.error.is_some())
            .unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notes").join(&b.id).join("note.md")).unwrap(),
        "external change"
    );
}
#[gpui::test]
fn shared_glass_transparency_preserves_both_note_editors(cx: &mut TestAppContext) {
    use super::*;
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let library = Rc::new(RefCell::new(Notebook::open(root.clone()).unwrap()));
    let mut classic = library.borrow_mut().create(Frame::default()).unwrap();
    classic.style = appearance::NoteStyle::legacy();
    library.borrow_mut().update(classic.clone()).unwrap();
    let glass = library.borrow_mut().create(Frame::default()).unwrap();
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let mut editors = Vec::new();
    for record in [classic, glass] {
        let (store, _) = library.borrow().open_store(&record.id).unwrap();
        let id = record.id.clone();
        let window = cx.add_window(|_, cx| {
            Note::new_record(
                store,
                parse_note(&commonmark_schema(), "Original").unwrap(),
                record,
                Some(library.clone()),
                cx,
            )
        });
        let editor = window.update(cx, |note, _, _| note.editor.clone()).unwrap();
        editor.update(cx, |editor, cx| {
            assert!(editor.dispatch(
                [commands::insert_text("Edited ")(editor.state()).unwrap()],
                cx
            ))
        });
        cx.update(|cx| {
            cx.global_mut::<AppState>().windows.insert(id, window);
        });
        editors.push(editor);
    }
    cx.update(|cx| set_glass_tint(0., cx));
    super::settle_io(cx);
    cx.executor().advance_clock(Duration::from_millis(500));
    super::settle_io(cx);
    assert_eq!(library.borrow().glass_tint(), 0.);
    for editor in editors {
        editor.update(cx, |editor, cx| {
            assert!(editor.dispatch([history::undo(editor.state()).unwrap()], cx))
        });
    }
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("index.json")).unwrap()).unwrap();
    assert_eq!(index["glass_tint"], 0.);
    let styles: Vec<_> = library
        .borrow()
        .notes()
        .iter()
        .map(|n| n.style.appearance)
        .collect();
    assert_eq!(styles, vec![Appearance::Classic, Appearance::Glass]);
}
#[gpui::test]
fn two_note_windows_keep_history_and_writes_independent(cx: &mut TestAppContext) {
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let (sa, _) = NoteStore::open(root.join("a")).unwrap();
    let (sb, _) = NoteStore::open(root.join("b")).unwrap();
    let (a, _) = cx.add_window_view(|_, cx| Note::new(sa, "First".into(), cx));
    let (b, cx) = cx.add_window_view(|_, cx| Note::new(sb, "Second".into(), cx));
    super::settle_io(cx);
    for (note, text) in [(&a, "A-edit "), (&b, "B-edit ")] {
        let editor = note.read_with(cx, |note, _| note.editor.clone());
        editor.update(cx, |editor, cx| {
            let edit = commands::insert_text(text)(editor.state()).unwrap();
            assert!(editor.dispatch([edit], cx));
        });
        note.update(cx, |note, cx| assert!(note.save(cx)));
    }
    let editor = a.read_with(cx, |note, _| note.editor.clone());
    editor.update(cx, |editor, cx| {
        let undo = history::undo(editor.state()).unwrap();
        assert!(editor.dispatch([undo], cx));
    });
    a.update(cx, |note, cx| {
        assert!(note.save(cx));
        note.writer.close().unwrap();
    });
    b.update(cx, |note, cx| assert!(note.save(cx)));
    assert_eq!(
        std::fs::read_to_string(root.join("a/note.md")).unwrap(),
        "First"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("b/note.md")).unwrap(),
        "B-edit Second"
    );
    assert_eq!(NoteStore::open(root.join("a")).unwrap().1, "First");
}
#[gpui::test]
fn editor_autosaves_edits_undo_redo_and_close(cx: &mut TestAppContext) {
    let _directory_fixture = tempfile::tempdir().unwrap();
    let directory = _directory_fixture.path().to_path_buf();
    let original = "---\ntitle: retained\n---\n\nOriginal\n\n<!-- untouched -->\n";
    let (mut store, _) = NoteStore::open(directory.clone()).unwrap();
    store.save(original).unwrap();
    let (note, cx) = cx.add_window_view(|_, cx| Note::new(store, original.into(), cx));
    super::settle_io(cx);
    let editor = note.read_with(cx, |note, _| note.editor.clone());
    editor.update(cx, |editor, cx| {
        let edit = commands::insert_text("Edited ")(editor.state()).unwrap();
        assert!(editor.dispatch([edit], cx));
    });
    super::settle_io(cx);
    cx.executor().advance_clock(Duration::from_millis(500));
    super::settle_io(cx);
    note.read_with(cx, |note, _| {
        assert!(note.write_task.is_some());
        note.writer.barrier();
    });
    super::settle_io(cx);
    let edited = std::fs::read_to_string(directory.join("note.md")).unwrap();
    assert!(edited.contains("Edited Original"), "{edited}");
    assert!(edited.starts_with("---\ntitle: retained\n---\n\n"));
    assert!(edited.ends_with("<!-- untouched -->\n"));
    for (command, expected) in [
        (
            history::undo as fn(&markraft_core::EditorState) -> _,
            original,
        ),
        (history::redo, edited.as_str()),
    ] {
        editor.update(cx, |editor, cx| {
            let edit = command(editor.state()).unwrap();
            assert!(editor.dispatch([edit], cx));
        });
        super::settle_io(cx);
        cx.executor().advance_clock(Duration::from_millis(500));
        super::settle_io(cx);
        note.read_with(cx, |note, _| note.writer.barrier());
        super::settle_io(cx);
        assert_eq!(
            std::fs::read_to_string(directory.join("note.md")).unwrap(),
            expected
        );
    }
    // Close flushes committed edits even before the debounce fires.
    editor.update(cx, |editor, cx| {
        let edit = commands::insert_text("Immediate ")(editor.state()).unwrap();
        assert!(editor.dispatch([edit], cx));
    });
    note.update(cx, |note, cx| assert!(note.save(cx)));
    assert!(
        std::fs::read_to_string(directory.join("note.md"))
            .unwrap()
            .contains("Immediate")
    );
    let before_composition = std::fs::read_to_string(directory.join("note.md")).unwrap();
    editor.update(cx, |editor, cx| {
        let candidate =
            markraft_core::composition::update_composition(editor.state(), "候補", 2).unwrap();
        assert!(editor.dispatch([candidate], cx));
    });
    note.update(cx, |note, cx| assert!(note.save(cx)));
    assert_eq!(
        std::fs::read_to_string(directory.join("note.md")).unwrap(),
        before_composition
    );
    editor.update(cx, |editor, cx| {
        let cancel = markraft_core::composition::cancel_composition(editor.state()).unwrap();
        assert!(editor.dispatch([cancel], cx));
    });
    note.update(cx, |note, cx| assert!(note.save(cx)));
    assert_eq!(
        std::fs::read_to_string(directory.join("note.md")).unwrap(),
        before_composition
    );
}
#[gpui::test]
fn appearance_refresh_keeps_editor_source_selection_and_undo(cx: &mut TestAppContext) {
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let original = "---\ntitle: preserve\n---\n\nPlain `code`\n";
    let (mut store, _) = NoteStore::open(root).unwrap();
    store.save(original).unwrap();
    let (note, cx) = cx.add_window_view(|_, cx| Note::new(store, original.into(), cx));
    let editor = note.read_with(cx, |note, _| note.editor.clone());
    editor.update(cx, |editor, cx| {
        assert!(editor.dispatch(
            [commands::insert_text("Edited ")(editor.state()).unwrap()],
            cx
        ));
    });
    let selection = editor.read_with(cx, |editor, _| editor.state().selection().clone());
    for appearance in [Appearance::Glass, Appearance::SeaGlass, Appearance::Classic] {
        cx.update(|window, cx| {
            note.update(cx, |note, cx| {
                note.record.style.appearance = appearance;
                note.record.style.color = ColorIndex::ALL[2];
                note.record.style.reading_size = 16.;
                note.restyle(window, cx);
                assert_eq!(note.editor.entity_id(), editor.entity_id());
                assert!(note.source_text(cx).unwrap().contains("Edited Plain"));
            })
        });
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.state().selection().clone()),
            selection
        );
    }
    editor.update(cx, |editor, cx| {
        assert!(editor.dispatch([markraft_core::history::undo(editor.state()).unwrap()], cx));
    });
    note.read_with(cx, |note, cx| {
        assert_eq!(note.source_text(cx).unwrap(), original)
    });
}
#[gpui::test]
fn closing_the_active_note_hands_active_to_a_remaining_window(cx: &mut TestAppContext) {
    use super::*;
    let _root_fixture = tempfile::tempdir().unwrap();
    let library = Rc::new(RefCell::new(
        Notebook::open(_root_fixture.path().to_path_buf()).unwrap(),
    ));
    let records: Vec<_> = (0..3)
        .map(|_| library.borrow_mut().create(Frame::default()).unwrap())
        .collect();
    cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
    let mut handles = Vec::new();
    for record in records.clone() {
        let (store, _) = library.borrow().open_store(&record.id).unwrap();
        let id = record.id.clone();
        let window = cx.add_window(|_, cx| {
            Note::new_record(
                store,
                parse_note(&commonmark_schema(), "Original").unwrap(),
                record,
                Some(library.clone()),
                cx,
            )
        });
        cx.update(|cx| {
            cx.global_mut::<AppState>().windows.insert(id, window);
        });
        handles.push(window);
    }
    let active = |cx: &mut TestAppContext| cx.update(|cx| cx.global::<AppState>().active.clone());
    cx.update(|cx| cx.global_mut::<AppState>().active = Some(records[0].id.clone()));
    // Closing a note in the background leaves the active one alone.
    handles[2]
        .update(cx, |note, window, cx| {
            note.finish_close(NoteState::Archived, window, cx)
        })
        .unwrap();
    super::settle_io(cx);
    assert_eq!(active(cx), Some(records[0].id.clone()));
    // Closing the active note moves it to a window that is still open.
    handles[0]
        .update(cx, |note, window, cx| {
            note.finish_close(NoteState::Archived, window, cx)
        })
        .unwrap();
    super::settle_io(cx);
    assert_eq!(active(cx), Some(records[1].id.clone()));
    handles[1]
        .update(cx, |note, window, cx| {
            note.finish_close(NoteState::Archived, window, cx)
        })
        .unwrap();
    super::settle_io(cx);
    assert_eq!(active(cx), None);
}
#[test]
fn a_system_termination_waits_for_the_save_instead_of_cancelling() {
    use super::*;
    let awaiting = std::cell::Cell::new(false);
    let mut started = 0;
    // Saving has not finished: answer Later, so logout and shutdown are not aborted.
    let reply = answer_termination(false, &awaiting, || {
        started += 1;
        true
    });
    assert_eq!((reply, started), (Termination::Later, 1));
    assert!(awaiting.get(), "the finished save owes AppKit a reply");
    // A repeated request while the save runs still waits and does not start a second one.
    let reply = answer_termination(false, &awaiting, || true);
    assert_eq!(reply, Termination::Later);
}
#[test]
fn a_system_termination_after_the_save_proceeds_at_once() {
    use super::*;
    let awaiting = std::cell::Cell::new(false);
    let reply = answer_termination(true, &awaiting, || panic!("nothing left to save"));
    assert_eq!(reply, Termination::Now);
    assert!(!awaiting.get());
}
#[test]
fn a_quit_that_cannot_start_cancels_rather_than_hanging_the_system() {
    use super::*;
    let awaiting = std::cell::Cell::new(false);
    // A note is still opening: nothing will ever reply, so a deferred answer would hang.
    let reply = answer_termination(false, &awaiting, || false);
    assert_eq!(reply, Termination::Cancel);
    assert!(!awaiting.get());
}
#[test]
fn a_finished_save_answers_the_system_once_and_menu_quit_quits_itself() {
    use super::*;
    // Menu Quit: nobody is waiting, so Dahlia quits itself after a good save.
    let awaiting = std::cell::Cell::new(false);
    assert_eq!(finish_quit(&awaiting, true), QuitEnd::Quit);
    assert_eq!(finish_quit(&awaiting, false), QuitEnd::Stay);
    // System request: the reply is the outcome, and it is sent only once.
    awaiting.set(true);
    assert_eq!(finish_quit(&awaiting, true), QuitEnd::Reply(true));
    assert!(!awaiting.get());
    assert_eq!(finish_quit(&awaiting, true), QuitEnd::Quit);
    awaiting.set(true);
    assert_eq!(finish_quit(&awaiting, false), QuitEnd::Reply(false));
}
