use crate::*;

#[derive(Clone, Default, PartialEq)]
pub(crate) struct FormatMenuState {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) code: bool,
    pub(crate) strike: bool,
    pub(crate) heading: Option<i64>,
}
pub(crate) fn menu_action(name: &'static str, action: impl Action, available: bool) -> MenuItem {
    // GPUI's native validation checks handlers and can override a disabled global action.
    if available {
        MenuItem::action(name, action)
    } else {
        MenuItem::action(name, Unavailable).disabled(true)
    }
}
/// The Window menu lists every open note, and at most this many others.
pub(crate) const WINDOW_MENU_NOTES: usize = 100;
/// Everything the menu bar shows. AppKit rebuilds the whole menu bar on every set, and that
/// grows with the note lists, so menus are rebuilt only when this changes.
#[derive(PartialEq)]
pub(crate) struct MenuInputs {
    pub(crate) notes: Vec<(String, String, NoteState)>,
    pub(crate) windows: Vec<String>,
    pub(crate) active: Option<(bool, bool, bool, appearance::NoteStyle)>,
    pub(crate) note_window_active: bool,
    pub(crate) format: FormatMenuState,
    pub(crate) arrange: (bool, bool, window_commands::Policy),
    pub(crate) importing: bool,
    pub(crate) deletes: (bool, bool),
    pub(crate) tint: (bool, bool),
    pub(crate) updates: bool,
}
impl MenuInputs {
    pub(crate) fn of(cx: &App) -> Self {
        let state = cx.global::<AppState>();
        let notebook = state.notebook.borrow();
        let active = state.active.as_ref().and_then(|id| {
            notebook
                .notes()
                .iter()
                .find(|note| &note.id == id)
                .map(|note| (note.open, note.collapsed, note.floating, note.style.clone()))
        });
        let tint = glass_tint(cx);
        Self {
            notes: notebook
                .notes()
                .iter()
                .map(|note| (note.id.clone(), note.title.clone(), note.state))
                .collect(),
            windows: state.windows.keys().cloned().collect(),
            active,
            note_window_active: cx
                .active_window()
                .is_none_or(|window| window.downcast::<Note>().is_some()),
            format: state
                .active
                .as_ref()
                .and_then(|id| state.format_menus.get(id))
                .cloned()
                .unwrap_or_default(),
            arrange: (
                state.arrange_undo.is_empty(),
                state.arrange_redo.is_empty(),
                state.notebook.borrow().arrange_policy(),
            ),
            importing: state.import_task.is_some(),
            deletes: (
                operations::undo_id(cx).is_some(),
                operations::redo_id(cx).is_some(),
            ),
            tint: (tint >= appearance::MAX_TINT, tint <= 0.),
            updates: updates::available(),
        }
    }
}
pub(crate) fn update_menus(cx: &mut App) {
    let inputs = MenuInputs::of(cx);
    if cx.global::<AppState>().menus.as_ref() == Some(&inputs) {
        return;
    }
    cx.global_mut::<AppState>().menus = Some(inputs);
    #[cfg(test)]
    {
        cx.global_mut::<AppState>().menu_builds += 1;
    }
    cx.set_menus(app_menus(cx));
}
/// The app's menus, built from current state. macOS shows them in the menu bar; platforms
/// without one render the same tree inside each note (`linux_menu`).
pub(crate) fn app_menus(cx: &App) -> Vec<Menu> {
    let notes = cx.global::<AppState>().notebook.borrow().notes().to_vec();
    let policy = cx.global::<AppState>().notebook.borrow().arrange_policy();
    let titles = menu_titles(&notes);
    let active = cx
        .global::<AppState>()
        .active
        .as_ref()
        .and_then(|id| notes.iter().find(|note| &note.id == id));
    let format_enabled = active.is_some_and(|note| note.open && !note.collapsed)
        && cx
            .active_window()
            .is_none_or(|window| window.downcast::<Note>().is_some());
    let format_state = cx
        .global::<AppState>()
        .active
        .as_ref()
        .and_then(|id| cx.global::<AppState>().format_menus.get(id))
        .cloned()
        .unwrap_or_default();
    let heading = format_state.heading;
    let format = Menu::new("Format").items([
        menu_action("Bold", markraft_gpui::Bold, format_enabled).checked(format_state.bold),
        menu_action("Italic", markraft_gpui::Italic, format_enabled).checked(format_state.italic),
        menu_action("Inline Code", markraft_gpui::Code, format_enabled).checked(format_state.code),
        menu_action(
            "Strikethrough",
            markraft_gpui::Strikethrough,
            format_enabled,
        )
        .checked(format_state.strike),
        MenuItem::separator(),
        menu_action("Title", markraft_gpui::Heading, format_enabled).checked(heading == Some(1)),
        menu_action("Heading", markraft_gpui::Heading2, format_enabled).checked(heading == Some(2)),
        menu_action("Subheading", markraft_gpui::Heading3, format_enabled)
            .checked(heading == Some(3)),
        menu_action("Body", markraft_gpui::Paragraph, format_enabled),
        MenuItem::submenu(
            Menu::new("More Heading Levels").items([
                menu_action("Heading 4", markraft_gpui::Heading4, format_enabled)
                    .checked(heading == Some(4)),
                menu_action("Heading 5", markraft_gpui::Heading5, format_enabled)
                    .checked(heading == Some(5)),
                menu_action("Heading 6", markraft_gpui::Heading6, format_enabled)
                    .checked(heading == Some(6)),
            ]),
        ),
        MenuItem::separator(),
        menu_action("Bullet List", markraft_gpui::Bullet, format_enabled),
        menu_action("Numbered List", markraft_gpui::Ordered, format_enabled),
        menu_action("Task List", markraft_gpui::Task, format_enabled),
        menu_action("Mark as Done", markraft_gpui::ToggleTask, format_enabled),
        menu_action("Quote", markraft_gpui::Quote, format_enabled),
        menu_action("Code Block", markraft_gpui::CodeBlock, format_enabled),
    ]);
    let collapse_label = if active.is_some_and(|note| note.collapsed) {
        "Expand"
    } else {
        "Collapse"
    };
    let mut window_items = vec![
        MenuItem::action(collapse_label, ToggleCollapse),
        MenuItem::action("Minimize", Minimize),
        MenuItem::action("Zoom", Zoom),
        MenuItem::separator(),
        menu_action(
            "Collapse All",
            CollapseAll,
            !cx.global::<AppState>().windows.is_empty(),
        ),
        menu_action(
            "Expand All",
            ExpandAll,
            !cx.global::<AppState>().windows.is_empty(),
        ),
        menu_action(
            "Minimize All",
            MinimizeAll,
            !cx.global::<AppState>().windows.is_empty(),
        ),
        menu_action(
            "Zoom All",
            ZoomAll,
            !cx.global::<AppState>().windows.is_empty(),
        ),
    ];
    if platform::can_arrange() {
        window_items.extend([
            menu_action(
                "Undo Arrange",
                UndoArrange,
                !cx.global::<AppState>().arrange_undo.is_empty(),
            ),
            menu_action(
                "Redo Arrange",
                RedoArrange,
                !cx.global::<AppState>().arrange_redo.is_empty(),
            ),
            MenuItem::submenu(
                Menu::new("Arrange By").items([
                    MenuItem::action(
                        "Color",
                        window_commands::Arrange {
                            order: window_commands::Order::Color,
                        },
                    ),
                    MenuItem::action(
                        "Content",
                        window_commands::Arrange {
                            order: window_commands::Order::Content,
                        },
                    ),
                    MenuItem::action(
                        "Date",
                        window_commands::Arrange {
                            order: window_commands::Order::Date,
                        },
                    ),
                    MenuItem::action(
                        "Location on Screen",
                        window_commands::Arrange {
                            order: window_commands::Order::Location,
                        },
                    ),
                    MenuItem::separator(),
                    MenuItem::submenu(
                        Menu::new("When Arranging").items([
                            MenuItem::action(
                                "Collapse Notes",
                                window_commands::SetPolicy {
                                    policy: window_commands::Policy::Collapse,
                                },
                            )
                            .checked(
                                policy == window_commands::Policy::Collapse,
                            ),
                            MenuItem::action(
                                "Expand Notes",
                                window_commands::SetPolicy {
                                    policy: window_commands::Policy::Expand,
                                },
                            )
                            .checked(
                                policy == window_commands::Policy::Expand,
                            ),
                            MenuItem::action(
                                "Leave as Is",
                                window_commands::SetPolicy {
                                    policy: window_commands::Policy::Keep,
                                },
                            )
                            .checked(
                                policy == window_commands::Policy::Keep,
                            ),
                        ]),
                    ),
                ]),
            ),
        ]);
    }
    if platform::can_toggle_float() {
        window_items.push(
            MenuItem::action("Float on Top", ToggleFloat)
                .checked(active.is_some_and(|n| n.floating)),
        );
    }
    window_items.push(
        MenuItem::action("Translucent", ToggleTranslucent)
            .checked(active.is_some_and(|n| n.style.translucent)),
    );
    window_items.push(MenuItem::separator());
    let mut others = 0;
    window_items.extend(
        notes
            .iter()
            .filter(|note| note.state == NoteState::Active)
            .filter(|note| {
                cx.global::<AppState>().windows.contains_key(&note.id) || {
                    others += 1;
                    others <= WINDOW_MENU_NOTES
                }
            })
            .map(|note| {
                MenuItem::action(
                    titles[&note.id].clone(),
                    OpenNote {
                        id: note.id.clone(),
                    },
                )
            }),
    );
    let mut app_items = vec![MenuItem::action("About Dahlia", About)];
    // Mac convention puts the update check straight after About.
    app_items.extend(updates::menu_item(
        cfg!(target_os = "macos"),
        updates::available(),
    ));
    app_items.extend([
        MenuItem::separator(),
        MenuItem::action("Settings…", GlassTransparency),
        MenuItem::separator(),
    ]);
    if platform::has_app_hiding() {
        // Services and app hiding are AppKit's; Linux desktops have neither.
        app_items.extend([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Dahlia", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
        ]);
    }
    app_items.push(MenuItem::action("Quit Dahlia", Quit));
    vec![
        Menu::new("Dahlia").items(app_items),
        Menu {
            name: "File".into(),
            disabled: false,
            items: vec![
                MenuItem::action("New Note", NewNote),
                MenuItem::action("Save", Save),
                MenuItem::action("Close", CloseNote),
                MenuItem::separator(),
                MenuItem::action("Use as Default", UseAsDefault),
                MenuItem::separator(),
                MenuItem::action("Import Notes…", ImportNotes)
                    .disabled(cx.global::<AppState>().import_task.is_some()),
                MenuItem::action("Insert Images…", ImportImages),
                MenuItem::action("Export Note…", ExportNote),
                MenuItem::action("Export All Notes…", ExportAll),
                MenuItem::action("Recovery", ShowRecovery),
                MenuItem::action("Save Snapshot", CreateCheckpoint),
                archived_menu(
                    Menu::new("Archived Notes").items(
                        notes
                            .iter()
                            .rev()
                            .filter(|n| n.state == NoteState::Archived)
                            .map(|n| {
                                MenuItem::action(
                                    titles[&n.id].clone(),
                                    RestoreNote {
                                        id: n.id.clone(),
                                        expected: NoteState::Archived,
                                    },
                                )
                            }),
                    ),
                ),
                archived_menu(
                    Menu::new("Trash").items(
                        notes
                            .iter()
                            .rev()
                            .filter(|n| n.state == NoteState::Trash)
                            .map(|n| {
                                MenuItem::action(
                                    titles[&n.id].clone(),
                                    RestoreNote {
                                        id: n.id.clone(),
                                        expected: NoteState::Trash,
                                    },
                                )
                            }),
                    ),
                ),
            ],
        },
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", markraft_gpui::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", markraft_gpui::Redo, OsAction::Redo),
            menu_action(
                "Undo Delete Note",
                UndoDeleteNote,
                operations::undo_id(cx).is_some(),
            ),
            menu_action(
                "Redo Delete Note",
                RedoDeleteNote,
                operations::redo_id(cx).is_some(),
            ),
            MenuItem::separator(),
            MenuItem::os_action("Cut", markraft_gpui::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", markraft_gpui::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", markraft_gpui::Paste, OsAction::Paste),
            MenuItem::action("Paste and Match Style", markraft_gpui::PastePlain),
            MenuItem::os_action("Select All", markraft_gpui::SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Find").items([
                MenuItem::action("Find…", Find),
                MenuItem::action("Find and Replace…", FindAndReplace),
                MenuItem::action("Find Next", FindNext),
                MenuItem::action("Find Previous", FindPrevious),
                MenuItem::action("Use Selection for Find", UseSelectionForFind),
                MenuItem::action("Jump to Selection", JumpToSelection),
            ])),
            MenuItem::submenu(
                Menu::new("Spelling and Grammar").items(
                    [menu_action(
                        "Check Document Now",
                        CheckDocument,
                        checking::Checking::native_supported(),
                    )]
                    .into_iter()
                    .chain([MenuItem::separator()])
                    .chain(checking_items(&checking::Setting::ALL[..3], active)),
                ),
            ),
            MenuItem::submenu(
                Menu::new("Substitutions")
                    .items(checking_items(&checking::Setting::ALL[3..], active)),
            ),
        ]),
        Menu::new("Color").items(appearance::COLORS.iter().zip(ColorIndex::ALL).map(
            |(name, color)| {
                MenuItem::action(*name, SetColor { color })
                    .checked(active.is_some_and(|n| n.style.color == color))
            },
        )),
        Menu::new("Appearance").items([
            MenuItem::action(
                "Classic",
                SetAppearance {
                    appearance: Appearance::Classic,
                },
            )
            .checked(active.is_some_and(|n| n.style.appearance == Appearance::Classic)),
            MenuItem::action(
                "Glass",
                SetAppearance {
                    appearance: Appearance::Glass,
                },
            )
            .checked(active.is_some_and(|n| n.style.appearance == Appearance::Glass)),
            MenuItem::action(
                "Sea Glass",
                SetAppearance {
                    appearance: Appearance::SeaGlass,
                },
            )
            .checked(active.is_some_and(|n| n.style.appearance == Appearance::SeaGlass)),
            MenuItem::separator(),
            MenuItem::action("Transparency Settings…", GlassTransparency),
            MenuItem::action(
                "Decrease Transparency",
                ChangeTransparency { delta: 0.025 },
            )
            .disabled(glass_tint(cx) >= appearance::MAX_TINT),
            MenuItem::action(
                "Increase Transparency",
                ChangeTransparency { delta: -0.025 },
            )
            .disabled(glass_tint(cx) <= 0.),
        ]),
        format,
        Menu::new("Font").items([
            MenuItem::action(
                "Bigger",
                ReadingSize {
                    delta: 1.,
                    reset: false,
                },
            ),
            MenuItem::action(
                "Smaller",
                ReadingSize {
                    delta: -1.,
                    reset: false,
                },
            ),
            MenuItem::action(
                "Actual Size",
                ReadingSize {
                    delta: 0.,
                    reset: true,
                },
            ),
        ]),
        Menu {
            name: "Window".into(),
            disabled: false,
            items: window_items,
        },
        Menu::new("Help").items([MenuItem::action("Dahlia Help", Help)]),
    ]
}
/// Menu labels by note id. A title shared with another note in the same list reads
/// "Title (2)", "Title (3)" and so on, so the entries can be told apart; the first keeps its name.
fn menu_titles(notes: &[NoteRecord]) -> std::collections::BTreeMap<String, String> {
    let mut titles = std::collections::BTreeMap::new();
    for state in [NoteState::Active, NoteState::Archived, NoteState::Trash] {
        let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for note in notes.iter().filter(|note| note.state == state) {
            let count = seen.entry(note.title.as_str()).or_default();
            *count += 1;
            let label = if *count == 1 {
                note.title.clone()
            } else {
                format!("{} ({count})", note.title)
            };
            titles.insert(note.id.clone(), label);
        }
    }
    titles
}
/// A submenu of notes, disabled while it has none rather than opening onto nothing.
pub(crate) fn archived_menu(menu: Menu) -> MenuItem {
    let empty = menu.items.is_empty();
    MenuItem::submenu(menu.disabled(empty))
}
pub(crate) fn checking_items(
    settings: &[checking::Setting],
    active: Option<&NoteRecord>,
) -> Vec<MenuItem> {
    settings
        .iter()
        .map(|&setting| {
            MenuItem::action(setting.label(), ToggleChecking { setting })
                .checked(active.is_some_and(|n| n.style.checking.enabled(setting)))
        })
        .collect()
}

#[cfg(test)]
mod title_tests {
    use super::{NoteRecord, NoteState, menu_titles};
    fn note(id: &str, title: &str, state: NoteState) -> NoteRecord {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "title": title,
            "open": true,
            "frame": { "x": 0., "y": 0., "width": 360., "height": 350. },
            "expanded_height": 350.,
            "collapsed": false,
            "floating": false,
            "state": state,
        }))
        .unwrap()
    }
    #[test]
    fn repeated_titles_are_numbered_only_within_their_own_list() {
        let notes = [
            note("a", "Groceries", NoteState::Active),
            note("b", "Ideas", NoteState::Active),
            note("c", "Groceries", NoteState::Active),
            note("d", "Groceries", NoteState::Active),
            note("e", "Ideas", NoteState::Archived),
            note("f", "Groceries", NoteState::Trash),
        ];
        let titles = menu_titles(&notes);
        let label = |id: &str| titles[id].as_str();
        assert_eq!(label("a"), "Groceries");
        assert_eq!(label("c"), "Groceries (2)");
        assert_eq!(label("d"), "Groceries (3)");
        assert_eq!(label("b"), "Ideas");
        assert_eq!(label("e"), "Ideas");
        assert_eq!(label("f"), "Groceries");
    }
}
