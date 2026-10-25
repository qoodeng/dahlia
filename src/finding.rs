use crate::{AppState, Note, panel};
use gpui::{prelude::*, *};
use markraft_core::{
    EditorState, TransactionSpec,
    kind::{DocumentKind, ReadingReplacementPolicy},
};
use std::ops::Range;

pub struct FindControl {
    query: Entity<markraft_gpui::EditorView>,
    replacement: Entity<markraft_gpui::EditorView>,
    target: WindowHandle<Note>,
    all: bool,
    results: Vec<(WindowHandle<Note>, String, usize)>,
    status: String,
    /// The status VoiceOver was last told about.
    announced: crate::platform::Announced,
    task: Option<Task<()>>,
    search: Option<Task<()>>,
    _query: Subscription,
    _closed: Subscription,
    _appearance: Subscription,
    _activation: Subscription,
    _edits: Vec<Subscription>,
}
/// Said when a note closes while Replace All is still preparing, before anything has changed.
const NOTE_CLOSED: &str = "A note closed before the replacements were ready. Try again.";
/// Said when a replacement fails after some notes may already have changed.
const PARTIALLY_REPLACED: &str = "Choose Edit ▸ Undo in each note that changed, or restore the snapshot Dahlia saved first from File ▸ Recovery.";
/// Why a match can't be replaced when it spans headings, lists, links or other formatting.
const STRUCTURE: &str = "This match crosses formatting Dahlia can’t change safely. Edit it in the note.";
/// Why one match can't be replaced, when the cause is internal. The cause goes to the log.
fn unsafe_match(error: impl std::fmt::Display) -> String {
    log::warn!("Cannot plan a replacement: {error}");
    "This match can’t be replaced safely. Edit it in the note.".to_owned()
}
/// A note's window update and then its placement save, as one message.
fn persisted(result: Result<std::io::Result<()>, impl std::fmt::Display>) -> Result<(), String> {
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(crate::copy::failed(
            "Couldn’t save this note’s size and position.",
            &error,
        )),
        Err(error) => {
            log::warn!("Cannot reach a note window: {error}");
            Err(NOTE_CLOSED.to_owned())
        }
    }
}
/// Find waits this long after the last keystroke in its field before searching.
const FIND_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);
impl FindControl {
    fn new(target: WindowHandle<Note>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input_style = panel::input_style(panel::Palette::of(window), ".SystemUIFont".into());
        let query = cx.new(|cx| {
            markraft_gpui::EditorView::single_line(cx)
                .with_style(input_style.clone())
                .with_aria_label("Find")
        });
        let replacement = cx.new(|cx| {
            markraft_gpui::EditorView::single_line(cx)
                .with_style(input_style)
                .with_aria_label("Replace with")
        });
        let text = cx.global::<AppState>().find_query.clone();
        query.update(cx, |editor, cx| editor.set_value(&text, cx));
        let changed = cx.subscribe(
            &query,
            |control, _, event: &markraft_gpui::EditorEvent, cx| {
                if matches!(event, markraft_gpui::EditorEvent::Changed { .. }) {
                    control.refresh(cx);
                }
            },
        );
        window.focus(&query.read(cx).focus_handle(cx), cx);
        // An active query searches a note again after every edit (about 60 ms per keystroke
        // at 1 MB), so hits are shown only while Find is open; Find Next runs the query again.
        let closed = cx.on_release(|_, cx| {
            let windows: Vec<_> = cx.global::<AppState>().windows.values().copied().collect();
            for handle in windows {
                let _ = handle.update(cx, |note, _, cx| {
                    note.editor.update(cx, |editor, cx| {
                        if !editor.find_status().query.is_empty() {
                            editor.set_find_query(String::new(), cx);
                        }
                    })
                });
            }
        });
        let mut control = Self {
            query,
            replacement,
            target,
            all: false,
            results: vec![],
            status: String::new(),
            announced: crate::platform::Announced::default(),
            task: None,
            search: None,
            _query: changed,
            _closed: closed,
            _edits: Vec::new(),
            _appearance: cx.observe_window_appearance(window, |control, window, cx| {
                let style = panel::input_style(panel::Palette::of(window), ".SystemUIFont".into());
                for field in [&control.query, &control.replacement] {
                    field.update(cx, |editor, cx| editor.set_style(style.clone(), cx));
                }
                cx.notify();
            }),
            // Notes may have changed while the panel was in the background; refresh on return.
            _activation: cx.observe_window_activation(window, |control, window, cx| {
                if window.is_window_active() {
                    control.return_to_panel(cx);
                }
            }),
        };
        control.refresh(cx);
        control
    }
    fn targets(&self, cx: &App) -> Vec<WindowHandle<Note>> {
        if self.all {
            cx.global::<AppState>().windows.values().copied().collect()
        } else {
            vec![self.target]
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.search(None, cx);
    }
    /// The panel became the key window again. Current Note scope follows the front note,
    /// and notes may have changed meanwhile, so search again.
    fn return_to_panel(&mut self, cx: &mut Context<Self>) {
        let moved = self.retarget(true, cx);
        if moved || !self.query.read(cx).text().is_empty() {
            self.refresh(cx);
        }
    }
    /// Point the panel at the front note when its target has closed, or, with `follow`, when
    /// Current Note scope should follow the user to another note. True when it moved.
    fn retarget(&mut self, follow: bool, cx: &mut Context<Self>) -> bool {
        let Some(front) = crate::active_window(cx) else {
            return false;
        };
        let closed = self.target.update(cx, |_, _, _| ()).is_err();
        if front == self.target || !(closed || (follow && !self.all)) {
            return false;
        }
        if !closed {
            // The note being left stops showing this panel's highlights.
            let _ = self.target.update(cx, |note, _, cx| {
                note.editor.update(cx, |editor, cx| {
                    if !editor.find_status().query.is_empty() {
                        editor.set_find_query(String::new(), cx);
                    }
                })
            });
        }
        self.target = front;
        true
    }
    /// Search after a pause in typing, off the main thread, then show `status`, or the
    /// match count without one. Hits found in a note that has changed since are discarded
    /// and that note is scheduled for another background search.
    fn search(&mut self, status: Option<String>, cx: &mut Context<Self>) {
        let query = self.query.read(cx).text().to_owned();
        cx.global_mut::<AppState>().find_query = query.clone();
        let editors: Vec<_> = self
            .targets(cx)
            .into_iter()
            .filter_map(|handle| handle.update(cx, |note, _, _| note.editor.clone()).ok())
            .collect();
        self._edits = editors
            .into_iter()
            .map(|editor| {
                cx.subscribe(
                    &editor,
                    |control, _, event: &markraft_gpui::EditorEvent, cx| {
                        if matches!(event, markraft_gpui::EditorEvent::Changed { .. }) {
                            control.refresh(cx);
                        }
                    },
                )
            })
            .collect();
        self.search = Some(cx.spawn(async move |control, cx| {
            cx.background_executor().timer(FIND_DEBOUNCE).await;
            let Ok(snapshots) = control.update(cx, |control, cx| {
                control
                    .targets(cx)
                    .into_iter()
                    .filter_map(|handle| {
                        handle
                            .update(cx, |note, _, cx| {
                                let editor = note.editor.read(cx);
                                (
                                    handle,
                                    note.generation,
                                    editor.state_without_find(),
                                    editor.doc_types(),
                                )
                            })
                            .ok()
                    })
                    .collect::<Vec<_>>()
            }) else {
                return;
            };
            let search = query.clone();
            let found = cx
                .background_executor()
                .spawn(async move {
                    snapshots
                        .into_iter()
                        .map(|(handle, revision, state, types)| {
                            let hits = markraft_gpui::find_hits_in(&state, &types, &search);
                            (handle, revision, hits)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = control.update(cx, |control, cx| {
                let mut again = false;
                control.results = found
                    .into_iter()
                    .filter_map(|(handle, revision, hits)| {
                        handle
                            .update(cx, |note, _, cx| {
                                let current = note.generation == revision;
                                note.editor.update(cx, |editor, cx| {
                                    if current {
                                        editor.set_find_results(query.clone(), hits, cx);
                                    } else {
                                        again = true;
                                        editor.set_find_results(query.clone(), Vec::new(), cx);
                                    }
                                });
                                (
                                    handle,
                                    note.record.title.clone(),
                                    note.editor.read(cx).find_status().total,
                                )
                            })
                            .ok()
                    })
                    .filter(|(_, _, count)| *count > 0)
                    .collect();
                control.status = status.unwrap_or_else(|| {
                    if query.is_empty() {
                        String::new()
                    } else {
                        matches(control.results.iter().map(|(_, _, count)| count).sum())
                    }
                });
                if again {
                    control.refresh(cx);
                }
                cx.notify();
            });
        }));
    }
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.retarget(false, cx) {
            // Search the new note now, so this step has hits to move between.
            let query = self.query.read(cx).text().to_owned();
            let _ = self.target.update(cx, |note, _, cx| {
                note.editor
                    .update(cx, |editor, cx| editor.set_find_query(query, cx))
            });
            self.refresh(cx);
        }
        let _ = self.target.update(cx, |note, _, cx| {
            note.editor.update(cx, |editor, cx| {
                if forward {
                    editor.find_next(cx);
                } else {
                    editor.find_previous(cx);
                }
            })
        });
    }
    fn replace(&mut self, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.task.is_some() {
            return;
        }
        let replacement = self.replacement.read(cx).text().to_owned();
        if self.retarget(false, cx) {
            self.refresh(cx);
        }
        if !all {
            let result = self.target.update(cx, |note, _, cx| {
                if note.editor.read(cx).is_composing() {
                    return false;
                }
                note.editor
                    .update(cx, |editor, cx| replace_current(editor, &replacement, cx))
            });
            self.status = if matches!(result, Ok(true)) {
                String::new()
            } else {
                "This match can’t be replaced. Edit it in the note.".into()
            };
            cx.notify();
            return;
        }
        let query = self.query.read(cx).text().to_owned();
        // Snapshot every target; finding and planning run off the main thread.
        let mut snapshots = Vec::new();
        for handle in self.targets(cx) {
            let snapshot = handle.update(cx, |note, _, cx| {
                // Until its text arrives the editor is an empty stand-in with nothing to replace.
                if note.opening {
                    return Err("This note is still opening. Try again in a moment.");
                }
                let editor = note.editor.read(cx);
                if editor.is_composing() {
                    return Err("Finish typing in the note, then choose Replace All again.");
                }
                Ok(Snapshot {
                    handle,
                    revision: note.generation,
                    state: editor.state_without_find(),
                    preparation: editor.prepare_edit(),
                    types: editor.doc_types(),
                    source: note.source.clone(),
                })
            });
            match snapshot {
                Ok(Ok(snapshot)) => snapshots.push(snapshot),
                Ok(Err(status)) => {
                    self.status = status.into();
                    cx.notify();
                    return;
                }
                Err(_) => {
                    self.status = NOTE_CLOSED.into();
                    cx.notify();
                    return;
                }
            }
        }
        let control_window = window.window_handle();
        self.status = "Preparing replacements…".into();
        cx.notify();
        self.task = Some(cx.spawn(async move |control, cx| {
            let planned = cx
                .background_executor()
                .spawn(async move {
                    let mut plans = Vec::new();
                    for snapshot in snapshots {
                        let (handle, revision) = (snapshot.handle, snapshot.revision);
                        if let Some(spec) = snapshot.plan(&query, &replacement)? {
                            plans.push((handle, revision, spec));
                        }
                    }
                    Ok::<_, String>(plans)
                })
                .await;
            let finish = |status: String, cx: &mut AsyncApp| {
                let _ = control.update(cx, |control, cx| {
                    control.task.take();
                    control.status = status;
                    cx.notify();
                });
            };
            let plans = match planned {
                Ok(plans) if plans.is_empty() => return finish("No matches.".into(), cx),
                Ok(plans) => plans,
                Err(error) => {
                    return finish(
                        format!("Couldn’t prepare the replacements. No notes were changed. {error}"),
                        cx,
                    );
                }
            };
            let question = control_window.update(cx, |_, window, cx| {
                let title = if plans.len() == 1 {
                    "Replace all matches in this note?".to_owned()
                } else {
                    format!("Replace all matches in {} notes?", plans.len())
                };
                window.prompt(
                    PromptLevel::Info,
                    &title,
                    Some("Dahlia saves a snapshot first. You can restore it from File ▸ Recovery."),
                    &["Replace All", "Cancel"],
                    cx,
                )
            });
            let Ok(question) = question else {
                return finish(String::new(), cx);
            };
            if question.await.ok() != Some(0) {
                return finish(String::new(), cx);
            }
            let unchanged = |plans: &[(WindowHandle<Note>, u64, markraft_gpui::PreparedEdit)], cx: &mut AsyncApp| {
                cx.update(|cx| {
                    for (handle, revision, prepared) in plans {
                        let unchanged = handle
                            .update(cx, |note, _, cx| {
                                note.generation == *revision && !note.editor.read(cx).is_composing()
                                    && !note.durability_lock.load(std::sync::atomic::Ordering::Acquire)
                                    && note.editor.read(cx).prepared_is_current(prepared)
                            })
                            .map_err(|error| {
                                log::warn!("Cannot reach a note window: {error}");
                                NOTE_CLOSED.to_owned()
                            })?;
                        if !unchanged {
                            return Err("A note changed while Dahlia was preparing, so nothing was replaced. Try again.".to_owned());
                        }
                    }
                    Ok(())
                })
            };
            if let Err(error) = unchanged(&plans, cx) {
                return finish(error, cx);
            }
            let saves = cx.update(|cx| {
                plans.iter().map(|(handle, _, _)| {
                    handle.update(cx, |note, window, cx| {
                        note.capture(window);
                        (*handle, note.request_save(cx))
                    }).map_err(|error| {
                        log::warn!("Cannot reach a note window: {error}");
                        NOTE_CLOSED.to_owned()
                    })
                }).collect::<Result<Vec<_>, String>>()
            });
            let saves = match saves {
                Ok(saves) => saves,
                Err(error) => return finish(error, cx),
            };
            for (handle, save) in saves {
                if let Err(error) = save.await {
                    return finish(error.message, cx);
                }
                if let Err(error) = persisted(handle.update(cx, |note, _, _| note.persist_record())) {
                    return finish(error, cx);
                }
            }
            let checkpoint = match unchanged(&plans, cx) {
                Ok(()) => cx.update(|cx| {
                    cx.global::<AppState>().notebook.borrow().request_checkpoint()
                }).await.map_err(|error| {
                    crate::copy::failed("Couldn’t save a snapshot first, so no notes were changed.", &error)
                }),
                Err(error) => Err(error),
            };
            // The checkpoint ran on the metadata thread; typing may have continued meanwhile.
            let saves = checkpoint.and_then(|_| unchanged(&plans, cx)).and_then(|()| {
                cx.update(|cx| {
                    plans.into_iter().map(|(handle, _, prepared)| {
                        handle.update(cx, |note, _, cx| {
                            if !note.editor.update(cx, |editor, cx| editor.dispatch_prepared(prepared, cx)) {
                                return Err(format!("Couldn’t apply every replacement. {PARTIALLY_REPLACED}"));
                            }
                            Ok((handle, note.request_save(cx)))
                        }).map_err(|error| {
                            log::warn!("Cannot reach a note window: {error}");
                            format!("A note closed before every replacement was applied. {PARTIALLY_REPLACED}")
                        })?
                    }).collect::<Result<Vec<_>, String>>()
                })
            });
            let result = match saves {
                Ok(saves) => {
                    let mut result = Ok(());
                    for (handle, save) in saves {
                        if let Err(error) = save.await { result = Err(error.message); }
                        else if let Err(error) = persisted(handle.update(cx, |note, _, _| note.persist_record())) { result = Err(error); }
                    }
                    result
                }
                Err(error) => Err(error),
            };
            let _ = control.update(cx, |control, cx| {
                control.task.take();
                let status = match result {
                    Ok(()) => "Replaced all matches.".into(),
                    Err(error) => error,
                };
                control.status = status.clone();
                control.search(Some(status), cx);
                cx.notify();
            });
        }));
    }
}

/// What Replace All plans against, taken on the main thread and used off it.
struct Snapshot {
    handle: WindowHandle<Note>,
    revision: u64,
    state: EditorState,
    preparation: markraft_gpui::EditPreparation,
    types: markraft_gpui::DocTypes,
    source: std::sync::Arc<markraft_commonmark::SourceTrack>,
}
impl Snapshot {
    /// One transaction replacing every hit, verified once to keep a savable source.
    fn plan(
        self,
        query: &str,
        replacement: &str,
    ) -> Result<Option<markraft_gpui::PreparedEdit>, String> {
        let hits = markraft_gpui::find_hits_in(&self.state, &self.types, query);
        let Some(spec) = plan_replacements(&self.state, query, hits, replacement)? else {
            return Ok(None);
        };
        let prepared = self.preparation.prepare(spec).map_err(unsafe_match)?;
        self.source
            .prepare_transactions(self.state.schema(), prepared.transactions())
            .map_err(unsafe_match)?;
        Ok(Some(prepared))
    }
}

/// Replace the selected hit, then select the next one.
fn replace_current(
    editor: &mut markraft_gpui::EditorView,
    replacement: &str,
    cx: &mut Context<markraft_gpui::EditorView>,
) -> bool {
    if editor.find_status().total == 0 {
        return false;
    }
    let selected = editor.context_snapshot(Point::default(), markraft_gpui::ContextTarget::Text);
    if editor
        .context_text(&selected)
        .is_none_or(|text| text.text != editor.find_status().query)
    {
        editor.set_find_query(editor.find_status().query, cx);
    }
    let request = editor.context_snapshot(Point::default(), markraft_gpui::ContextTarget::Text);
    let replaced = editor.replace_context_text(&request, replacement, cx);
    if replaced {
        // The edit already moved the find state past the replacement, so stepping again
        // would skip a hit. Re-query from the replacement's end instead, which also keeps
        // a replacement that contains the query from being matched again.
        let end = editor.state().selection().to(editor.state().doc());
        editor.dispatch(
            [TransactionSpec::new()
                .selection(markraft_core::Selection::cursor(end))
                .add_to_history(false)],
            cx,
        );
        editor.set_find_query(editor.find_status().query, cx);
    }
    replaced
}
/// Every hit of the editor's query, after re-running it so the hits are current.
#[cfg(test)]
pub(crate) fn collect_matches(
    editor: &mut markraft_gpui::EditorView,
    cx: &mut Context<markraft_gpui::EditorView>,
) -> Vec<Range<usize>> {
    editor.set_find_query(editor.find_status().query, cx);
    editor.find_hits()
}

/// Plan Replace All as one transaction. Hits in different top-level blocks never interact,
/// so each block's hits are replaced, last first, against a state of its own, blocks run in
/// parallel, and each block's result becomes one change of a single change set. This is
/// linear in the hits and the document, where applying every hit to the whole document in
/// turn was not. `state` should have no find query (`EditorView::state_without_find`), or
/// every planned edit searches the whole document again.
pub(crate) fn plan_replacements(
    state: &EditorState,
    query: &str,
    mut hits: Vec<Range<usize>>,
    replacement: &str,
) -> Result<Option<TransactionSpec>, String> {
    if hits.iter().any(|hit| hit.is_empty()) {
        return Err("Can’t replace an empty match. Change your search and try again.".into());
    }
    hits.sort_by_key(|hit| hit.start);
    hits.dedup();
    if hits.windows(2).any(|pair| pair[0].end > pair[1].start) {
        return Err("Can’t replace matches that overlap. Change your search and try again.".into());
    }
    if hits.is_empty() {
        return Ok(None);
    }
    if let Some(spec) = literal_replacements(state, query, &hits, replacement) {
        return Ok(Some(spec));
    }
    let mut blocks = Vec::new();
    let mut offset = 0;
    for child in state.doc().children() {
        blocks.push(offset..offset + child.node_size());
        offset += child.node_size();
    }
    let mut groups: Vec<(Range<usize>, Vec<Range<usize>>)> = Vec::new();
    for hit in hits {
        let block = blocks
            .get(blocks.partition_point(|block| block.end <= hit.start))
            .filter(|block| hit.end <= block.end)
            .ok_or(STRUCTURE)?;
        match groups.last_mut() {
            Some((last, group)) if last == block => group.push(hit),
            _ => groups.push((block.clone(), vec![hit])),
        }
    }
    // A block can be edited independently when no document-wide definitions can
    // affect its reading. Otherwise keep the complete context for references.
    let mut independent = true;
    state.doc().descendants(&mut |node, _, _, _| {
        if matches!(
            state.schema().node_type(node.type_id()).name(),
            "raw_block" | "footnote_definition"
        ) {
            independent = false;
        }
        !node.is_textblock(state.schema())
    });
    let threads = std::thread::available_parallelism()
        .map_or(1, |threads| threads.get())
        .min(groups.len());
    let chunk = groups.len().div_ceil(threads);
    let changes = std::thread::scope(|scope| {
        let workers: Vec<_> = groups
            .chunks(chunk)
            .map(|groups| {
                scope.spawn(move || {
                    groups
                        .iter()
                        .map(|(block, hits)| {
                            replace_block(state, block, hits, query, replacement, independent)
                        })
                        .collect::<Result<Vec<_>, String>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err("Something went wrong. Try again.".into()))
            })
            .collect::<Result<Vec<_>, String>>()
    })?;
    Ok(Some(
        TransactionSpec::new()
            .changes(changes.into_iter().flatten())
            .user_event(markraft_core::protocol::event::INPUT_REPLACE),
    ))
}
/// Ordinary unstyled words need no reading/source projection. Keep richer text,
/// graphemes and Markdown syntax on the formatter's conservative path.
fn literal_replacements(
    state: &EditorState,
    query: &str,
    hits: &[Range<usize>],
    replacement: &str,
) -> Option<TransactionSpec> {
    if query.is_empty()
        || !query.bytes().all(|b| b.is_ascii_alphanumeric())
        || !replacement
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b' ')
    {
        return None;
    }
    let mut changes = Vec::with_capacity(hits.len());
    for hit in hits {
        // ASCII neighbors prove the match does not cut a combining grapheme or
        // join an unselected Unicode scalar. Boundary/marked cases use the codec.
        let neighbors = state
            .doc()
            .slice(
                hit.start.saturating_sub(1),
                (hit.end + 1).min(state.doc().content_size()),
            )
            .ok()?;
        if neighbors.tokens().iter().any(|token| !matches!(token, markraft_core::Token::Node(node) if node.is_text() && node.text().is_some_and(str::is_ascii))) {
            return None;
        }
        let slice = state.doc().slice(hit.start, hit.end).ok()?;
        let mut text = String::new();
        for token in slice.tokens() {
            match token {
                markraft_core::Token::Node(node) if node.is_text() && node.marks().is_empty() => {
                    text.push_str(node.text()?)
                }
                _ => return None,
            }
        }
        if !text.eq_ignore_ascii_case(query) {
            return None;
        }
        changes.push(if replacement.is_empty() {
            markraft_core::Change::delete(hit.start, hit.end)
        } else {
            markraft_core::Change::replace(
                hit.start,
                hit.end,
                markraft_core::Slice::from_fragment(markraft_core::Fragment::from_node(
                    state.schema().text(replacement),
                )),
            )
        });
    }
    Some(
        TransactionSpec::new()
            .changes(changes)
            .user_event(markraft_core::protocol::event::INPUT_REPLACE),
    )
}

/// Replace `hits`, all inside the top-level `block`, and return the block's new content as
/// one change in `state`'s coordinates.
fn replace_block(
    state: &EditorState,
    block: &Range<usize>,
    hits: &[Range<usize>],
    query: &str,
    replacement: &str,
    independent: bool,
) -> Result<markraft_core::Change, String> {
    if independent {
        let (index, _) = state
            .doc()
            .content()
            .find_index(block.start)
            .ok_or(STRUCTURE)?;
        let node = state.doc().child(index).clone();
        let local = EditorState::create(
            markraft_core::EditorStateConfig::new(state.schema().clone())
                .doc(
                    state
                        .doc()
                        .copy(markraft_core::Fragment::from_nodes([node])),
                )
                .extensions(markraft_commonmark::commonmark_extensions(state.schema())),
        )
        .map_err(unsafe_match)?;
        let hits: Vec<_> = hits
            .iter()
            .map(|hit| hit.start - block.start..hit.end - block.start)
            .collect();
        let mut change =
            replace_block(&local, &(0..block.len()), &hits, query, replacement, false)?;
        change.from = block.start;
        change.to = block.end;
        return Ok(change);
    }
    let kind = crate::markdown::Markdown {
        schema: state.schema().clone(),
    };
    let mut current = state.clone();
    for hit in hits.iter().rev() {
        let spec = replacement_spec(&kind, &current, hit.clone(), query, replacement)?;
        current = current
            .update([spec])
            .map_err(unsafe_match)?
            .state()
            .clone();
    }
    let grown = current.doc().content_size() as isize - state.doc().content_size() as isize;
    let end = usize::try_from(block.end as isize + grown)
        .map_err(|_| STRUCTURE.to_owned())?;
    let mut nodes = Vec::new();
    let mut offset = 0;
    for child in current.doc().children() {
        let next = offset + child.node_size();
        if offset >= block.start && next <= end {
            nodes.push(child.clone());
        } else if offset < end && next > block.start {
            return Err(STRUCTURE.into());
        }
        offset = next;
    }
    Ok(markraft_core::Change::replace(
        block.start,
        block.end,
        markraft_core::Slice::from_fragment(markraft_core::Fragment::from_nodes(nodes)),
    ))
}
/// The edit replacing one hit's reading text, keeping the styles of unchanged characters.
fn replacement_spec(
    kind: &crate::markdown::Markdown,
    state: &EditorState,
    range: Range<usize>,
    query: &str,
    replacement: &str,
) -> Result<TransactionSpec, String> {
    let command = kind
        .replace_reading(
            range.clone(),
            replacement,
            ReadingReplacementPolicy::PreserveUnchanged,
        )
        .ok_or(STRUCTURE)?;
    if let Some(spec) = command(state)? {
        return Ok(spec);
    }
    // Outside paragraphs only a literal source span, one that reads as it is spelled, is
    // replaced as text; anything hidden, escaped or encoded inside the hit refuses.
    let source = state.doc().text_between(
        state.schema(),
        range.start,
        range.end,
        Some("\n"),
        Some(&|_| "\u{fffc}".to_owned()),
    );
    if source.chars().count() != range.len() || source.to_lowercase() != query.to_lowercase() {
        return Err("This match can’t be replaced safely. Edit it in the note.".into());
    }
    let selected = state
        .update([TransactionSpec::new()
            .selection(markraft_core::Selection::text(range.start, range.end))])
        .map_err(unsafe_match)?;
    markraft_core::commands::insert_text(replacement)(selected.state())
        .ok_or_else(|| "This match can’t be replaced safely. Edit it in the note.".into())
}

/// "No matches", "1 match", "12 matches".
pub fn matches(count: usize) -> String {
    match count {
        0 => "No matches".into(),
        1 => "1 match".into(),
        count => format!("{count} matches"),
    }
}
/// A label and its field on one row, the label right-aligned as in AppKit's Find panel.
fn labelled(label: &'static str, field: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().w(px(56.)).flex_shrink_0().text_right().child(label))
        .child(div().flex_1().min_w_0().child(field))
}
impl Render for FindControl {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The status line is not a live region in GPUI, so say each new status.
        self.announced.say(&self.status);
        let palette = panel::Palette::of(window);
        let focused = |field: &Entity<markraft_gpui::EditorView>, cx: &App| {
            field.read(cx).focus_handle(cx).is_focused(window)
        };
        let control = cx.entity().downgrade();
        let scope = |id: &'static str, label: &'static str, all: bool, current: bool| {
            let control = control.clone();
            panel::segment(
                panel::button(id, label, palette, false).debug_selector(|| id.into()),
                current,
                move |_, cx| {
                    let _ = control.update(cx, |control, cx| {
                        control.all = all;
                        control.refresh(cx);
                    });
                },
            )
            .when(current, |segment| {
                segment
                    .bg(palette.selected)
                    .text_color(palette.selected_text)
                    .border_color(palette.selected)
            })
            .rounded(px(0.))
        };
        panel::root(palette)
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(10.))
            // Return finds the next match, as the default button says; Escape and
            // Command-W close the panel. A composition in progress keeps both keys.
            .capture_key_down(cx.listener(|control, event: &KeyDownEvent, window, cx| {
                let composing = [&control.query, &control.replacement]
                    .iter()
                    .any(|field| field.read(cx).is_composing());
                if composing {
                    return;
                }
                if panel::bare(event, "escape") {
                    window.remove_window();
                    cx.stop_propagation();
                } else if panel::bare(event, "enter") {
                    control.step(true, cx);
                    cx.stop_propagation();
                }
            }))
            .on_action(|_: &crate::CloseNote, window, _| window.remove_window())
            .child(labelled(
                "Find:",
                panel::field(palette, focused(&self.query, cx)).child(self.query.clone()),
            ))
            .child(labelled(
                "Replace:",
                panel::field(palette, focused(&self.replacement, cx))
                    .child(self.replacement.clone()),
            ))
            .child(labelled(
                "In:",
                div()
                    .id("find-scope")
                    .flex()
                    .role(Role::RadioGroup)
                    .aria_label("Search in")
                    .rounded(px(5.))
                    .overflow_hidden()
                    .child(scope(
                        "find-scope-current",
                        "Current Note",
                        false,
                        !self.all,
                    ))
                    .child(scope("find-scope-all", "All Open Notes", true, self.all)),
            ))
            .child(
                div()
                    .id("find-status")
                    .role(Role::Status)
                    .min_h(px(14.))
                    .pl(px(64.))
                    .text_size(px(11.))
                    .text_color(palette.secondary)
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .id("find-notes")
                    .flex_1()
                    .overflow_y_scroll()
                    .bg(palette.field)
                    .rounded(px(5.))
                    .border_1()
                    .border_color(palette.field_border)
                    .children(self.results.iter().enumerate().map(
                        |(index, (handle, title, count))| {
                            let handle = *handle;
                            div()
                                .id(index)
                                .role(Role::Button)
                                .aria_label(format!("{title}, {}", matches(*count)))
                                .tab_index(0)
                                .flex()
                                .justify_between()
                                .gap(px(12.))
                                .px(px(8.))
                                .py(px(3.))
                                .border_1()
                                .border_color(transparent_black())
                                .focus_visible(move |row| row.border_color(palette.focus))
                                .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                                .child(
                                    div()
                                        .text_color(palette.secondary)
                                        .font_features(FontFeatures(std::sync::Arc::new(vec![(
                                            "tnum".into(),
                                            1,
                                        )])))
                                        .child(matches(*count)),
                                )
                                .on_click(cx.listener(move |control, _, _, cx| {
                                    control.target = handle;
                                    let _ =
                                        handle.update(cx, |_, window, _| window.activate_window());
                                }))
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        panel::button("find-replace-all", "Replace All", palette, false).on_click(
                            cx.listener(|control, _, window, cx| control.replace(true, window, cx)),
                        ),
                    )
                    .child(
                        panel::button("find-replace", "Replace", palette, false).on_click(
                            cx.listener(|control, _, window, cx| {
                                control.replace(false, window, cx)
                            }),
                        ),
                    )
                    .child(
                        panel::button("find-previous", "Previous", palette, false)
                            .on_click(cx.listener(|control, _, _, cx| control.step(false, cx))),
                    )
                    .child(
                        panel::button("find-next", "Next", palette, true)
                            .on_click(cx.listener(|control, _, _, cx| control.step(true, cx))),
                    ),
            )
    }
}

/// Search again, if Find is open, after a note's editor has been replaced.
pub(crate) fn refresh(cx: &mut App) {
    if let Some(handle) = cx.global::<AppState>().find_window {
        let _ = handle.update(cx, |control, _, cx| control.refresh(cx));
    }
}
pub fn show(cx: &mut App) {
    let Some(target) = crate::active_window(cx) else {
        return;
    };
    let _ = target.update(cx, |note, window, cx| {
        if note.record.collapsed {
            note.collapse(window, cx);
        }
    });
    let query = cx.global::<AppState>().find_query.clone();
    if let Some(handle) = cx.global::<AppState>().find_window
        && handle
            .update(cx, |control, window, cx| {
                control.target = target;
                if control.query.read(cx).text() != query {
                    control
                        .query
                        .update(cx, |editor, cx| editor.set_value(&query, cx));
                }
                control.refresh(cx);
                window.activate_window();
            })
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Find and Replace".into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(440.), px(320.)),
            cx,
        ))),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        cx.new(|cx| FindControl::new(target, window, cx))
    }) {
        Ok(handle) => {
            cx.global_mut::<AppState>().find_window = Some(handle);
            crate::app::sync_panel_levels(cx);
        }
        Err(error) => {
            log::warn!("Cannot open the Find window: {error}");
            crate::notify::notify(cx, "Couldn’t open Find and Replace.");
        }
    }
}
pub fn next(forward: bool, cx: &mut App) {
    let query = cx.global::<AppState>().find_query.clone();
    if let Some(handle) = crate::active_window(cx) {
        let _ = handle.update(cx, |note, _, cx| {
            note.editor.update(cx, |editor, cx| {
                if editor.find_status().query != query {
                    editor.set_find_query(query, cx);
                } else if forward {
                    editor.find_next(cx);
                } else {
                    editor.find_previous(cx);
                }
            })
        });
    }
}
pub fn selection(cx: &mut App) {
    if let Some(handle) = crate::active_window(cx)
        && let Ok(Some(query)) = handle.update(cx, |note, _, cx| {
            note.editor.read(cx).find_selection_query()
        })
    {
        cx.global_mut::<AppState>().find_query = query;
        show(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{Note, collect_matches, plan_replacements, replace_current};
    use gpui::AppContext;
    fn note_with(
        text: &str,
        cx: &mut gpui::TestAppContext,
    ) -> (gpui::Entity<Note>, tempfile::TempDir) {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        let (mut store, _) = crate::storage::NoteStore::open(root.clone()).unwrap();
        store.save(text).unwrap();
        let text = text.to_owned();
        (cx.new(|cx| Note::new(store, text, cx)), _root_fixture)
    }
    /// A click is a selection-only transaction, which Markraft's find state does not follow.
    fn click(
        editor: &mut markraft_gpui::EditorView,
        at: usize,
        cx: &mut gpui::Context<markraft_gpui::EditorView>,
    ) {
        editor.dispatch(
            [
                markraft_core::TransactionSpec::new()
                    .selection(markraft_core::Selection::cursor(at)),
            ],
            cx,
        );
        assert_eq!(editor.state().selection().from(editor.state().doc()), at);
    }
    #[gpui::test]
    fn replace_all_after_a_click_replaces_every_hit_and_never_the_caret(
        cx: &mut gpui::TestAppContext,
    ) {
        let (note, _fixture) = note_with("one x two x three x\n", cx);
        note.update(cx, |note, cx| {
            let ranges = note.editor.update(cx, |editor, cx| {
                editor.set_find_query("x".into(), cx);
                assert_eq!(editor.find_status().total, 3);
                click(editor, 2, cx);
                collect_matches(editor, cx)
            });
            assert_eq!(ranges.len(), 3);
            assert!(ranges.iter().all(|hit| !hit.is_empty()), "{ranges:?}");
            let state = note.editor.read(cx).state_without_find();
            let spec = plan_replacements(&state, "x", ranges, "y")
                .unwrap()
                .unwrap();
            assert!(
                note.editor
                    .update(cx, |editor, cx| editor.dispatch_isolated([spec], cx))
            );
            assert_eq!(note.source_text(cx).unwrap(), "one y two y three y\n");
        });
        drop(note);
    }
    #[gpui::test]
    fn replace_all_keeps_styles_handles_many_hits_per_block_and_refuses_hidden_syntax(
        cx: &mut gpui::TestAppContext,
    ) {
        let original = "# x heading\n\n**x** x _x_ x\n\n- x item\n\n# a\\*b\n";
        let (note, _fixture) = note_with(original, cx);
        note.update(cx, |note, cx| {
            let hits = note.editor.update(cx, |editor, cx| {
                editor.set_find_query("x".into(), cx);
                collect_matches(editor, cx)
            });
            assert_eq!(hits.len(), 6);
            let state = note.editor.read(cx).state_without_find();
            let spec = plan_replacements(&state, "x", hits, "yz").unwrap().unwrap();
            assert!(
                note.editor
                    .update(cx, |editor, cx| editor.dispatch_isolated([spec], cx))
            );
            assert_eq!(
                note.source_text(cx).unwrap(),
                "# yz heading\n\n**yz** yz _yz_ yz\n\n- yz item\n\n# a\\*b\n"
            );
            // One Replace All is one undo step.
            assert!(note.editor.update(cx, |editor, cx| {
                let undo: markraft_core::commands::Command =
                    std::sync::Arc::new(markraft_core::history::undo);
                editor.run_command(&undo, cx)
            }));
            assert_eq!(note.source_text(cx).unwrap(), original);
            // A heading hit whose source is escaped does not read as it is spelled.
            let hits = note.editor.update(cx, |editor, cx| {
                editor.set_find_query("a*b".into(), cx);
                collect_matches(editor, cx)
            });
            assert_eq!(hits.len(), 1);
            let state = note.editor.read(cx).state_without_find();
            assert_eq!(
                plan_replacements(&state, "a*b", hits, "c").unwrap_err(),
                "This match can’t be replaced safely. Edit it in the note."
            );
        });
        drop(note);
    }
    #[gpui::test]
    fn find_waits_for_a_pause_then_searches_off_the_main_thread(cx: &mut gpui::TestAppContext) {
        use crate::{AppState, Notebook, appearance::Fonts};
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        let library = std::rc::Rc::new(std::cell::RefCell::new(
            Notebook::open(root.clone()).unwrap(),
        ));
        cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
        let record = library
            .borrow_mut()
            .create(crate::Frame::default())
            .unwrap();
        let (mut store, _) = library.borrow().open_store(&record.id).unwrap();
        store.save("one x two x\n").unwrap();
        let note = cx.add_window(|_, cx| Note::new(store, "one x two x\n".into(), cx));
        let control = cx.add_window(|window, cx| super::FindControl::new(note, window, cx));
        let total = |cx: &mut gpui::TestAppContext| {
            note.update(cx, |note, _, cx| note.editor.read(cx).find_status().total)
                .unwrap()
        };
        for typed in ["o", "x"] {
            control
                .update(cx, |control, _, cx| {
                    control
                        .query
                        .update(cx, |field, cx| field.set_value(typed, cx))
                })
                .unwrap();
        }
        // Each keystroke only restarts the wait; nothing has searched the note yet.
        assert_eq!(total(cx), 0);
        cx.executor().advance_clock(super::FIND_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(total(cx), 2);
        let status = control
            .update(cx, |control, _, _| control.status.clone())
            .unwrap();
        assert_eq!(status, "2 matches");
        note.update(cx, |note, _, cx| {
            note.editor.update(cx, |editor, cx| {
                let edit = markraft_core::commands::insert_text("x x")(editor.state()).unwrap();
                assert!(editor.dispatch([edit], cx));
            });
        })
        .unwrap();
        assert_eq!(
            total(cx),
            0,
            "old results are invalidated without synchronous searching"
        );
        cx.executor().advance_clock(super::FIND_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(
            total(cx),
            3,
            "typing refreshes the open Find bar in the background"
        );
        // Closing Find stops every note from searching itself again after each edit.
        cx.update(|cx| {
            cx.global_mut::<AppState>()
                .windows
                .insert("test".into(), note)
        });
        control
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        let query = note
            .update(cx, |note, _, cx| note.editor.read(cx).find_status().query)
            .unwrap();
        assert!(query.is_empty());
        assert_eq!(total(cx), 0);
        drop(library);
    }
    /// Find open on note A in Current Note scope, with notes A and B both holding "x".
    #[allow(clippy::type_complexity)]
    fn find_over_two_notes(
        cx: &mut gpui::TestAppContext,
    ) -> (
        tempfile::TempDir,
        std::rc::Rc<std::cell::RefCell<crate::Notebook>>,
        gpui::WindowHandle<Note>,
        gpui::WindowHandle<Note>,
        gpui::WindowHandle<super::FindControl>,
    ) {
        use crate::{AppState, Notebook, appearance::Fonts};
        let fixture = tempfile::tempdir().unwrap();
        let library = std::rc::Rc::new(std::cell::RefCell::new(
            Notebook::open(fixture.path().to_path_buf()).unwrap(),
        ));
        cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
        let open = |text: &str, cx: &mut gpui::TestAppContext| {
            let record = library
                .borrow_mut()
                .create(crate::Frame::default())
                .unwrap();
            let (mut store, _) = library.borrow().open_store(&record.id).unwrap();
            store.save(text).unwrap();
            let text = text.to_owned();
            let window = cx.add_window(|_, cx| Note::new(store, text, cx));
            cx.update(|cx| {
                cx.global_mut::<AppState>()
                    .windows
                    .insert(record.id.clone(), window)
            });
            (record.id, window)
        };
        let (_, a) = open("a x\n", cx);
        let (b_id, b) = open("b x x\n", cx);
        cx.update(|cx| cx.global_mut::<AppState>().active = Some(b_id));
        let control = cx.add_window(|window, cx| super::FindControl::new(a, window, cx));
        control
            .update(cx, |control, _, cx| {
                control
                    .query
                    .update(cx, |field, cx| field.set_value("x", cx))
            })
            .unwrap();
        cx.executor().advance_clock(super::FIND_DEBOUNCE);
        cx.run_until_parked();
        (fixture, library, a, b, control)
    }
    fn hits(note: gpui::WindowHandle<Note>, cx: &mut gpui::TestAppContext) -> usize {
        note.update(cx, |note, _, cx| note.editor.read(cx).find_status().total)
            .unwrap()
    }
    #[gpui::test]
    fn the_scope_segments_switch_the_search_when_pressed(cx: &mut gpui::TestAppContext) {
        let (_fixture, _library, _a, _b, control) = find_over_two_notes(cx);
        let cx = &mut gpui::VisualTestContext::from_window(control.into(), cx);
        for (id, all) in [("find-scope-all", true), ("find-scope-current", false)] {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let segment = cx.debug_bounds(id).unwrap();
            cx.simulate_click(segment.center(), gpui::Modifiers::default());
            assert_eq!(control.update(cx, |c, _, _| c.all).unwrap(), all);
        }
    }
    #[gpui::test]
    fn find_follows_the_front_note_when_the_panel_comes_back(cx: &mut gpui::TestAppContext) {
        let (_fixture, _library, a, b, control) = find_over_two_notes(cx);
        assert_eq!((hits(a, cx), hits(b, cx)), (1, 0));
        // The user clicked into B, then back to the panel: it now searches B.
        control
            .update(cx, |control, _, cx| control.return_to_panel(cx))
            .unwrap();
        cx.executor().advance_clock(super::FIND_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(control.update(cx, |c, _, _| c.target).unwrap(), b);
        assert_eq!((hits(a, cx), hits(b, cx)), (0, 2), "A stops being highlighted");
        // All Open Notes keeps the target the user chose.
        control.update(cx, |control, _, _| control.target = a).unwrap();
        control.update(cx, |control, _, _| control.all = true).unwrap();
        control
            .update(cx, |control, _, cx| control.return_to_panel(cx))
            .unwrap();
        assert_eq!(control.update(cx, |c, _, _| c.target).unwrap(), a);
    }
    #[gpui::test]
    fn find_retargets_when_its_note_has_closed(cx: &mut gpui::TestAppContext) {
        let (_fixture, library, a, b, control) = find_over_two_notes(cx);
        // A closes; B remains and is the front note.
        cx.update(|cx| {
            let id = cx
                .global::<crate::AppState>()
                .windows
                .iter()
                .find(|(_, window)| **window == a)
                .map(|(id, _)| id.clone())
                .unwrap();
            cx.global_mut::<crate::AppState>().release_window(&id);
        });
        a.update(cx, |_, window, _| window.remove_window()).unwrap();
        cx.run_until_parked();
        control
            .update(cx, |control, _, cx| control.step(true, cx))
            .unwrap();
        assert_eq!(control.update(cx, |c, _, _| c.target).unwrap(), b);
        assert_eq!(hits(b, cx), 2, "Next searched the front note, not nothing");
        // Replace All on a closed target prepares against the front note instead of
        // reporting a closed note.
        control.update(cx, |control, _, _| control.target = a).unwrap();
        control
            .update(cx, |control, window, cx| control.replace(true, window, cx))
            .unwrap();
        let status = control.update(cx, |c, _, _| c.status.clone()).unwrap();
        assert_eq!(status, "Preparing replacements…");
        drop(library);
    }
    /// Find open over a note whose window is up but whose text has not arrived; the test
    /// decides when it does.
    #[allow(clippy::type_complexity)]
    fn find_over_an_opening_note(
        cx: &mut gpui::TestAppContext,
    ) -> (
        tempfile::TempDir,
        std::rc::Rc<std::cell::RefCell<crate::Notebook>>,
        gpui::WindowHandle<Note>,
        gpui::WindowHandle<super::FindControl>,
    ) {
        use crate::{AppState, Notebook, appearance::Fonts};
        let fixture = tempfile::tempdir().unwrap();
        let library = std::rc::Rc::new(std::cell::RefCell::new(
            Notebook::open(fixture.path().to_path_buf()).unwrap(),
        ));
        cx.update(|cx| cx.set_global(AppState::new(library.clone(), Fonts::available(&[]))));
        let record = library
            .borrow_mut()
            .create(crate::Frame::default())
            .unwrap();
        let (mut store, _) = library.borrow().open_store(&record.id).unwrap();
        store.save("Large x\n").unwrap();
        let placeholder = crate::parse_note(&markraft_commonmark::commonmark_schema(), "").unwrap();
        let id = record.id.clone();
        let library_for_note = library.clone();
        let note = cx.add_window(|_, cx| {
            Note::opening(store, placeholder, record, Some(library_for_note), cx)
        });
        cx.update(|cx| {
            let state = cx.global_mut::<AppState>();
            state.windows.insert(id.clone(), note);
            state.active = Some(id);
            state.find_query = "x".into();
        });
        cx.update(super::show);
        let control = cx.update(|cx| cx.global::<AppState>().find_window.unwrap());
        (fixture, library, note, control)
    }
    #[gpui::test]
    fn replace_all_waits_for_a_note_that_is_still_opening(cx: &mut gpui::TestAppContext) {
        let (_fixture, _library, note, control) = find_over_an_opening_note(cx);
        assert!(note.update(cx, |note, _, _| note.opening).unwrap());
        control
            .update(cx, |control, window, cx| {
                control.all = true;
                control.replace(true, window, cx);
                // Its text has not arrived, so "No matches" or a success would be false.
                assert_eq!(
                    control.status,
                    "This note is still opening. Try again in a moment."
                );
                assert!(control.task.is_none());
            })
            .unwrap();
    }
    #[gpui::test]
    fn find_follows_a_note_into_its_real_editor(cx: &mut gpui::TestAppContext) {
        let (_fixture, _library, note, control) = find_over_an_opening_note(cx);
        // Find searches while the note is still the empty stand-in and finds nothing.
        cx.executor().advance_clock(super::FIND_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(hits(note, cx), 0);
        // The text arrives with a fresh editor, which knows nothing of the query.
        note.update(cx, |note, window, cx| {
            let schema = markraft_commonmark::commonmark_schema();
            let source = crate::parse_note(&schema, "Large x\n").unwrap();
            let prepared = crate::editor_setup(&schema, &source).prepare();
            note.finish_opening_prepared(source, prepared, window, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(super::FIND_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(hits(note, cx), 1, "highlights follow the new editor");
        let status = control.update(cx, |c, _, _| c.status.clone()).unwrap();
        assert_eq!(status, "1 match");
    }
    #[test]
    fn empty_ranges_are_never_planned_as_replacements() {
        let schema = markraft_commonmark::commonmark_schema();
        let source = markraft_commonmark::SourceDocument::parse(&schema, "alpha").unwrap();
        let state = markraft_core::EditorState::create(
            markraft_core::EditorStateConfig::new(schema).doc(source.document().clone()),
        )
        .unwrap();
        let empty = std::iter::once(2..2).collect();
        assert_eq!(
            plan_replacements(&state, "", empty, "inserted").unwrap_err(),
            "Can’t replace an empty match. Change your search and try again."
        );
    }
    #[gpui::test]
    fn prepared_replacements_reject_stale_selection_and_keep_one_undo(
        cx: &mut gpui::TestAppContext,
    ) {
        let original = "one needle two needle\n";
        let (note, _fixture) = note_with(original, cx);
        note.update(cx, |note, cx| {
            let prepare = |note: &mut Note, cx: &mut gpui::Context<Note>| {
                let editor = note.editor.read(cx);
                let state = editor.state_without_find();
                let hits = markraft_gpui::find_hits_in(&state, &editor.doc_types(), "needle");
                let spec = plan_replacements(&state, "needle", hits, "pin")
                    .unwrap()
                    .unwrap();
                let prepared = editor.prepare_edit().prepare(spec).unwrap();
                note.source
                    .prepare_transactions(state.schema(), prepared.transactions())
                    .unwrap();
                prepared
            };
            let prepared = prepare(note, cx);
            note.editor.update(cx, |editor, cx| {
                editor.dispatch(
                    [markraft_core::TransactionSpec::new()
                        .selection(markraft_core::Selection::cursor(2))
                        .add_to_history(false)],
                    cx,
                );
                assert_eq!(editor.state().selection().head(editor.state().doc()), 2);
                assert!(!editor.prepared_is_current(&prepared));
                assert!(!editor.dispatch_prepared(prepared, cx));
            });
            assert_eq!(note.source_text(cx).unwrap(), original);
            let prepared = prepare(note, cx);
            note.durability_lock
                .store(true, std::sync::atomic::Ordering::Release);
            assert!(
                !note
                    .editor
                    .update(cx, |editor, cx| editor.dispatch_prepared(prepared, cx)),
                "normal guards still refuse prepared edits"
            );
            note.durability_lock
                .store(false, std::sync::atomic::Ordering::Release);
            assert_eq!(note.source_text(cx).unwrap(), original);
            let prepared = prepare(note, cx);
            assert!(
                note.editor
                    .update(cx, |editor, cx| editor.dispatch_prepared(prepared, cx))
            );
            assert_eq!(note.source_text(cx).unwrap(), "one pin two pin\n");
            assert!(note.editor.update(cx, |editor, cx| {
                let undo: markraft_core::commands::Command =
                    std::sync::Arc::new(markraft_core::history::undo);
                editor.run_command(&undo, cx)
            }));
            assert_eq!(note.source_text(cx).unwrap(), original);
        });
        drop(note);
    }
    #[gpui::test]
    fn plain_word_fast_path_does_not_cut_a_combining_grapheme(cx: &mut gpui::TestAppContext) {
        let original = "one e\u{301} two\n";
        let (note, _fixture) = note_with(original, cx);
        note.update(cx, |note, cx| {
            let editor = note.editor.read(cx);
            let state = editor.state_without_find();
            let hits = markraft_gpui::find_hits_in(&state, &editor.doc_types(), "e");
            assert!(!hits.is_empty());
            assert!(super::literal_replacements(&state, "e", &hits, "q").is_none());
            assert_eq!(note.source_text(cx).unwrap(), original);
        });
        drop(note);
    }
    #[gpui::test]
    fn replace_all_keeps_reference_definitions_in_context(cx: &mut gpui::TestAppContext) {
        let original = "[alpha][ref] and alpha\n\n[ref]: https://example.com/target\n";
        let (note, _fixture) = note_with(original, cx);
        note.update(cx, |note, cx| {
            let hits = note.editor.update(cx, |editor, cx| {
                editor.set_find_query("alpha".into(), cx);
                collect_matches(editor, cx)
            });
            assert_eq!(hits.len(), 2);
            let state = note.editor.read(cx).state_without_find();
            let spec = plan_replacements(&state, "alpha", hits, "beta")
                .unwrap()
                .unwrap();
            assert!(
                note.editor
                    .update(cx, |editor, cx| editor.dispatch_isolated([spec], cx))
            );
            assert_eq!(
                note.source_text(cx).unwrap(),
                "[beta][ref] and beta\n\n[ref]: https://example.com/target\n"
            );
        });
        drop(note);
    }
    #[gpui::test]
    fn successive_single_replacements_visit_every_hit_in_order(cx: &mut gpui::TestAppContext) {
        let (note, _fixture) = note_with("one x two x three x\n", cx);
        note.update(cx, |note, cx| {
            note.editor.update(cx, |editor, cx| {
                editor.set_find_query("x".into(), cx);
                assert!(replace_current(editor, "y", cx));
                assert!(replace_current(editor, "y", cx));
            });
            assert_eq!(note.source_text(cx).unwrap(), "one y two y three x\n");
            // A replacement that contains the query is not replaced again.
            note.editor.update(cx, |editor, cx| {
                editor.set_find_query("y".into(), cx);
                assert!(replace_current(editor, "yy", cx));
                assert!(replace_current(editor, "yy", cx));
            });
            assert_eq!(note.source_text(cx).unwrap(), "one yy two yy three x\n");
        });
        drop(note);
    }
    #[gpui::test]
    fn displayed_find_replacements_preserve_source_destinations_and_one_undo(
        cx: &mut gpui::TestAppContext,
    ) {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        let original = "---\r\ntitle: alpha preserved\r\n---\r\n\r\n# alpha\r\n\r\n**alpha** [alpha](https://example.com/alpha)\r\n\r\n![picture](attachments/alpha.png)\r\n\r\n<!-- alpha preserved -->\r\n";
        let (mut store, _) = crate::storage::NoteStore::open(root.clone()).unwrap();
        store.save(original).unwrap();
        let note = cx.new(|cx| Note::new(store, original.to_owned(), cx));
        note.update(cx, |note, cx| {
            let ranges = note.editor.update(cx, |editor, cx| {
                editor.set_find_query("alpha".into(), cx);
                assert_eq!(editor.find_status().total, 4);
                collect_matches(editor, cx)
            });
            let state = note.editor.read(cx).state_without_find();
            let spec = plan_replacements(&state, "alpha", ranges, "beta")
                .unwrap()
                .unwrap();
            assert!(
                note.editor
                    .update(cx, |editor, cx| editor.dispatch_isolated([spec], cx))
            );
            let source = note.source_text(cx).unwrap();
            assert_eq!(
                source,
                original
                    .replace("# alpha", "# beta")
                    .replace("**alpha** [alpha]", "**beta** [beta]")
                    .replace("<!-- alpha preserved -->", "<!-- beta preserved -->")
            );
            assert!(note.editor.update(cx, |editor, cx| {
                let undo: markraft_core::commands::Command =
                    std::sync::Arc::new(markraft_core::history::undo);
                editor.run_command(&undo, cx)
            }));
            assert_eq!(note.source_text(cx).unwrap(), original);
            assert!(note.save(cx));
        });
        assert_eq!(
            std::fs::read_to_string(root.join("note.md")).unwrap(),
            original
        );
        drop(note);
    }
    #[test]
    fn overlapping_find_plan_is_rejected_before_any_mutation() {
        let schema = markraft_commonmark::commonmark_schema();
        let source = markraft_commonmark::SourceDocument::parse(&schema, "alpha").unwrap();
        let state = markraft_core::EditorState::create(
            markraft_core::EditorStateConfig::new(schema).doc(source.document().clone()),
        )
        .unwrap();
        assert!(plan_replacements(&state, "", vec![1..4, 3..5], "changed").is_err());
        assert_eq!(source.source(), "alpha");
    }
}
