use crate::*;

/// Where a note's error came from, which decides the way out the note offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorSource {
    /// A save failed; Retry save is offered and a later successful save clears it.
    Save,
    /// note.md changed on disk; saving stays refused until the user picks a way out.
    Conflict,
    /// Anything else; only the user dismisses it.
    Other,
}
#[derive(Clone, Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct NoteError {
    pub(crate) message: String,
    pub(crate) origin: ErrorSource,
    #[source]
    pub(crate) cause: Option<Arc<std::io::Error>>,
}
// Equality describes the visible error; clones retain the separate diagnostic cause.
impl PartialEq for NoteError {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message && self.origin == other.origin
    }
}
impl NoteError {
    pub(crate) fn other(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            origin: ErrorSource::Other,
            cause: None,
        }
    }
    pub(crate) fn save(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            origin: ErrorSource::Save,
            cause: None,
        }
    }
    fn save_failure(error: std::io::Error) -> Self {
        Self {
            message: if storage::is_external_change(&error) {
                "Another app changed this note. Keep your version as a copy, or reload the other app’s version."
                    .to_owned()
            } else {
                copy::failed("Couldn’t save this note.", &error)
            },
            origin: if storage::is_external_change(&error) {
                ErrorSource::Conflict
            } else {
                ErrorSource::Save
            },
            cause: Some(Arc::new(error)),
        }
    }
}

/// A note that can't be written as Markdown; the renderer's wording goes to the log.
fn unwritable(error: impl std::fmt::Display) -> std::io::Error {
    log::warn!("Cannot render Markdown: {error}");
    std::io::Error::other("This note can’t be written as Markdown right now.")
}

