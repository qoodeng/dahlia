use crate::*;

pub(crate) const NOTE_EDITOR_CONTEXT: &str = "DahliaNote > Markraft";
pub(crate) const RETRY: &str = "Try Again";
pub(crate) const KEEP_MINE: &str = "Keep Mine as a Copy";
pub(crate) const RELOAD: &str = "Reload";
pub(crate) const DISMISS: &str = "Dismiss";
/// Help ▸ Dahlia Help: the few things a Stickies user would not find by looking.
pub(crate) const HELP: &str = "Dahlia Help\n\nEach note is a Markdown file. Type Markdown, or use the Format menu.\n\nDouble-click a note’s top strip to collapse it to one line; on a Mac this follows your title-bar double-click setting. Drag the strip to move the note.\n\nGlass and Sea Glass notes share one transparency, set in Appearance ▸ Transparency Settings…\n\nClosing a note asks whether to archive or delete it. Archived and deleted notes, snapshots and earlier versions of notes are in File ▸ Recovery.";
actions!(
    dahlia,
    [
        Quit,
        Save,
        NewNote,
        CloseNote,
        ToggleCollapse,
        ToggleFloat,
        Zoom,
        ToggleTranslucent,
        UseAsDefault,
        GlassTransparency,
        ImportNotes,
        ImportImages,
        ExportNote,
        UndoDeleteNote,
        RedoDeleteNote,
        ShowRecovery,
        CreateCheckpoint,
        ExportAll,
        Find,
        FindAndReplace,
        FindNext,
        FindPrevious,
        UseSelectionForFind,
        JumpToSelection,
        CheckDocument,
        Minimize,
        CollapseAll,
        ExpandAll,
        MinimizeAll,
        ZoomAll,
        UndoArrange,
        RedoArrange,
        About,
        CheckForUpdates,
        Hide,
        HideOthers,
        ShowAll,
        Help,
        Unavailable
    ]
);
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub(crate) struct OpenNote {
    pub(crate) id: String,
}
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub(crate) struct RestoreNote {
    pub(crate) id: String,
    pub(crate) expected: NoteState,
}
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub(crate) struct SetAppearance {
    pub(crate) appearance: Appearance,
}
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub(crate) struct SetColor {
    pub(crate) color: ColorIndex,
}
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub(crate) struct ReadingSize {
    pub(crate) delta: f32,
    pub(crate) reset: bool,
}
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub(crate) struct ChangeTransparency {
    pub(crate) delta: f32,
}
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub(crate) struct ToggleChecking {
    pub(crate) setting: checking::Setting,
}