/// Preserve the renderer's diagnostic source without changing the visible save wording.
#[derive(Debug, thiserror::Error)]
#[error("Couldn’t save this note.")]
struct SourceSaveError(#[source] markraft_commonmark::SourceError);

pub(crate) enum EditorLoad {
    Placeholder,
    Prepared(Box<markraft_gpui::PreparedSetup>),
}
pub(crate) struct Note {
    pub(crate) shell_focus: FocusHandle,
    pub(crate) editor: Entity<EditorView>,
    pub(crate) schema: Schema,
    pub(crate) source: Arc<SourceTrack>,
    pub(crate) writer: NoteWriter,
    pub(crate) record: NoteRecord,
    pub(crate) asset_directory: std::path::PathBuf,
    pub(crate) checker: checking::DocumentChecking,
    pub(crate) checking_task: Option<Task<checking::Scan>>,
    document_check: Option<Task<()>>,
    library: Option<Rc<RefCell<Notebook>>>,
    pub(crate) error: Option<NoteError>,
    /// The error VoiceOver was last told about.
    announced: platform::Announced,
    pub(crate) generation: u64,
    pub(crate) save_task: Option<Task<()>>,
    pub(crate) write_task: Option<Task<()>>,
    layout_task: Option<Task<()>>,
    /// Until then, window size and position changes are ours (zoom, collapse, arrange), not
    /// the user's hand on the edge or the strip.
    settling_until: Option<std::time::Instant>,
    pub(crate) close_task: Option<Task<()>>,
    pub(crate) redo_after_close: Option<Vec<String>>,
    pub(crate) transfer_task: Option<Task<()>>,
    format_state: Option<EditorFormatState>,
    /// The window is open but the note's source is still being read in the background:
    /// the editor is empty, accepts no edits, and nothing is saved.
    pub(crate) opening: bool,
    pub(crate) durability_lock: Arc<std::sync::atomic::AtomicBool>,
    _changes: Subscription,
    _quit: Subscription,
    _state: Subscription,
    _images: Task<()>,
    _bounds: Option<Subscription>,
    _activation: Option<Subscription>,
    #[cfg(not(target_os = "macos"))]
    pub(crate) menu: linux_menu::MenuState,
}
impl Note {
    #[cfg(test)]
    pub(crate) fn new(store: NoteStore, text: String, cx: &mut Context<Self>) -> Self {
        let frame = Frame::default();
        Self::new_record(
            store,
            parse_note(&commonmark_schema(), &text).unwrap(),
            NoteRecord {
                id: "test".into(),
                title: "Untitled".into(),
                open: true,
                frame,
                expanded_height: frame.height,
                collapsed: false,
                floating: false,
                style: appearance::NoteStyle::legacy(),
                state: NoteState::Active,
                state_changed_at: 0,
                source_base: None,
                modified_at: 0,
                zoom_restore: None,
            },
            None,
            cx,
        )
    }
    pub(crate) fn new_record(
        store: NoteStore,
        source: SourceDocument,
        record: NoteRecord,
        library: Option<Rc<RefCell<Notebook>>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(store, source, record, library, false, cx)
    }
    /// A note whose window shows before its source is read; `placeholder` is an empty
    /// document until `finish_opening` brings the real one.
    pub(crate) fn opening(
        store: NoteStore,
        placeholder: SourceDocument,
        record: NoteRecord,
        library: Option<Rc<RefCell<Notebook>>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(store, placeholder, record, library, true, cx)
    }
    pub(crate) fn build(
        store: NoteStore,
        source: SourceDocument,
        mut record: NoteRecord,
        library: Option<Rc<RefCell<Notebook>>>,
        opening: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        #[cfg(test)]
        cx.background_executor().allow_parking();
        let schema = commonmark_schema();
        let source = Arc::new(SourceTrack::new(source));
        if !opening {
            record.title = title_of(&schema, source.origin().document());
        }
        let image_base = record
            .source_base
            .clone()
            .unwrap_or_else(|| store.directory().to_owned());
        let asset_directory = store.directory().to_owned();
        let durability_lock = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let editor = Self::editor_for(
            &schema,
            &source,
            &record,
            image_base,
            if opening {
                EditorLoad::Placeholder
            } else {
                EditorLoad::Prepared(Box::new(editor_setup(&schema, source.origin()).prepare()))
            },
            durability_lock.clone(),
            cx,
        );
        let (changes, state) = Self::watch(&editor, cx);
        let quit = cx.on_app_quit(|note, cx| {
            let saved = note.request_save(cx);
            let rescue = note.rescue_unsaved(cx);
            async move {
                if let Err(error) = saved.await {
                    log::error!("{}", error.message);
                    rescue();
                }
            }
        });
        let images = cx.spawn(async move |note, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if note
                    .update(cx, |note, cx| {
                        note.editor
                            .update(cx, |editor, cx| editor.refresh_images(cx))
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut checker = checking::DocumentChecking::new();
        let checking_task = (!opening).then(|| checker.refresh(&editor, record.style.checking, cx));
        Self {
            shell_focus: cx.focus_handle(),
            editor,
            schema,
            source,
            writer: NoteWriter::new(store),
            record,
            asset_directory,
            checker,
            checking_task,
            document_check: None,
            library,
            error: None,
            announced: platform::Announced::default(),
            generation: 0,
            save_task: None,
            write_task: None,
            layout_task: None,
            settling_until: None,
            close_task: None,
            redo_after_close: None,
            transfer_task: None,
            format_state: None,
            opening,
            durability_lock,
            _changes: changes,
            _quit: quit,
            _state: state,
            _images: images,
            _bounds: None,
            _activation: None,
            #[cfg(not(target_os = "macos"))]
            menu: linux_menu::MenuState::new(cx),
        }
    }
    fn editor_for(
        schema: &Schema,
        source: &Arc<SourceTrack>,
        record: &NoteRecord,
        image_base: std::path::PathBuf,
        load: EditorLoad,
        durability_lock: Arc<std::sync::atomic::AtomicBool>,
        cx: &mut Context<Self>,
    ) -> Entity<EditorView> {
        let (opening, prepared) = match load {
            EditorLoad::Placeholder => (true, editor_setup(schema, source.origin()).prepare()),
            EditorLoad::Prepared(prepared) => (false, *prepared),
        };
        let guard_source = source.clone();
        let guard_schema = schema.clone();
        let style = appearance::editor_style(&record.style, &fonts(cx));
        cx.new(|cx| {
            EditorView::from_prepared(prepared, cx)
            .with_style(style)
            .with_messages(copy::editor_messages())
            .with_aria_label(record.title.clone())
            .with_file_paste(true)
            // Note content decides which images load, so keep them inside the note's folder.
            .with_image_base(Some(image_base.clone()))
            .with_image_root(Ok(Some(image_base)))
            .with_image_confinement(true)
            .with_transaction_guard(move |transactions| {
                if durability_lock.load(std::sync::atomic::Ordering::Acquire)
                    && transactions
                        .iter()
                        .any(|transaction| transaction.doc_changed())
                {
                    return Err(EditRejection::Protected(
                        "This note is finishing a save. Try again when it finishes.".into(),
                    ));
                }
                // Typing into the empty stand-in would be saved over the real source.
                if opening {
                    return if transactions
                        .iter()
                        .any(|transaction| transaction.doc_changed())
                    {
                        Err(EditRejection::Protected(
                            "This note is still opening.".into(),
                        ))
                    } else {
                        Ok(())
                    };
                }
                guard_source
                    .apply_transactions(&guard_schema, transactions)
                    .map_err(|error| {
                        log::warn!("Edit refused by the source guard: {error}");
                        EditRejection::Protected(
                            "Dahlia can’t make this edit without rewriting other Markdown in the note."
                                .to_owned(),
                        )
                    })
            })
        })
    }
    fn watch(editor: &Entity<EditorView>, cx: &mut Context<Self>) -> (Subscription, Subscription) {
        let changes = cx.subscribe(editor, |note, _, event: &EditorEvent, cx| {
            if let EditorEvent::FilesPasted(item) = event {
                note.paste_images(item.clone(), cx);
            }
            if let EditorEvent::Changed { text_input, .. } = event {
                note.generation += 1;
                note.record.modified_at = storage::timestamp();
                let title = title_of(&note.schema, note.editor.read(cx).committed_document());
                if title != note.record.title {
                    note.editor
                        .update(cx, |editor, cx| editor.set_aria_label(title.clone(), cx));
                }
                note.record.title = title;
                if *text_input {
                    note.checker
                        .typed(&note.editor, note.record.style.checking, cx);
                }
                note.save_task = Some(cx.spawn(async move |note, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(400))
                        .await;
                    let _ = note.update(cx, |note, cx| {
                        note.autosave(cx);
                        note.checking_task = Some(note.checker.refresh(
                            &note.editor,
                            note.record.style.checking,
                            cx,
                        ));
                    });
                }));
                cx.notify();
            }
        });
        let state = cx.observe(editor, |note, editor, cx| {
            if let Some(error) = editor.update(cx, |editor, _| editor.take_edit_error()) {
                note.error = Some(NoteError::other(error.message().to_owned()));
                cx.notify();
            }
            let editor = editor.read(cx);
            let format_state = (editor.active_marks(), editor.active_block_type());
            if note.format_state.as_ref() != Some(&format_state) {
                note.format_state = Some(format_state);
                if cx.has_global::<AppState>() {
                    note.cache_format_menu(cx);
                    cx.defer(update_menus);
                }
            }
        });
        (changes, state)
    }
    /// Show the source read in the background in place of the empty stand-in.
    pub(crate) fn finish_opening_prepared(
        &mut self,
        source: SourceDocument,
        prepared: markraft_gpui::PreparedSetup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = Arc::new(SourceTrack::new(source));
        self.record.title = title_of(&self.schema, source.origin().document());
        let image_base = self
            .record
            .source_base
            .clone()
            .unwrap_or_else(|| self.asset_directory.clone());
        let focused = self.editor.read(cx).focus_handle(cx).is_focused(window);
        self.editor = Self::editor_for(
            &self.schema,
            &source,
            &self.record,
            image_base,
            EditorLoad::Prepared(Box::new(prepared)),
            self.durability_lock.clone(),
            cx,
        );
        self.source = source;
        (self._changes, self._state) = Self::watch(&self.editor, cx);
        self.opening = false;
        self.format_state = None;
        self.cache_format_menu(cx);
        self.checking_task = Some(self.checker.refresh(
            &self.editor,
            self.record.style.checking,
            cx,
        ));
        if focused {
            window.focus(&self.editor.read(cx).focus_handle(cx), cx);
        }
        if cx.has_global::<AppState>() {
            update_menus(cx);
            // Find's highlights and counts belong to the editor that was just replaced.
            cx.defer(crate::finding::refresh);
        }
        cx.notify();
    }
    /// The source could not be read as Markdown: close the window and leave the note closed,
    /// as opening it synchronously would have.
    pub(crate) fn fail_opening(
        &mut self,
        error: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.record.open = false;
        let placement = self.persist_record();
        let closed = self.writer.request_close();
        let id = self.record.id.clone();
        let handle = window
            .window_handle()
            .downcast::<Note>()
            .expect("note window");
        self.close_task = Some(cx.spawn(async move |_, cx| {
            let result = closed.await;
            let _ = handle.update(cx, |note, window, cx| {
                note.close_task.take();
                cx.global_mut::<AppState>().release_window(&id);
                note._bounds.take();
                note._activation.take();
                window.remove_window();
                cx.defer(move |cx| {
                    notify(cx, error);
                    if let Err(error) = placement {
                        notify(
                            cx,
                            copy::failed("Couldn’t save this note’s size and position.", &error),
                        );
                    }
                    if !matches!(result, Ok(Ok(()))) {
                        notify(
                            cx,
                            "Couldn’t confirm that this note’s last changes were saved. Open it from the Window menu and check its text.",
                        );
                    }
                    update_menus(cx);
                });
            });
        }));
    }
    pub(crate) fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cache_format_menu(cx);
        platform::apply_style(window, &self.record);
        window.set_window_title(&self.record.title);
        self._bounds = Some(cx.observe_window_bounds(window, |note, window, cx| {
            let before = note.record.frame;
            note.capture(window);
            // Dragging or resizing a zoomed note by hand leaves the zoomed frame: the next
            // Zoom then zooms again instead of jumping back to a frame the user moved away from.
            if note.record.zoom_restore.is_some()
                && manual_bounds_change(
                    before,
                    note.record.frame,
                    platform::primary_button_down(),
                    note.bounds_settling(),
                )
            {
                note.record.zoom_restore = None;
            }
            note.layout_task = Some(cx.spawn(async move |note, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let _ = note.update(cx, |note, cx| {
                    if let Err(error) = note.persist_record() {
                        note.error = Some(NoteError::save(copy::failed(
                            "Couldn’t save this note’s size and position.",
                            &error,
                        )));
                        cx.notify();
                    }
                });
            }));
        }));
        self._activation = Some(cx.observe_window_activation(window, |note, window, cx| {
            // Reduce Transparency, Increase Contrast and the highlight colour are system
            // settings, changed in another app. `app::refresh_appearance` restyles every
            // note when the system says so; this catches one that was missed.
            note.refresh_system_appearance(window, cx);
            if window.is_window_active() {
                cx.global_mut::<AppState>().active = Some(note.record.id.clone());
                update_menus(cx);
            }
        }));
    }
    pub(crate) fn capture(&mut self, window: &Window) {
        self.record.frame = platform::note_frame(window);
        if self.record.collapsed {
            self.record.frame.height = HEIGHT;
        } else {
            self.record.expanded_height = self.record.frame.height;
        }
    }
    fn cache_format_menu(&self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let marks = editor.active_marks();
        let types = DocTypes::from_schema_names(&self.schema, &commonmark_doc_type_names());
        let marked =
            |ty: Option<markraft_core::MarkTypeId>| ty.is_some_and(|ty| marks.contains_type(ty));
        let heading = editor.active_block_type().and_then(|(ty, attrs)| {
            (Some(ty) == types.heading)
                .then(|| attrs.get("level").and_then(|value| value.as_int()))
                .flatten()
        });
        let state = FormatMenuState {
            bold: marked(types.strong),
            italic: marked(types.em),
            code: marked(types.code),
            strike: marked(types.strikethrough),
            heading,
        };
        if cx.has_global::<AppState>() {
            cx.global_mut::<AppState>()
                .format_menus
                .insert(self.record.id.clone(), state);
        }
    }
    #[cfg(test)]
    pub(crate) fn source_text(&self, cx: &App) -> Result<String, String> {
        if self.opening {
            return Err("This note is still opening.".into());
        }
        self.source
            .snapshot()
            .render(&self.schema, self.editor.read(cx).committed_document())
            .map_err(|error| {
                log::warn!("Cannot render Markdown: {error}");
                "Couldn’t save this note.".to_owned()
            })
    }
    fn paste_images(&mut self, item: ClipboardItem, cx: &mut Context<Self>) {
        if self.transfer_task.is_some() {
            return;
        }
        if self.record.source_base.is_some() {
            self.error = Some(NoteError::other(
                "Images can’t be added to this note. Export it, then import the exported file.",
            ));
            cx.notify();
            return;
        }
        let directory = self.asset_directory.clone();
        let revision = self.generation;
        let selection = self.editor.read(cx).state().selection().clone();
        self.transfer_task = Some(cx.spawn(async move |note, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (order, images) = attachments::clipboard_images(item)?;
                    attachments::publish(&directory, &images)?;
                    Ok::<_, std::io::Error>(
                        order
                            .iter()
                            .map(|r| format!("![image]({r})"))
                            .collect::<Vec<_>>()
                            .join("\n\n"),
                    )
                })
                .await;
            let _ = note.update(cx, |note, cx| {
                note.transfer_task.take();
                let result = result.map_err(|e| copy::plain(&e)).and_then(|markdown| {
                    if note.generation != revision
                        || *note.editor.read(cx).state().selection() != selection
                        || note.editor.read(cx).is_composing()
                    {
                        return Err(
                            "The note changed before the images were ready. Add them again.".into(),
                        );
                    }
                    let slice =
                        markraft_commonmark::from_markdown_fragment(&note.schema, &markdown)
                            .map_err(|error| {
                                log::warn!("Cannot insert pasted images: {error}");
                                "These images can’t be inserted at this position. Move the cursor and try again.".to_owned()
                            })?;
                    let applied = note.editor.update(cx, |editor, cx| {
                        let command = markraft_core::commands::replace_selection(slice);
                        command(editor.state())
                            .is_some_and(|spec| editor.dispatch_isolated([spec], cx))
                    });
                    if applied {
                        Ok(())
                    } else {
                        Err("These images can’t be inserted at this position. Move the cursor and try again.".into())
                    }
                });
                if let Err(error) = result {
                    note.error = Some(NoteError::other(error));
                }
                cx.notify();
            });
        }));
    }
    pub(crate) fn persist_record(&self) -> std::io::Result<()> {
        if let Some(library) = &self.library {
            library.borrow_mut().update(self.record.clone())?;
        }
        Ok(())
    }
    fn import_images(&mut self, cx: &mut Context<Self>) {
        if self.transfer_task.is_some() {
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Insert".into()),
        });
        self.transfer_task = Some(cx.spawn(async move |note, cx| {
            let selected = picker.await;
            let _ = note.update(cx, |note, cx| {
                note.transfer_task.take();
                match selected {
                    Ok(Ok(Some(paths))) => note.paste_images(
                        ClipboardItem {
                            entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
                                paths.into(),
                            ))],
                        },
                        cx,
                    ),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        log::warn!("Image picker failed: {error}");
                        note.error = Some(NoteError::other("Couldn’t open the image picker."));
                        cx.notify();
                    }
                    Err(_) => {
                        note.error = Some(NoteError::other("Couldn’t open the image picker."));
                        cx.notify();
                    }
                }
            });
        }));
    }
    pub(crate) fn check_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening {
            return;
        }
        let scan = self
            .checker
            .check_all(&self.editor, self.record.style.checking, cx);
        let handle = window.window_handle().downcast::<Note>().expect("note");
        // Its own slot: the autosave timer replaces `checking_task`, which would drop this.
        self.document_check = Some(cx.spawn(async move |_, cx| {
            let outcome = scan.await;
            let prompt = handle
                .update(cx, |note, window, cx| {
                    let issue =
                        note.checker
                            .results(note.editor.read(cx))
                            .into_iter()
                            .find(|check| {
                                matches!(
                                    check.kind,
                                    checking::Kind::Spelling | checking::Kind::Grammar
                                )
                            });
                    let Some(issue) = issue else {
                        // Only a complete scan may say that nothing was found.
                        let (title, detail) = match outcome {
                            checking::Scan::Complete => ("No spelling or grammar issues found.", None),
                            checking::Scan::Superseded => (
                                "The note changed while it was being checked.",
                                Some("Choose Check Document Now to try again."),
                            ),
                            checking::Scan::Unavailable => (
                                "Spelling and grammar checking isn’t available on this Mac.",
                                None,
                            ),
                        };
                        drop(window.prompt(PromptLevel::Info, title, detail, &["OK"], cx));
                        return None;
                    };
                    let request = note
                        .editor
                        .read(cx)
                        .context_source_range_snapshot(issue.range)?;
                    let word = note.editor.read(cx).context_text(&request)?.text;
                    let mut suggestions = issue.suggestions;
                    if let Some(replacement) = issue.replacement {
                        suggestions.insert(0, replacement);
                    }
                    if suggestions.is_empty() && issue.kind == checking::Kind::Spelling {
                        suggestions = note.checker.guesses(&word);
                    }
                    suggestions.truncate(3);
                    let mut labels: Vec<_> = suggestions
                        .iter()
                        .map(|s| format!("Replace with {s}"))
                        .collect();
                    let ignore = issue.kind == checking::Kind::Spelling;
                    if ignore {
                        labels.push("Ignore Word".into());
                    }
                    labels.push("Cancel".into());
                    // Name the word; a grammar issue says what is wrong with it.
                    let (title, detail) = match issue.detail {
                        Some(description) => (description, Some(format!("“{word}”"))),
                        None => (format!("“{word}” may be misspelled."), None),
                    };
                    let buttons: Vec<_> = labels.iter().map(String::as_str).collect();
                    let response =
                        window.prompt(PromptLevel::Info, &title, detail.as_deref(), &buttons, cx);
                    Some((response, request, word, suggestions, ignore))
                })
                .ok()
                .flatten();
            let Some((response, request, word, suggestions, ignore)) = prompt else {
                return;
            };
            if let Ok(choice) = response.await {
                let _ = handle.update(cx, |note, _, cx| {
                    if let Some(replacement) = suggestions.get(choice) {
                        let replaced = note.editor.update(cx, |editor, cx| {
                            editor.replace_context_text(&request, replacement, cx)
                        });
                        if !replaced {
                            note.error = Some(NoteError::other(
                                "The text changed, so the correction wasn’t made.",
                            ));
                        }
                    } else if ignore && choice == suggestions.len() {
                        note.checker.ignore(&word);
                    }
                    note.checking_task = Some(note.checker.refresh(
                        &note.editor,
                        note.record.style.checking,
                        cx,
                    ));
                    cx.notify();
                });
            }
        }));
    }
    /// Snapshot immutable editor state on the UI thread. Rendering, validation and the
    /// durable write run in writer order, independently of the GPUI task's lifetime.
    pub(crate) fn request_save(
        &self,
        cx: &App,
    ) -> impl std::future::Future<Output = Result<(), NoteError>> + use<> {
        let opening = self.opening;
        let source = self.source.snapshot();
        let schema = self.schema.clone();
        let document = self.editor.read(cx).committed_document().clone();
        let pending = (!opening).then(|| {
            self.writer.save_with(move || {
                source.render(&schema, &document).map_err(|error| {
                    log::warn!("Cannot render Markdown: {error}");
                    std::io::Error::other(SourceSaveError(error))
                })
            })
        });
        async move {
            match pending {
                None => Ok(()),
                Some(pending) => pending
                    .await
                    .map_err(|_| {
                        NoteError::save(
                            "Couldn’t save this note. Copy its text, then quit and reopen Dahlia.",
                        )
                    })?
                    .map_err(NoteError::save_failure),
            }
        }
    }
    /// For a quit that cannot be vetoed after this note's save was refused: keeps the text
    /// this note holds in Recovery, since it would be lost with the window. Render and write
    /// only if called, so a note that saved pays nothing.
    pub(crate) fn rescue_unsaved(&self, cx: &App) -> impl FnOnce() + use<> {
        let opening = self.opening;
        let id = self.record.id.clone();
        let root = self
            .library
            .as_ref()
            .map(|library| library.borrow().root().to_owned());
        let source = self.source.snapshot();
        let schema = self.schema.clone();
        let document = self.editor.read(cx).committed_document().clone();
        move || {
            let Some(root) = root.filter(|_| !opening) else {
                return;
            };
            match source.render(&schema, &document) {
                Ok(text) => {
                    if let Err(error) = storage::rescue(&root, &id, &text) {
                        log::error!("Could not keep the unsaved text of a note: {error}");
                    }
                }
                Err(error) => log::error!("Could not keep the unsaved text of a note: {error}"),
            }
        }
    }
    fn autosave(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        let pending = self.request_save(cx);
        self.write_task = Some(cx.spawn(async move |note, cx| {
            let result = pending.await;
            let _ = note.update(cx, |note, cx| {
                if generation != note.generation {
                    return;
                }
                let outcome = result.and_then(|()| {
                    note.persist_record().map_err(|error| {
                        NoteError::save(copy::failed("Couldn’t save this note’s settings.", &error))
                    })
                });
                note.settle_save(outcome);
                if cx.has_global::<AppState>() {
                    update_menus(cx);
                }
                cx.notify();
            });
        }));
    }
    #[cfg(test)]
    pub(crate) fn save(&mut self, cx: &App) -> bool {
        // Nothing was shown or edited yet, so the file on disk is the note.
        if self.opening {
            return true;
        }
        let outcome = self
            .source_text(cx)
            .map_err(NoteError::save)
            .and_then(|text| self.writer.flush(text).map_err(NoteError::save_failure))
            .and_then(|_| {
                self.persist_record().map_err(|error| {
                    NoteError::save(copy::failed("Couldn’t save this note’s settings.", &error))
                })
            });
        self.settle_save(outcome)
    }
    /// Record a save outcome. Success clears only an earlier save failure, never an
    /// unrelated error the user has not seen through.
    fn settle_save(&mut self, outcome: Result<(), NoteError>) -> bool {
        match outcome {
            Ok(()) => {
                if self
                    .error
                    .as_ref()
                    .is_some_and(|error| error.origin != ErrorSource::Other)
                {
                    self.error = None;
                }
                true
            }
            Err(error) => {
                log::warn!("{}", error.message);
                self.error = Some(error);
                false
            }
        }
    }
    /// The ways out offered under the error; Retry only makes sense for a failed save.
    pub(crate) fn error_actions(&self) -> &'static [&'static str] {
        match self.error.as_ref().map(|error| error.origin) {
            Some(ErrorSource::Save) => &[RETRY],
            Some(ErrorSource::Conflict) => &[KEEP_MINE, RELOAD],
            Some(ErrorSource::Other) => &[DISMISS],
            None => &[],
        }
    }
    /// What the collapsed strip shows: an error replaces the title until it is resolved.
    pub(crate) fn strip_label(&self) -> String {
        match &self.error {
            Some(error) if self.record.collapsed => format!("⚠︎ {}", error.message),
            _ => self.record.title.clone(),
        }
    }
    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_task.is_some() || self.transfer_task.is_some() {
            return;
        }
        // The editor is an empty stand-in until the real document arrives; Archive and Delete
        // would do nothing and Export would write an empty file.
        if self.opening {
            self.error = Some(NoteError::other("This note is still opening."));
            cx.notify();
            return;
        }
        if self.editor.read(cx).text().is_empty() {
            self.finish_close(NoteState::Archived, window, cx);
            return;
        }
        let answer = platform::close_prompt(&self.record.title, window, cx);
        let handle = window
            .window_handle()
            .downcast::<Note>()
            .expect("note window");
        self.close_task = Some(cx.spawn(async move |_, cx| {
            let choice = answer.await;
            let _ = handle.update(cx, |note, window, cx| {
                note.close_task.take();
                match choice {
                    platform::CloseChoice::Archive => {
                        note.finish_close(NoteState::Archived, window, cx)
                    }
                    platform::CloseChoice::Trash => note.finish_close(NoteState::Trash, window, cx),
                    platform::CloseChoice::ExportAndArchive => note.export(true, window, cx),
                    platform::CloseChoice::Cancel => {
                        window.activate_window();
                        let focus = if note.record.collapsed {
                            note.shell_focus.clone()
                        } else {
                            note.editor.read(cx).focus_handle(cx)
                        };
                        window.focus(&focus, cx);
                    }
                }
            });
        }));
    }
    pub(crate) fn finish_close(
        &mut self,
        destination: NoteState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.close_task.is_some() || self.opening || cx.global::<AppState>().quitting {
            return;
        }
        self.save_task.take();
        self.layout_task.take();
        self.capture(window);
        self.durability_lock
            .store(true, std::sync::atomic::Ordering::Release);
        let pending_save = self.request_save(cx);
        let handle = window
            .window_handle()
            .downcast::<Note>()
            .expect("note window");
        self.close_task = Some(cx.spawn(async move |_, cx| {
            let result: Result<(), NoteError> = async {
                pending_save.await?;
                let (library, pending) = handle
                    .update(cx, |note, window, _| {
                        note.capture(window);
                        let library = note
                            .library
                            .clone()
                            .ok_or_else(|| NoteError::other("Couldn’t reach Dahlia’s notes. Quit and reopen Dahlia."))?;
                        let pending = library
                            .borrow_mut()
                            .request_transition(note.record.clone(), NoteState::Active, destination)
                            .map_err(|error| {
                                NoteError::other(copy::failed("Couldn’t close this note.", &error))
                            })?;
                        Ok::<_, NoteError>((library, pending))
                    })
                    .map_err(|_| NoteError::other("Couldn’t close this note."))??;
                let completed = pending.wait().await;
                let record = cx
                    .update(|_| library.borrow_mut().complete(completed))
                    .map_err(|error| {
                        NoteError::other(copy::failed("Couldn’t close this note.", &error))
                    })?;
                let closed = handle
                    .update(cx, |note, _, _| note.writer.request_close())
                    .map_err(|_| NoteError::other("Couldn’t close this note."))?;
                closed
                    .await
                    .map_err(|_| {
                        NoteError::other(
                            "Couldn’t finish closing this note. Quit and reopen Dahlia, then check its text.",
                        )
                    })?
                    .map_err(|error| {
                        NoteError::other(copy::failed("Couldn’t close this note.", &error))
                    })?;
                handle
                    .update(cx, |note, window, cx| {
                        note.record = record;
                        if destination == NoteState::Trash {
                            let state = cx.global_mut::<AppState>();
                            state.undo_deletes.push(note.record.id.clone());
                            state.redo_deletes = note
                                .redo_after_close
                                .take()
                                .unwrap_or_default()
                                .into_iter()
                                .filter(|id| id != &note.record.id)
                                .collect();
                            if state.undo_deletes.len() > 20 {
                                state.undo_deletes.remove(0);
                            }
                        }
                        cx.global_mut::<AppState>()
                            .release_window(&note.record.id);
                        note._bounds.take();
                        note._activation.take();
                        update_menus(cx);
                        window.remove_window();
                    })
                    .map_err(|_| NoteError::other("Couldn’t close this note."))?;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                let _ = handle.update(cx, |note, _, cx| {
                    note.close_task.take();
                    note.redo_after_close.take();
                    note.durability_lock
                        .store(false, std::sync::atomic::Ordering::Release);
                    note.settle_save(Err(error));
                    cx.notify();
                });
            }
        }));
    }
    /// Preserve edits separately before releasing the conflicted source's lock.
    pub(crate) fn keep_mine_as_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_task.is_some() {
            return;
        }
        let Some(library) = self.library.clone() else {
            return;
        };
        self.durability_lock
            .store(true, std::sync::atomic::Ordering::Release);
        let source = self.source.snapshot();
        let schema = self.schema.clone();
        let document = self.editor.read(cx).committed_document().clone();
        let record = self.record.clone();
        let directory = self.asset_directory.clone();
        let handle = window
            .window_handle()
            .downcast::<Note>()
            .expect("note window");
        self.close_task = Some(cx.spawn(async move |_, cx| {
            let text = cx
                .background_executor()
                .spawn(async move {
                    source
                        .render(&schema, &document)
                        .map_err(unwritable)
                })
                .await;
            let result = match text
                .and_then(|text| library.borrow_mut().request_copy(&record, text, directory))
            {
                Ok(pending) => {
                    let completed = pending.wait().await;
                    library.borrow_mut().complete(completed)
                }
                Err(error) => Err(error),
            };
            let _ = handle.update(cx, |note, window, cx| {
                note.close_task.take();
                match result {
                    Ok(copy) => {
                        note.reopen_from_disk(window, cx);
                        cx.defer(move |cx| {
                            if let Err(error) = open_note(&copy.id, cx) {
                                notify(cx, error);
                            }
                        });
                    }
                    Err(error) => {
                        note.durability_lock
                            .store(false, std::sync::atomic::Ordering::Release);
                        note.error = Some(NoteError::other(copy::failed(
                            "Couldn’t save a copy.",
                            &error,
                        )));
                        cx.notify();
                    }
                }
            });
        }));
    }
    pub(crate) fn reload_from_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_task.is_some() {
            return;
        }
        self.durability_lock
            .store(true, std::sync::atomic::Ordering::Release);
        let source = self.source.snapshot();
        let schema = self.schema.clone();
        let document = self.editor.read(cx).committed_document().clone();
        let directory = self.asset_directory.clone();
        let handle = window
            .window_handle()
            .downcast::<Note>()
            .expect("note window");
        self.close_task = Some(cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let text = source
                        .render(&schema, &document)
                        .map_err(unwritable)?;
                    storage::preserve(&directory, &text)
                })
                .await;
            let _ = handle.update(cx, |note, window, cx| {
                note.close_task.take();
                match result {
                    Ok(()) => note.reopen_from_disk(window, cx),
                    Err(error) => {
                        note.durability_lock
                            .store(false, std::sync::atomic::Ordering::Release);
                        note.error = Some(NoteError::other(copy::failed(
                            "Couldn’t keep your edits, so the note wasn’t reloaded.",
                            &error,
                        )));
                        cx.notify();
                    }
                }
            });
        }));
    }
    /// Release the writer's lock asynchronously before replacing its window.
    fn reopen_from_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_task.take();
        self.write_task.take();
        self.layout_task.take();
        self.capture(window);
        let placement = self.persist_record();
        let closed = self.writer.request_close();
        let id = self.record.id.clone();
        let handle = window
            .window_handle()
            .downcast::<Note>()
            .expect("note window");
        self.close_task = Some(cx.spawn(async move |_, cx| {
            let result = closed
                .await
                .map_err(|_| {
                    std::io::Error::other(
                        "Couldn’t save this note. Copy its text, then quit and reopen Dahlia.",
                    )
                })
                .and_then(|result| result);
            let _ = handle.update(cx, |note, window, cx| {
                note.close_task.take();
                if let Err(error) = result {
                    note.durability_lock
                        .store(false, std::sync::atomic::Ordering::Release);
                    note.error = Some(NoteError::other(copy::plain(&error)));
                    cx.notify();
                    return;
                }
                cx.global_mut::<AppState>().release_window(&id);
                note._bounds.take();
                note._activation.take();
                window.remove_window();
                cx.defer(move |cx| {
                    if let Err(error) = placement {
                        notify(
                            cx,
                            copy::failed("Couldn’t save this note’s size and position.", &error),
                        );
                    }
                    if let Err(error) = open_note(&id, cx) {
                        notify(cx, error);
                    }
                });
            });
        }));
    }
    /// Window size and position changes for the next moment come from the app, not the user.
    /// A zoom or an animated restore reports many bounds while it runs.
    pub(crate) fn settle_bounds(&mut self) {
        self.settling_until = Some(std::time::Instant::now() + BOUNDS_SETTLE);
    }
    fn bounds_settling(&self) -> bool {
        self.settling_until
            .is_some_and(|until| std::time::Instant::now() < until)
    }
    /// Zoom toggles between the pre-zoom frame and the system zoom frame. Repeating
    /// `zoom_window` alone does not return here, because the custom masks and resize
    /// limits keep recomputing the standard frame. A collapsed strip is not resizable, so
    /// like AppKit's disabled zoom button it does not start a zoom; one zoomed before it
    /// collapsed can still be returned.
    pub(crate) fn toggle_zoom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.record.zoom_restore.is_none() {
            self.capture(window);
            if !self.record.begin_zoom() {
                return;
            }
            self.settle_bounds();
            window.zoom_window();
        } else if let Some(frame) = self.record.unzoom() {
            self.settle_bounds();
            // Glide back as the zoom glided out.
            platform::place(window, frame, true);
        }
        if let Err(error) = self.persist_record() {
            self.error = Some(NoteError::save(copy::failed(
                "Couldn’t save this note’s settings.",
                &error,
            )));
        }
        cx.notify();
    }
    pub(crate) fn collapse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.record.collapsed {
            self.editor.update(cx, |editor, cx| {
                if let Some(cancel) = markraft_core::composition::cancel_composition(editor.state())
                {
                    editor.dispatch([cancel], cx);
                }
            });
        }
        self.capture(window);
        self.settle_bounds();
        self.record.collapsed = !self.record.collapsed;
        let strip = self.record.frame;
        self.record.frame = if self.record.collapsed {
            Frame {
                height: HEIGHT,
                ..strip
            }
        } else {
            // A borderless note is not held on screen by AppKit: slide it up if it would
            // grow past the bottom of the display, and keep the top edge when it fits.
            strip.expanded_to(self.record.expanded_height, &displays(cx))
        };
        platform::configure(
            window,
            self.record.collapsed,
            self.record.floating,
            self.record.style.frosted(),
            Some((self.record.frame.width, self.record.frame.height)),
        );
        if self.record.frame.y != strip.y {
            platform::place(window, self.record.frame, false);
        }
        #[cfg(not(target_os = "macos"))]
        window.resize(size(
            px(self.record.frame.width),
            px(self.record.frame.height),
        ));
        if self.record.collapsed {
            window.focus(&self.shell_focus, cx);
        } else {
            window.focus(&self.editor.read(cx).focus_handle(cx), cx);
        }
        if let Err(error) = self.persist_record() {
            self.error = Some(NoteError::save(copy::failed(
                "Couldn’t save this note’s settings.",
                &error,
            )));
        }
        if cx.has_global::<AppState>() {
            update_menus(cx);
        }
        cx.notify();
    }
    fn toggle_float(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The menu offers Float on Top only where it works.
        if !platform::can_toggle_float() {
            return;
        }
        self.record.floating = !self.record.floating;
        // Panels already open must move with it; the notebook sees the change once saved.
        cx.defer(sync_panel_levels);
        platform::configure(
            window,
            self.record.collapsed,
            self.record.floating,
            self.record.style.frosted(),
            None,
        );
        if let Err(error) = self.persist_record() {
            self.error = Some(NoteError::save(copy::failed(
                "Couldn’t save this note’s settings.",
                &error,
            )));
        }
        cx.notify();
    }
    pub(crate) fn refresh_system_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let style = appearance::editor_style(&self.record.style, &fonts(cx));
        if self.editor.read(cx).style() != &style {
            self.editor
                .update(cx, |editor, cx| editor.set_style(style, cx));
        }
        platform::apply_style(window, &self.record);
        cx.notify();
    }
    pub(crate) fn restyle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.checking_task = Some(self.checker.refresh(
            &self.editor,
            self.record.style.checking,
            cx,
        ));
        let style = appearance::editor_style(&self.record.style, &fonts(cx));
        self.editor
            .update(cx, |editor, cx| editor.set_style(style, cx));
        platform::apply_style(window, &self.record);
        if let Err(error) = self.persist_record() {
            self.error = Some(NoteError::save(copy::failed(
                "Couldn’t save this note’s settings.",
                &error,
            )));
        }
        if cx.has_global::<AppState>() {
            update_menus(cx);
        }
        cx.notify();
    }
}
/// How long after a zoom, restore or collapse the window may still be reporting its own motion.
const BOUNDS_SETTLE: Duration = Duration::from_secs(1);
/// Whether a bounds change is the user dragging or resizing the window: the frame truly moved
/// or resized, the mouse button is down, and the app is not mid-way through a change of its own.
fn manual_bounds_change(before: Frame, after: Frame, button_down: bool, settling: bool) -> bool {
    let moved = |a: f32, b: f32| (a - b).abs() > 0.5;
    button_down
        && !settling
        && (moved(before.x, after.x)
            || moved(before.y, after.y)
            || moved(before.width, after.width)
            || moved(before.height, after.height))
}
/// Large sources open a protected placeholder while preparation runs in the background.
pub(crate) const BACKGROUND_PARSE_BYTES: usize = 256 * 1024;
pub(crate) fn editor_setup(schema: &Schema, source: &SourceDocument) -> Setup {
    Setup::new(schema.clone())
        .types(DocTypes::from_schema_names(
            schema,
            &commonmark_doc_type_names(),
        ))
        .kind(Arc::new(markdown::Markdown {
            schema: schema.clone(),
        }))
        .extensions(commonmark_extensions(schema))
        .doc(source.document().clone())
}
/// Parse a note's source; note content must never be able to panic the app.
pub(crate) fn parse_note(schema: &Schema, text: &str) -> Result<SourceDocument, String> {
    SourceDocument::parse(schema, text).map_err(|error| {
        log::warn!("Cannot parse a note: {error}");
        "This note couldn’t be read as Markdown. Export it from File ▸ Recovery.".to_owned()
    })
}
/// The first line a reader sees. Runs on every keystroke, so it reads blocks only until
/// one has text: the cost follows the first block, not the note.
pub(crate) fn title_of(schema: &Schema, doc: &markraft_core::Node) -> String {
    doc.children()
        .find_map(|block| {
            // Read each block as a document of its own, exactly as the whole note reads.
            let alone = doc.copy(markraft_core::Fragment::from_nodes([block.clone()]));
            let text = markraft_commonmark::to_plain_text(schema, &alone);
            text.lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| clip_title(line.trim()))
        })
        .unwrap_or_else(|| "Untitled".into())
}
/// Titles keep their first 60 characters; a longer line ends in an ellipsis so the strip and
/// the Window menu never show a word cut off mid-way as if it were the whole title.
pub(crate) const TITLE_CHARS: usize = 60;
pub(crate) fn clip_title(line: &str) -> String {
    let mut chars = line.chars();
    let mut title: String = chars.by_ref().take(TITLE_CHARS).collect();
    if chars.next().is_some() {
        // End on a whole word when one finishes in the last third.
        if let Some((cut, _)) = title
            .char_indices()
            .rev()
            .find(|&(at, c)| c.is_whitespace() && at >= title.len() * 2 / 3)
        {
            title.truncate(cut);
        }
        title.truncate(title.trim_end().len());
        title.push('…');
    }
    title
}
pub(crate) fn frame_of(bounds: Bounds<Pixels>) -> Frame {
    Frame {
        x: bounds.origin.x.into(),
        y: bounds.origin.y.into(),
        width: bounds.size.width.into(),
        height: bounds.size.height.into(),
    }
}
pub(crate) fn bounds_of(frame: Frame) -> Bounds<Pixels> {
    Bounds::new(
        point(px(frame.x), px(frame.y)),
        size(px(frame.width), px(frame.height)),
    )
}
pub(crate) fn displays(cx: &App) -> Vec<Frame> {
    if let Some(frames) = platform::display_frames().filter(|frames| !frames.is_empty()) {
        return frames;
    }
    cx.displays()
        .iter()
        .map(|display| frame_of(display.visible_bounds()))
        .collect()
}

impl Render for Note {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.record.title);
        // Errors appear as text only, so say them; every place that sets one is covered here.
        self.announced
            .say(self.error.as_ref().map_or("", |error| &error.message));
        let collapsed = self.record.collapsed;
        let style = &self.record.style;
        let sea = style.sea_glass();
        let frosted = style.frosted();
        let radius = style.corner_radius();
        let inset = style.edge_inset();
        let active = window.is_window_active();
        let opaque = platform::reduce_transparency();
        let tint = if opaque { 1. } else { glass_tint(cx) };
        let body = rgb(style.paper());
        let paper = if frosted && !opaque {
            let increase_contrast = platform::increase_contrast();
            Hsla::from(body).opacity(if sea {
                appearance::sea_paper_alpha(tint, increase_contrast)
            } else {
                appearance::glass_paper_alpha(tint, increase_contrast)
            })
        } else {
            body.into()
        };
        // Colour goes on a solid layer: Glass keeps each note's spine colour opaque in its
        // strip, a handle that tells six translucent notes apart, with a hairline below. Sea
        // Glass's strip is a pale veil of its edge colour over the pastel instead.
        let spine = if sea {
            Hsla::from(rgb(style.sea_edge())).opacity(chrome::SEA_STRIP_VEIL)
        } else {
            Hsla::from(rgb(style.spine()))
        };
        let hairline = if sea {
            Hsla::from(rgb(style.sea_ink())).opacity(0.2)
        } else {
            appearance::ink(0.12)
        };
        let ink = rgb(style.text_ink());
        div()
            .track_focus(&self.shell_focus)
            .key_context("DahliaNote")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(paper)
            .opacity(platform::content_opacity(&self.record.style))
            .rounded(px(radius))
            .overflow_hidden()
            .text_color(ink)
            .font_family(fonts(cx).prose)
            .when(sea, |root| {
                // A soft inner rim: light along the top edge, a faint shade along the bottom.
                root.shadow(vec![
                    BoxShadow::new(px(0.), px(1.), Hsla::from(rgb(0xffffff)).opacity(0.7))
                        .blur_radius(px(2.))
                        .inset(),
                    BoxShadow::new(px(0.), px(-1.), Hsla::from(rgb(0x000000)).opacity(0.08))
                        .blur_radius(px(3.))
                        .inset(),
                ])
            })
            .on_action(cx.listener(|note, action: &SetAppearance, window, cx| {
                note.record.style.appearance = action.appearance;
                note.restyle(window, cx);
            }))
            .on_action(cx.listener(|note, action: &SetColor, window, cx| {
                note.record.style.color = action.color;
                note.restyle(window, cx);
            }))
            .on_action(cx.listener(|note, _: &ToggleTranslucent, window, cx| {
                note.record.style.translucent = !note.record.style.translucent;
                note.restyle(window, cx);
            }))
            .on_action(cx.listener(|note, _: &UseAsDefault, window, cx| {
                note.capture(window);
                let record = note.record.clone();
                let library = cx.global::<AppState>().notebook.clone();
                let pending = library.borrow_mut().request_defaults(&record);
                match pending {
                    Err(error) => {
                        note.error = Some(NoteError::other(copy::failed(
                            "Couldn’t save these settings as the default.",
                            &error,
                        )));
                        cx.notify();
                    }
                    Ok(pending) => cx
                        .spawn(async move |_, cx| {
                            let completed = pending.wait().await;
                            cx.update(|cx| {
                                let result = library.borrow_mut().complete(completed);
                                match result {
                                    Ok(()) => {
                                        propagate_checking(&record.id, record.style.checking, cx)
                                    }
                                    Err(error) => notify(
                                        cx,
                                        copy::failed(
                                            "Couldn’t save these settings as the default.",
                                            &error,
                                        ),
                                    ),
                                }
                            });
                        })
                        .detach(),
                }
            }))
            .on_action(cx.listener(|note, action: &ToggleChecking, window, cx| {
                note.record.style.checking.toggle(action.setting);
                note.restyle(window, cx);
            }))
            .on_action(cx.listener(|note, _: &ImportImages, _, cx| note.import_images(cx)))
            .on_action(
                cx.listener(|note, _: &CheckDocument, window, cx| note.check_document(window, cx)),
            )
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .when(!collapsed, |root| {
                root.on_drop(cx.listener(|note, paths: &ExternalPaths, _, cx| {
                    note.paste_images(
                        ClipboardItem {
                            entries: vec![ClipboardEntry::ExternalPaths(paths.clone())],
                        },
                        cx,
                    );
                }))
            })
            .on_action(cx.listener(|note, _: &Save, window, cx| {
                note.capture(window);
                note.autosave(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|note, _: &CloseNote, window, cx| note.close(window, cx)))
            .on_action(
                cx.listener(|note, _: &ExportNote, window, cx| note.export(false, window, cx)),
            )
            .on_action(
                cx.listener(|note, _: &ToggleCollapse, window, cx| note.collapse(window, cx)),
            )
            .on_action(
                cx.listener(|note, _: &ToggleFloat, window, cx| note.toggle_float(window, cx)),
            )
            .on_action(cx.listener(|note, _: &Zoom, window, cx| note.toggle_zoom(window, cx)))
            .map(|root| {
                #[cfg(not(target_os = "macos"))]
                let root = root.on_action(cx.listener(
                    |note, _: &linux_menu::ToggleNoteMenu, window, cx| {
                        note.toggle_note_menu(window, cx)
                    },
                ));
                root
            })
            .child(div().h(px(HEIGHT)).flex_shrink_0())
            .when(sea, |root| {
                // A broad upper-left reflection under the text. The tumbled silhouette is
                // the larger corner radius; there is no moving highlight.
                root.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded(px(radius))
                        .bg(linear_gradient(
                            135.,
                            linear_color_stop(Hsla::from(rgb(0xffffff)).opacity(0.2), 0.),
                            linear_color_stop(Hsla::from(rgb(0xffffff)).opacity(0.), 1.),
                        )),
                )
            })
            .when(!collapsed, |root| {
                root.on_action(cx.listener(|note, action: &ReadingSize, window, cx| {
                    note.record.style.reading_size = if action.reset {
                        12.
                    } else {
                        (note.record.style.reading_size.round() + action.delta).clamp(9., 48.)
                    };
                    note.restyle(window, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .px(px(6. + inset))
                        .pb(px(inset))
                        .child(self.editor.clone()),
                )
            })
            // A collapsed note's 12-point window would clip the banner and its buttons, which
            // would still take focus: the strip shows the message instead (see `strip_label`).
            .when_some(self.error.clone().filter(|_| !collapsed), |root, error| {
                let actions = self.error_actions().iter().map(|label| {
                    error_action(label).on_click(cx.listener(move |note, _, window, cx| {
                        match *label {
                            RETRY => {
                                note.autosave(cx);
                            }
                            KEEP_MINE => note.keep_mine_as_copy(window, cx),
                            RELOAD => note.reload_from_disk(window, cx),
                            _ => note.error = None,
                        }
                        cx.notify();
                    }))
                });
                // Interface, not note text: the system face, smaller and quieter than the
                // note, set off by a hairline, with its way out as real buttons.
                root.child(
                    div()
                        .debug_selector(|| "error-banner".into())
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .px(px(10.))
                        .py(px(6.))
                        .border_t_1()
                        .border_color(appearance::ink(0.15))
                        .font_family(".SystemUIFont")
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .text_color(appearance::ink(0.85))
                        .child(error.message)
                        .child(div().flex().gap(px(8.)).children(actions)),
                )
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(HEIGHT))
                    .bg(spine)
                    .when(frosted && !collapsed, |strip| {
                        strip.border_b_1().border_color(hairline)
                    })
                    // GPUI clips children to rectangles, so the opaque spine follows the
                    // note's rounded corners itself. macOS also clips the whole window; on
                    // Linux nothing else does, and a square spine showed past the rim.
                    .when(frosted, |strip| {
                        if collapsed {
                            strip.rounded(px(radius))
                        } else {
                            strip.rounded_t(px(radius))
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |note, event: &MouseDownEvent, window, cx| {
                            let local = event.position.x;
                            let width = window.viewport_size().width;
                            if local < px(6. + inset) || local > width - px(6. + inset) {
                                return;
                            }
                            if event.click_count == 2 {
                                match platform::double_click_action() {
                                    platform::DoubleClickAction::Zoom => {
                                        note.toggle_zoom(window, cx)
                                    }
                                    platform::DoubleClickAction::Minimize => {
                                        window.minimize_window()
                                    }
                                    platform::DoubleClickAction::Nothing => {}
                                    platform::DoubleClickAction::Collapse => {
                                        note.collapse(window, cx)
                                    }
                                }
                            } else {
                                window.start_window_move();
                            }
                        }),
                    )
                    .child(
                        // GPUI clicks a focused control on Return and Space.
                        chrome::control(Control::Close, collapsed, active, &self.record.style)
                            .absolute()
                            .left(px(10. + inset))
                            .top_0()
                            .on_click(cx.listener(|note, _, window, cx| note.close(window, cx))),
                    )
                    .when(collapsed, |header| {
                        // Clear of the close box, symmetric on both sides, and small enough
                        // that its descenders fit the 12-point strip. The text is a flex
                        // child that may shrink below its content width, so GPUI knows the
                        // width to truncate to and ends a long title with an ellipsis
                        // instead of clipping it mid-glyph.
                        header.child(
                            div()
                                .absolute()
                                .left(px(26. + inset))
                                .right(px(26. + inset))
                                .top_0()
                                .h(px(12.))
                                .flex()
                                .items_center()
                                .overflow_hidden()
                                .font_family(".SystemUIFont")
                                .text_size(px(chrome::TITLE_SIZE))
                                .line_height(px(12.))
                                .when(frosted, |title| {
                                    title.text_color(if sea { ink.into() } else { appearance::ink(0.85) })
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(self.strip_label()),
                                ),
                        )
                    })
                    .when(!collapsed, |header| {
                        #[cfg(not(target_os = "macos"))]
                        let header = header.child(
                            linux_menu::button(self, active, cx)
                                .absolute()
                                .right(px(34. + inset))
                                .top_0(),
                        );
                        header
                    })
                    .when(!collapsed, |root| {
                        root.child(
                            chrome::control(Control::Zoom, false, active, &self.record.style)
                                .absolute()
                                .right(px(22. + inset))
                                .top_0()
                                .on_click(cx.listener(|note, _, window, cx| {
                                    note.toggle_zoom(window, cx)
                                })),
                        )
                    })
                    .child(
                        chrome::control(Control::Collapse, collapsed, active, &self.record.style)
                            .absolute()
                            .right(px(10. + inset))
                            .top_0()
                            .on_click(cx.listener(|note, _, window, cx| note.collapse(window, cx))),
                    ),
            )
            .when(!frosted && !cfg!(target_os = "macos"), |root| {
                root.child(chrome::perimeter())
            })
            .when(frosted, |root| {
                root.child(chrome::rim(platform::increase_contrast(), radius))
            })
            .children({
                #[cfg(not(target_os = "macos"))]
                let panel = linux_menu::panel(self, window, cx);
                #[cfg(target_os = "macos")]
                let panel: Option<AnyElement> = None;
                panel
            })
    }
}

pub(crate) fn error_action(label: &'static str) -> Stateful<Div> {
    div()
        .id(label)
        .role(Role::Button)
        .aria_label(label)
        .tab_index(0)
        .cursor_pointer()
        .h(px(18.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(appearance::ink(0.25))
        .bg(Hsla::from(rgb(0xffffff)).opacity(0.5))
        .focus_visible(|style| style.border_color(rgb(chrome::FOCUS_RING)))
        .active(|style| style.bg(appearance::ink(0.1)))
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::{Frame, manual_bounds_change};
    #[test]
    fn only_a_held_button_outside_our_own_changes_counts_as_the_user() {
        let zoomed = Frame {
            x: 0.,
            y: 0.,
            width: 1200.,
            height: 900.,
        };
        let dragged = Frame { x: 30., ..zoomed };
        let resized = Frame {
            width: 900.,
            ..zoomed
        };
        assert!(manual_bounds_change(zoomed, dragged, true, false));
        assert!(manual_bounds_change(zoomed, resized, true, false));
        // The animation of a zoom or restore, a collapse, or a system adjustment.
        assert!(!manual_bounds_change(zoomed, dragged, false, false));
        assert!(!manual_bounds_change(zoomed, dragged, true, true));
        // A report that changes nothing, or by rounding only, is not an edit.
        assert!(!manual_bounds_change(zoomed, zoomed, true, false));
        let rounded = Frame {
            width: 1200.25,
            ..zoomed
        };
        assert!(!manual_bounds_change(zoomed, rounded, true, false));
    }
    #[test]
    fn long_titles_end_in_an_ellipsis_and_short_ones_are_untouched() {
        assert_eq!(super::clip_title("Groceries"), "Groceries");
        let exact = "x".repeat(super::TITLE_CHARS);
        assert_eq!(super::clip_title(&exact), exact);
        let long = "Groceries for the weekend and a very long title that will not fit";
        assert_eq!(
            super::clip_title(long),
            "Groceries for the weekend and a very long title that will…"
        );
        // No space before the ellipsis, and characters are never split.
        let spaced = format!("{} tail", "a".repeat(super::TITLE_CHARS - 1));
        assert_eq!(super::clip_title(&spaced), format!("{}…", "a".repeat(59)));
        let unbroken = "b".repeat(70);
        assert_eq!(super::clip_title(&unbroken), format!("{}…", "b".repeat(60)));
        assert_eq!(super::clip_title(&"🍃".repeat(70)).chars().count(), 61);
    }
    #[test]
    fn title_reads_only_up_to_the_first_block_with_text_and_matches_the_whole_note() {
        let schema = markraft_commonmark::commonmark_schema();
        let whole = |doc: &markraft_core::Node| {
            markraft_commonmark::to_plain_text(&schema, doc)
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| super::clip_title(line.trim()))
                .unwrap_or_else(|| "Untitled".into())
        };
        for source in [
            "",
            "\n\n",
            "# **Bold** title\n\nbody",
            "- first item\n- second\n",
            "| a | b |\n|---|---|\n| c | d |\n",
            "```\ncode line\n```\n",
            "<!-- hidden -->\n\nVisible",
            "> quoted *text*\n",
            &"long ".repeat(40),
        ] {
            let parsed = markraft_commonmark::SourceDocument::parse(&schema, source).unwrap();
            assert_eq!(
                super::title_of(&schema, parsed.document()),
                whole(parsed.document()),
                "{source:?}"
            );
        }
    }
    #[test]
    fn unparseable_markdown_is_reported_instead_of_panicking() {
        // No CommonMark input found so far fails to parse, so a schema that lacks most
        // CommonMark types forces the parser's error path.
        use markraft_core::{NodeTypeSpec, Schema, SchemaSpec};
        let schema = Schema::new(
            SchemaSpec::new()
                .node(NodeTypeSpec::new("doc", "paragraph+"))
                .node(NodeTypeSpec::new("paragraph", "text*"))
                .node(NodeTypeSpec::new("text", "")),
        )
        .unwrap();
        let error = super::parse_note(&schema, "# Heading\n\n| a |\n|---|\n| b |\n").unwrap_err();
        assert!(error.contains("couldn’t be read as Markdown"), "{error}");
        assert!(!error.contains("UnknownType"), "no Debug text: {error}");
    }
}

#[cfg(test)]
mod error_tests {
    use super::{Arc, ErrorSource, NoteError, SourceSaveError, copy, storage};
    use std::error::Error;
    #[test]
    fn save_errors_keep_original_io_cause_and_plain_wording_across_clones() {
        let io = std::io::Error::from(std::io::ErrorKind::StorageFull);
        let expected = copy::failed("Couldn’t save this note.", &io);
        let error = NoteError::save_failure(io);
        assert_eq!(error.to_string(), expected);
        assert_eq!(error.origin, ErrorSource::Save);
        assert!(error.source().is_some());
        let clone = error.clone();
        let cause = error.cause.as_ref().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::StorageFull);
        assert!(Arc::ptr_eq(cause, clone.cause.as_ref().unwrap()));
        let conflict =
            NoteError::save_failure(std::io::Error::other(storage::StorageError::ExternalChange));
        assert_eq!(conflict.origin, ErrorSource::Conflict);
        assert!(storage::is_external_change(
            conflict.cause.as_ref().unwrap()
        ));
        let render = NoteError::save_failure(std::io::Error::other(SourceSaveError(
            markraft_commonmark::SourceError::UnsupportedEdit,
        )));
        let io = render.cause.as_ref().unwrap();
        let source = io
            .get_ref()
            .unwrap()
            .downcast_ref::<SourceSaveError>()
            .unwrap();
        assert_eq!(source.0, markraft_commonmark::SourceError::UnsupportedEdit);
        assert!(source.source().is_some());
    }
}