pub(crate) fn bind(cx: &mut App) {
    markraft_gpui::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("cmd-b", markraft_gpui::Bold, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-i", markraft_gpui::Italic, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-&", markraft_gpui::Ordered, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-*", markraft_gpui::Bullet, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-(", markraft_gpui::Task, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new(
            "cmd-enter",
            markraft_gpui::ToggleTask,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-shift-b",
            markraft_gpui::Quote,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        // Code Block keeps Markraft's Option-Command-C: notes have no styles for Stickies'
        // Copy Style to copy. Command-E is Use Selection for Find, as in every Mac editor
        // and Stickies, rather than Markraft's inline-code shortcut.
        KeyBinding::new("cmd-e", UseSelectionForFind, Some(NOTE_EDITOR_CONTEXT)),
        // Paste and Match Style is Option-Shift-Command-V in Stickies and the HIG;
        // Shift-Command-V keeps working. Bound last, so it is the one the menu shows.
        KeyBinding::new("cmd-shift-v", markraft_gpui::PastePlain, Some("Markraft")),
        KeyBinding::new(
            "cmd-alt-shift-v",
            markraft_gpui::PastePlain,
            Some("Markraft"),
        ),
        KeyBinding::new(
            "cmd-=",
            ReadingSize {
                delta: 1.,
                reset: false,
            },
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-+",
            ReadingSize {
                delta: 1.,
                reset: false,
            },
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd--",
            ReadingSize {
                delta: -1.,
                reset: false,
            },
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-0",
            ReadingSize {
                delta: 0.,
                reset: true,
            },
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-alt-0",
            markraft_gpui::Paragraph,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-alt-1",
            markraft_gpui::Heading,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-alt-2",
            markraft_gpui::Heading2,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-alt-3",
            markraft_gpui::Heading3,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-alt-4",
            markraft_gpui::Heading4,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-alt-5",
            markraft_gpui::Heading5,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-alt-6",
            markraft_gpui::Heading6,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-shift-c",
            markraft_gpui::Code,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "cmd-shift-x",
            markraft_gpui::Strikethrough,
            Some(NOTE_EDITOR_CONTEXT),
        ),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-n", NewNote, None),
        KeyBinding::new("cmd-w", CloseNote, None),
        KeyBinding::new("cmd-f", Find, None),
        KeyBinding::new("cmd-,", GlassTransparency, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        // Stickies' Window menu: Collapse is Command-M, Zoom Shift-Command-M and Collapse
        // All Option-Command-M. Minimize has no shortcut there, so it has none here.
        KeyBinding::new("cmd-m", ToggleCollapse, None),
        KeyBinding::new("cmd-shift-m", Zoom, None),
        KeyBinding::new("cmd-alt-m", CollapseAll, None),
        KeyBinding::new("cmd-?", Help, None),
        KeyBinding::new("cmd-g", FindNext, None),
        KeyBinding::new("cmd-shift-g", FindPrevious, None),
        KeyBinding::new("cmd-e", UseSelectionForFind, None),
        KeyBinding::new("cmd-j", JumpToSelection, None),
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("ctrl-s", Save, None),
        KeyBinding::new("ctrl-n", NewNote, None),
        KeyBinding::new("ctrl-w", CloseNote, None),
        KeyBinding::new("cmd-alt-t", ToggleTranslucent, None),
        // Stickies' Color menu is Command-1 to 6. In the editor those beat Markraft's
        // heading shortcuts; headings stay on Option-Command-0 to 6.
        KeyBinding::new("cmd-1", SetColor { color: ColorIndex::ALL[0] }, Some("DahliaNote")),
        KeyBinding::new("cmd-2", SetColor { color: ColorIndex::ALL[1] }, Some("DahliaNote")),
        KeyBinding::new("cmd-3", SetColor { color: ColorIndex::ALL[2] }, Some("DahliaNote")),
        KeyBinding::new("cmd-4", SetColor { color: ColorIndex::ALL[3] }, Some("DahliaNote")),
        KeyBinding::new("cmd-5", SetColor { color: ColorIndex::ALL[4] }, Some("DahliaNote")),
        KeyBinding::new("cmd-6", SetColor { color: ColorIndex::ALL[5] }, Some("DahliaNote")),
        KeyBinding::new("cmd-1", SetColor { color: ColorIndex::ALL[0] }, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-2", SetColor { color: ColorIndex::ALL[1] }, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-3", SetColor { color: ColorIndex::ALL[2] }, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-4", SetColor { color: ColorIndex::ALL[3] }, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-5", SetColor { color: ColorIndex::ALL[4] }, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-6", SetColor { color: ColorIndex::ALL[5] }, Some(NOTE_EDITOR_CONTEXT)),
        KeyBinding::new("cmd-alt-[", ChangeTransparency { delta: 0.025 }, None),
        KeyBinding::new("cmd-alt-]", ChangeTransparency { delta: -0.025 }, None),
    ]);
    // Stickies' remaining Window and Spelling shortcuts, for commands only macOS offers.
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-alt-f", ToggleFloat, None),
        KeyBinding::new("cmd-alt-z", UndoArrange, None),
        KeyBinding::new("cmd-alt-shift-z", RedoArrange, None),
        KeyBinding::new("cmd-;", CheckDocument, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    {
        bind_ctrl_keys(cx);
        linux_menu::bind_keys(cx);
    }
    cx.on_action(|_: &Quit, cx| cx.defer(quit_all));
    cx.on_action(|_: &NewNote, cx| cx.defer(create_note));
    cx.on_action(|_: &GlassTransparency, cx| cx.defer(settings::show));
    cx.on_action(|_: &ImportNotes, cx| cx.defer(operations::import_notes));
    cx.on_action(|_: &ShowRecovery, cx| cx.defer(recovery::show));
    cx.on_action(|_: &CreateCheckpoint, cx| cx.defer(recovery::checkpoint));
    cx.on_action(|_: &ExportAll, cx| cx.defer(operations::export_all));
    cx.on_action(|_: &Find, cx| cx.defer(finding::show));
    cx.on_action(|_: &FindAndReplace, cx| cx.defer(finding::show));
    cx.on_action(|_: &About, _| platform::show_about());
    cx.on_action(|_: &CheckForUpdates, cx| cx.defer(|_| updates::check()));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Help, _| {
        drop(notify::app_alert(HELP));
    });
    cx.on_action(|_: &FindNext, cx| cx.defer(|cx| finding::next(true, cx)));
    cx.on_action(|_: &FindPrevious, cx| cx.defer(|cx| finding::next(false, cx)));
    cx.on_action(|_: &UseSelectionForFind, cx| cx.defer(finding::selection));
    cx.on_action(|_: &CollapseAll, cx| {
        cx.defer(|cx| window_commands::all(window_commands::BulkOperation::Collapse, cx))
    });
    cx.on_action(|_: &ExpandAll, cx| {
        cx.defer(|cx| window_commands::all(window_commands::BulkOperation::Expand, cx))
    });
    cx.on_action(|_: &MinimizeAll, cx| {
        cx.defer(|cx| window_commands::all(window_commands::BulkOperation::Minimize, cx))
    });
    cx.on_action(|_: &ZoomAll, cx| {
        cx.defer(|cx| window_commands::all(window_commands::BulkOperation::Zoom, cx))
    });
    cx.on_action(|_: &UndoArrange, cx| cx.defer(|cx| window_commands::undo(false, cx)));
    cx.on_action(|_: &RedoArrange, cx| cx.defer(|cx| window_commands::undo(true, cx)));
    cx.on_action(|action: &window_commands::Arrange, cx| {
        let order = action.order;
        cx.defer(move |cx| window_commands::arrange(order, cx));
    });
    cx.on_action(|action: &window_commands::SetPolicy, cx| {
        let result = cx
            .global::<AppState>()
            .notebook
            .borrow_mut()
            .set_arrange_policy(action.policy);
        if let Err(error) = result {
            notify(
                cx,
                copy::failed("Couldn’t save the arrangement setting.", &error),
            );
        }
        update_menus(cx);
    });
    cx.on_action(|_: &JumpToSelection, cx| {
        cx.defer(|cx| {
            if let Some(handle) = active_window(cx) {
                let _ = handle.update(cx, |note, window, cx| {
                    if note.record.collapsed {
                        note.collapse(window, cx);
                    }
                    note.editor.update(cx, |editor, cx| {
                        editor.dispatch(
                            [markraft_core::TransactionSpec::new()
                                .selection(editor.state().selection().clone())
                                .scroll_into_view()],
                            cx,
                        );
                    });
                    window.activate_window();
                    window.focus(&note.editor.read(cx).focus_handle(cx), cx);
                });
            }
        })
    });
    cx.on_action(|action: &RestoreNote, cx| {
        let id = action.id.clone();
        let expected = action.expected;
        cx.defer(move |cx| operations::restore(&id, expected, false, cx));
    });
    cx.on_action(|_: &UndoDeleteNote, cx| cx.defer(operations::undo_delete));
    cx.on_action(|_: &RedoDeleteNote, cx| cx.defer(operations::redo_delete));
    cx.on_action(|action: &ChangeTransparency, cx| {
        let value = glass_tint(cx) + action.delta;
        cx.defer(move |cx| set_glass_tint(value, cx));
    });
    cx.on_action(|action: &OpenNote, cx| {
        let id = action.id.clone();
        cx.defer(move |cx| {
            if let Err(error) = open_note(&id, cx) {
                notify(cx, error);
            }
        });
    });
}

/// Markraft and the shortcuts above use `cmd`, which is Super on Linux and Windows. Bind the
/// conventional Ctrl equivalents there; bound last, they win over Markraft's Emacs Ctrl keys at
/// the same context depth. macOS keeps its bindings unchanged.
#[cfg(not(target_os = "macos"))]
fn bind_ctrl_keys(cx: &mut App) {
    use markraft_gpui as m;
    const MARKRAFT: Option<&str> = Some("Markraft");
    const EDITOR: Option<&str> = Some(NOTE_EDITOR_CONTEXT);
    let reading = |delta: f32, reset: bool| ReadingSize { delta, reset };
    cx.bind_keys([
        // Markraft's editing keys, scoped as Markraft scopes them.
        KeyBinding::new("ctrl-a", m::SelectAll, MARKRAFT),
        KeyBinding::new("ctrl-c", m::Copy, MARKRAFT),
        KeyBinding::new("ctrl-x", m::Cut, MARKRAFT),
        KeyBinding::new("ctrl-v", m::Paste, MARKRAFT),
        KeyBinding::new("ctrl-shift-v", m::PastePlain, MARKRAFT),
        KeyBinding::new("ctrl-z", m::Undo, MARKRAFT),
        KeyBinding::new("ctrl-shift-z", m::Redo, MARKRAFT),
        KeyBinding::new("ctrl-y", m::Redo, MARKRAFT),
        KeyBinding::new("ctrl-left", m::WordLeft, MARKRAFT),
        KeyBinding::new("ctrl-right", m::WordRight, MARKRAFT),
        KeyBinding::new("ctrl-shift-left", m::SelectWordLeft, MARKRAFT),
        KeyBinding::new("ctrl-shift-right", m::SelectWordRight, MARKRAFT),
        KeyBinding::new("ctrl-backspace", m::DeleteWordBackward, MARKRAFT),
        KeyBinding::new("ctrl-delete", m::DeleteWordForward, MARKRAFT),
        KeyBinding::new("ctrl-home", m::DocumentStart, MARKRAFT),
        KeyBinding::new("ctrl-end", m::DocumentEnd, MARKRAFT),
        KeyBinding::new("ctrl-shift-home", m::SelectDocumentStart, MARKRAFT),
        KeyBinding::new("ctrl-shift-end", m::SelectDocumentEnd, MARKRAFT),
        // Ctrl+Alt+L locks the screen on KDE, Xfce and Cinnamon.
        KeyBinding::new("ctrl-shift-l", m::ChooseCodeLanguage, MARKRAFT),
        // Markraft's other Emacs keys are a Mac text-view convention; GTK and Qt editors
        // leave Ctrl+D, Ctrl+K and Ctrl+P alone, and Ctrl+H is Replace (below).
        KeyBinding::new("ctrl-d", NoAction, MARKRAFT),
        KeyBinding::new("ctrl-k", NoAction, MARKRAFT),
        KeyBinding::new("ctrl-p", NoAction, MARKRAFT),
        KeyBinding::new("ctrl-h", FindAndReplace, MARKRAFT),
        // App shortcuts that Markraft's Emacs keys (Ctrl+F/N/E) would otherwise take in a note.
        KeyBinding::new("ctrl-f", Find, MARKRAFT),
        KeyBinding::new("ctrl-n", NewNote, MARKRAFT),
        KeyBinding::new("ctrl-e", UseSelectionForFind, MARKRAFT),
        // Dahlia's note-editor shortcuts, mirroring the Command bindings above.
        KeyBinding::new("ctrl-b", m::Bold, EDITOR),
        KeyBinding::new("ctrl-i", m::Italic, EDITOR),
        KeyBinding::new("ctrl-shift-7", m::Ordered, EDITOR),
        KeyBinding::new("ctrl-shift-8", m::Bullet, EDITOR),
        KeyBinding::new("ctrl-shift-9", m::Task, EDITOR),
        KeyBinding::new("ctrl-&", m::Ordered, EDITOR),
        KeyBinding::new("ctrl-*", m::Bullet, EDITOR),
        KeyBinding::new("ctrl-(", m::Task, EDITOR),
        KeyBinding::new("ctrl-enter", m::ToggleTask, EDITOR),
        KeyBinding::new("ctrl-shift-b", m::Quote, EDITOR),
        // Code Fences in Typora and other Linux Markdown editors; no three-modifier chord.
        KeyBinding::new("ctrl-shift-k", m::CodeBlock, EDITOR),
        KeyBinding::new("ctrl-e", UseSelectionForFind, EDITOR),
        KeyBinding::new("ctrl-=", reading(1., false), EDITOR),
        KeyBinding::new("ctrl-+", reading(1., false), EDITOR),
        KeyBinding::new("ctrl--", reading(-1., false), EDITOR),
        KeyBinding::new("ctrl-0", reading(0., true), EDITOR),
        KeyBinding::new("ctrl-alt-0", m::Paragraph, EDITOR),
        KeyBinding::new("ctrl-alt-1", m::Heading, EDITOR),
        KeyBinding::new("ctrl-alt-2", m::Heading2, EDITOR),
        KeyBinding::new("ctrl-alt-3", m::Heading3, EDITOR),
        KeyBinding::new("ctrl-alt-4", m::Heading4, EDITOR),
        KeyBinding::new("ctrl-alt-5", m::Heading5, EDITOR),
        KeyBinding::new("ctrl-alt-6", m::Heading6, EDITOR),
        KeyBinding::new("ctrl-shift-c", m::Code, EDITOR),
        KeyBinding::new("ctrl-shift-x", m::Strikethrough, EDITOR),
        // App-wide shortcuts (Ctrl+Q/S/N/W are already bound on every platform).
        KeyBinding::new("ctrl-f", Find, None),
        // Replace is Ctrl+H in GNOME and KDE editors.
        KeyBinding::new("ctrl-h", FindAndReplace, None),
        KeyBinding::new("ctrl-,", GlassTransparency, None),
        KeyBinding::new("ctrl-g", FindNext, None),
        KeyBinding::new("ctrl-shift-g", FindPrevious, None),
        KeyBinding::new("ctrl-e", UseSelectionForFind, None),
        KeyBinding::new("ctrl-j", JumpToSelection, None),
        // Stickies' Collapse and Zoom; the window manager owns Minimize.
        KeyBinding::new("ctrl-m", ToggleCollapse, None),
        KeyBinding::new("ctrl-shift-m", Zoom, None),
        // Ctrl+Alt+T opens a terminal on GNOME and Ubuntu.
        KeyBinding::new("ctrl-shift-t", ToggleTranslucent, None),
        KeyBinding::new("f1", Help, None),
        KeyBinding::new("ctrl-1", SetColor { color: ColorIndex::ALL[0] }, Some("DahliaNote")),
        KeyBinding::new("ctrl-2", SetColor { color: ColorIndex::ALL[1] }, Some("DahliaNote")),
        KeyBinding::new("ctrl-3", SetColor { color: ColorIndex::ALL[2] }, Some("DahliaNote")),
        KeyBinding::new("ctrl-4", SetColor { color: ColorIndex::ALL[3] }, Some("DahliaNote")),
        KeyBinding::new("ctrl-5", SetColor { color: ColorIndex::ALL[4] }, Some("DahliaNote")),
        KeyBinding::new("ctrl-6", SetColor { color: ColorIndex::ALL[5] }, Some("DahliaNote")),
        KeyBinding::new("ctrl-alt-[", ChangeTransparency { delta: 0.025 }, None),
        KeyBinding::new("ctrl-alt-]", ChangeTransparency { delta: -0.025 }, None),
    ]);
}

#[cfg(test)]
mod tests {
    #[test]
    fn note_shortcuts_require_a_note_editor_and_exclude_query_fields() {
        let filter = gpui::KeyBindingContextPredicate::parse(super::NOTE_EDITOR_CONTEXT).unwrap();
        let note = gpui::KeyContext::try_from("DahliaNote").unwrap();
        let editor = gpui::KeyContext::try_from("Markraft").unwrap();
        assert!(filter.eval(&[note.clone(), editor.clone()]));
        assert!(!filter.eval(std::slice::from_ref(&editor)));
        assert!(!filter.eval(&[note]));
        assert!(!filter.eval(&[gpui::KeyContext::try_from("FindControl").unwrap(), editor]));
    }

    /// What a keystroke does in a note's editor, or at the note level when `editor` is false.
    fn resolve(cx: &gpui::App, keys: &str, editor: bool) -> Option<Box<dyn gpui::Action>> {
        let mut stack = vec![gpui::KeyContext::try_from("DahliaNote").unwrap()];
        if editor {
            stack.push(gpui::KeyContext::try_from("Markraft").unwrap());
        }
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, _) =
            keymap.bindings_for_input(&[gpui::Keystroke::parse(keys).unwrap()], &stack);
        bindings.first().map(|binding| binding.action().boxed_clone())
    }

    #[gpui::test]
    fn shortcuts_follow_the_audit(cx: &mut gpui::TestAppContext) {
        use super::*;
        cx.update(|cx| {
            bind(cx);
            let is = |keys: &str, editor: bool, action: &dyn gpui::Action| {
                let found = resolve(cx, keys, editor);
                assert!(
                    found.as_ref().is_some_and(|found| found.partial_eq(action)),
                    "{keys} (editor: {editor}) is {:?}, expected {}",
                    found.map(|found| found.name()),
                    action.name()
                );
            };
            let color = |index: usize| SetColor {
                color: ColorIndex::ALL[index],
            };
            #[cfg(target_os = "macos")]
            {
                for (index, digit) in ["1", "2", "3", "4", "5", "6"].iter().enumerate() {
                    is(&format!("cmd-{digit}"), true, &color(index));
                    is(&format!("cmd-{digit}"), false, &color(index));
                }
                is("cmd-alt-1", true, &markraft_gpui::Heading);
                is("cmd-0", true, &ReadingSize { delta: 0., reset: true });
                is("cmd-m", true, &ToggleCollapse);
                is("cmd-shift-m", true, &Zoom);
                is("cmd-alt-m", true, &CollapseAll);
                is("cmd-alt-f", true, &ToggleFloat);
                is("cmd-alt-t", true, &ToggleTranslucent);
                is("cmd-alt-z", false, &UndoArrange);
                is("cmd-alt-shift-z", false, &RedoArrange);
                is("cmd-;", true, &CheckDocument);
                is("cmd-?", false, &Help);
                is("cmd-alt-c", true, &markraft_gpui::CodeBlock);
                is("cmd-alt-shift-v", true, &markraft_gpui::PastePlain);
                is("cmd-shift-v", true, &markraft_gpui::PastePlain);
                is("cmd-e", true, &UseSelectionForFind);
            }
            #[cfg(not(target_os = "macos"))]
            {
                for (index, digit) in ["1", "2", "3", "4", "5", "6"].iter().enumerate() {
                    is(&format!("ctrl-{digit}"), true, &color(index));
                }
                is("ctrl-alt-1", true, &markraft_gpui::Heading);
                is("ctrl-m", true, &ToggleCollapse);
                is("ctrl-shift-m", true, &Zoom);
                is("ctrl-h", true, &FindAndReplace);
                is("ctrl-h", false, &FindAndReplace);
                is("ctrl-shift-k", true, &markraft_gpui::CodeBlock);
                is("ctrl-shift-l", true, &markraft_gpui::ChooseCodeLanguage);
                is("ctrl-shift-t", true, &ToggleTranslucent);
                is("ctrl-shift-v", true, &markraft_gpui::PastePlain);
                is("f1", false, &Help);
                is("f10", false, &crate::linux_menu::ToggleNoteMenu);
                // Nothing left on a desktop-reserved or three-modifier chord, and no Mac
                // Emacs keys in a Linux editor.
                for keys in [
                    "ctrl-alt-t",
                    "ctrl-alt-l",
                    "ctrl-alt-f",
                    "ctrl-alt-shift-c",
                    "ctrl-alt-shift-v",
                    "ctrl-d",
                    "ctrl-k",
                    "ctrl-p",
                ] {
                    assert!(
                        resolve(cx, keys, true).is_none(),
                        "{keys} is still bound in the editor"
                    );
                }
            }
        });
    }
}
