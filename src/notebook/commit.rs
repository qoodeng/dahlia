//! Structural notebook changes: sent to the metadata writer, published only once it acknowledges them.
use super::*;

pub(super) fn rebase_value<T: Clone + PartialEq>(candidate: &mut T, base: &T, current: &T) {
    if candidate == base {
        *candidate = current.clone();
    }
}

/// Carry the fields the user changed while a structural commit was in flight onto the
/// acknowledged `note`. Both patterns name every field with no `..`: a new `NoteRecord` or
/// `NoteStyle` field fails to compile here until it is rebased below or marked structural.
fn rebase_note(note: &mut NoteRecord, base: &NoteRecord, current: &NoteRecord) {
    let NoteRecord {
        id: _, // Structural: the commit decides which notes exist.
        title,
        open,
        frame,
        expanded_height,
        collapsed,
        floating,
        style,
        state,
        state_changed_at,
        source_base,
        modified_at,
        zoom_restore,
    } = note;
    rebase_value(title, &base.title, &current.title);
    rebase_value(open, &base.open, &current.open);
    rebase_value(frame, &base.frame, &current.frame);
    rebase_value(expanded_height, &base.expanded_height, &current.expanded_height);
    rebase_value(collapsed, &base.collapsed, &current.collapsed);
    rebase_value(floating, &base.floating, &current.floating);
    rebase_value(state, &base.state, &current.state);
    rebase_value(state_changed_at, &base.state_changed_at, &current.state_changed_at);
    rebase_value(source_base, &base.source_base, &current.source_base);
    rebase_value(modified_at, &base.modified_at, &current.modified_at);
    rebase_value(zoom_restore, &base.zoom_restore, &current.zoom_restore);
    let NoteStyle {
        appearance,
        color,
        translucent,
        reading_size,
        checking,
    } = style;
    rebase_value(appearance, &base.style.appearance, &current.style.appearance);
    rebase_value(color, &base.style.color, &current.style.color);
    rebase_value(translucent, &base.style.translucent, &current.style.translucent);
    rebase_value(reading_size, &base.style.reading_size, &current.style.reading_size);
    rebase_value(checking, &base.style.checking, &current.style.checking);
}

/// A structural operation that has been sent, but has not changed UI-visible state.
/// Always await and complete it: dropping its waiter does not cancel the disk write.
#[must_use = "Await this request and pass its acknowledgement to Notebook::complete"]
pub struct PendingCommit<T> {
    pub(super) token: u64,
    pub(super) base: Index,
    pub(super) candidate: Index,
    pub(super) receiver: oneshot::Receiver<Outcome>,
    pub(super) value: T,
}
#[must_use = "Apply the acknowledgement with Notebook::complete"]
pub struct CompletedCommit<T> {
    pub(super) pending: PendingCommit<T>,
    pub(super) outcome: Outcome,
}
impl<T> PendingCommit<T> {
    pub async fn wait(mut self) -> CompletedCommit<T> {
        let outcome = (&mut self.receiver)
            .await
            .unwrap_or_else(|_| (false, Err(Metadata::stopped())));
        CompletedCommit {
            pending: self,
            outcome,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub(super) enum CommitError {
    #[error("Dahlia is still saving another change. Try again in a moment.")]
    Busy,
    #[error(
        "Dahlia couldn’t save its list of notes. Copy any edited text, then quit and reopen Dahlia."
    )]
    WriterStopped,
}
impl Notebook {
    pub(super) fn busy() -> io::Error {
        io::Error::new(io::ErrorKind::WouldBlock, CommitError::Busy)
    }
    pub(super) fn request_commit<T>(
        &mut self,
        index: Index,
        prepare: impl FnOnce() -> io::Result<()> + Send + 'static,
        value: T,
    ) -> io::Result<PendingCommit<T>> {
        self.request_commit_creating(index, Vec::new(), prepare, value)
    }
    /// `request_commit` for a `prepare` that creates `created` note folders. If `prepare`
    /// fails, even partway through a batch, or the index then cannot be written, every one
    /// of them is removed again; they hold only copies of text the caller still has.
    pub(super) fn request_commit_creating<T>(
        &mut self,
        index: Index,
        created: Vec<PathBuf>,
        prepare: impl FnOnce() -> io::Result<()> + Send + 'static,
        value: T,
    ) -> io::Result<PendingCommit<T>> {
        let undo = (!created.is_empty()).then(|| {
            Box::new(move || {
                for directory in created {
                    if let Err(error) = fs::remove_dir_all(&directory)
                        && error.kind() != io::ErrorKind::NotFound
                    {
                        log::warn!(
                            "Could not remove {} after a failed change: {error}",
                            directory.display()
                        );
                    }
                }
            }) as Undo
        });
        self.request_commit_undoing(index, undo, prepare, value)
    }
    /// `request_commit` that runs `undo` on the writer thread if the index never reaches disk,
    /// whether `prepare` or the write failed.
    pub(super) fn request_commit_undoing<T>(
        &mut self,
        index: Index,
        undo: Option<Undo>,
        prepare: impl FnOnce() -> io::Result<()> + Send + 'static,
        value: T,
    ) -> io::Result<PendingCommit<T>> {
        if self.pending_commit.is_some() {
            return Err(Self::busy());
        }
        validate_index(&index)?;
        let (reply, receiver) = oneshot::channel();
        self.metadata
            .sender
            .send(Request::Write {
                index: index.clone(),
                urgency: Urgency::Now,
                prepare: Some(Box::new(prepare)),
                undo,
                reply: Some(reply),
            })
            .map_err(|_| Metadata::stopped())?;
        self.next_commit += 1;
        self.pending_commit = Some(self.next_commit);
        Ok(PendingCommit {
            token: self.next_commit,
            base: self.index.clone(),
            candidate: index,
            receiver,
            value,
        })
    }
    /// Apply only an acknowledged structural change. Newer, unrelated UI fields survive.
    /// A failed publication is rolled back by the same serial worker; sources stay intact.
    pub fn complete<T>(&mut self, completed: CompletedCommit<T>) -> io::Result<T> {
        let CompletedCommit {
            pending,
            outcome: (published, result),
        } = completed;
        if self.pending_commit != Some(pending.token) {
            return Err(io::Error::other(
                "This note changed in the meantime. Try again.",
            ));
        }
        self.pending_commit = None;
        if result.is_ok() {
            let mut candidate = pending.candidate.clone();
            for note in &mut candidate.notes {
                if let Some(base) = pending.base.notes.iter().find(|old| old.id == note.id)
                    && let Some(current) = self.index.notes.iter().find(|new| new.id == note.id)
                {
                    rebase_note(note, base, current);
                }
            }
            if candidate.defaults == pending.base.defaults {
                candidate.defaults = self.index.defaults.clone();
            }
            if candidate.glass_tint == pending.base.glass_tint {
                candidate.glass_tint = self.index.glass_tint;
            }
            if candidate.arrange_policy == pending.base.arrange_policy {
                candidate.arrange_policy = self.index.arrange_policy;
            }
            self.index = candidate;
        }
        if self.index != pending.candidate || (published && result.is_err()) {
            // This also rolls back an index rename whose directory sync failed. The worker
            // tracks those published bytes, so outside edits are still never overwritten.
            self.metadata.queue(self.index.clone(), Urgency::Now)?;
        }
        result.map(|()| pending.value)
    }
    pub fn request_flush(&mut self) -> io::Result<PendingCommit<()>> {
        self.request_commit(self.index.clone(), || Ok(()), ())
    }
    pub fn request_create(&mut self, frame: Frame) -> io::Result<PendingCommit<NoteRecord>> {
        let mut note = NoteRecord::new(uuid::Uuid::new_v4().to_string(), frame);
        note.style = self.index.defaults.style.clone();
        note.floating = self.index.defaults.floating;
        let directory = self.root.join("notes").join(&note.id);
        let mut index = self.index.clone();
        index.notes.push(note.clone());
        let notes = self.root.join("notes");
        self.request_commit_creating(
            index,
            vec![directory.clone()],
            move || {
                NoteStore::open(directory)?.0.save("")?;
                sync_directory(&notes)
            },
            note,
        )
    }
    /// Remove a note from the index and keep its files, so a ghost or unwanted entry can be
    /// cleared. A marker in its folder stops the next launch listing it again.
    pub fn request_forget(&mut self, id: &str) -> io::Result<PendingCommit<()>> {
        let mut index = self.index.clone();
        let before = index.notes.len();
        index.notes.retain(|note| note.id != id);
        if index.notes.len() == before {
            return Err(io::Error::other("This note is no longer available."));
        }
        let directory = note_directory(&self.root, id)?;
        let marker = directory.join(FORGOTTEN);
        // Only a marker this call wrote is taken back, if the index then fails to save.
        let marked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let undo = {
            let (marker, marked) = (marker.clone(), marked.clone());
            Box::new(move || {
                if marked.load(std::sync::atomic::Ordering::Acquire)
                    && let Err(error) = fs::remove_file(&marker)
                    && error.kind() != io::ErrorKind::NotFound
                {
                    log::warn!(
                        "Could not remove {} after a failed change: {error}",
                        marker.display()
                    );
                }
            }) as Undo
        };
        self.request_commit_undoing(
            index,
            Some(undo),
            move || {
                // A ghost has no folder, so there is nothing to mark.
                if !directory.exists() {
                    return Ok(());
                }
                // Marked before writing, so a marker left half written is taken back too.
                marked.store(!marker.exists(), std::sync::atomic::Ordering::Release);
                match write_new(&marker, b"removed from Dahlia\n") {
                    Err(error) if error.kind() != io::ErrorKind::AlreadyExists => Err(error),
                    _ => Ok(()),
                }
            },
            (),
        )
    }
    pub fn request_update_many(&mut self, notes: Vec<NoteRecord>) -> io::Result<PendingCommit<()>> {
        let mut index = self.index.clone();
        let mut ids = std::collections::HashSet::new();
        for note in notes {
            if !ids.insert(note.id.clone()) {
                return Err(io::Error::other("Couldn’t save. Try again."));
            }
            let id = note.id.clone();
            *index
                .notes
                .iter_mut()
                .find(|old| old.id == id)
                .ok_or_else(|| io::Error::other("This note is no longer available."))? = note;
        }
        self.request_commit(index, || Ok(()), ())
    }
    pub fn request_transition(
        &mut self,
        mut note: NoteRecord,
        expected: NoteState,
        destination: NoteState,
    ) -> io::Result<PendingCommit<NoteRecord>> {
        let existing = self
            .note(&note.id)
            .ok_or_else(|| io::Error::other("This note is no longer available."))?;
        if existing.state != expected || expected == destination {
            return Err(io::Error::other(
                "This note changed in another window. Try again.",
            ));
        }
        note.state = destination;
        note.open = destination == NoteState::Active;
        note.state_changed_at = timestamp();
        let mut index = self.index.clone();
        *index
            .notes
            .iter_mut()
            .find(|old| old.id == note.id)
            .ok_or_else(|| io::Error::other("This note is no longer available."))? = note.clone();
        self.request_commit(index, || Ok(()), note)
    }
    pub fn request_restore(
        &mut self,
        id: &str,
        expected: NoteState,
    ) -> io::Result<PendingCommit<NoteRecord>> {
        if expected == NoteState::Active {
            return Err(io::Error::other("This note is already open."));
        }
        let note = self
            .note(id)
            .ok_or_else(|| io::Error::other("This note is no longer available."))?;
        self.request_transition(note, expected, NoteState::Active)
    }
    pub fn request_defaults(&mut self, note: &NoteRecord) -> io::Result<PendingCommit<()>> {
        let (width, height) = note.resting_size();
        let mut index = self.index.clone();
        index.defaults = Defaults {
            style: note.style.clone(),
            floating: note.floating,
            width: width.clamp(230., MAX_NOTE_SIZE),
            height: height.clamp(160., MAX_NOTE_SIZE),
        };
        self.request_commit(index, || Ok(()), ())
    }
    pub fn request_import(
        &mut self,
        sources: Vec<crate::transfer::Imported>,
        frame: Frame,
    ) -> io::Result<PendingCommit<Vec<NoteRecord>>> {
        let mut index = self.index.clone();
        let mut additions = Vec::new();
        let mut files = Vec::new();
        let mut frame = frame;
        for (position, source) in sources.into_iter().enumerate() {
            // Imported notes cascade like new ones instead of stacking exactly.
            if position > 0 {
                frame.x += 28.;
                frame.y += 28.;
            }
            let mut note = NoteRecord::new(uuid::Uuid::new_v4().to_string(), frame);
            note.style = self.index.defaults.style.clone();
            note.floating = self.index.defaults.floating;
            note.title = source.title;
            files.push((
                self.root.join("notes").join(&note.id),
                source.text,
                source.images,
            ));
            index.notes.push(note.clone());
            additions.push(note);
        }
        let notes = self.root.join("notes");
        let added = !files.is_empty();
        let created = files.iter().map(|(directory, ..)| directory.clone()).collect();
        self.request_commit_creating(
            index,
            created,
            move || {
                for (directory, text, images) in files {
                    let (mut store, _) = NoteStore::open(directory.clone())?;
                    crate::attachments::publish(&directory, &images)?;
                    store.save(&text)?;
                }
                if added {
                    sync_directory(&notes)?;
                }
                Ok(())
            },
            additions,
        )
    }
    pub fn request_copy(
        &mut self,
        original: &NoteRecord,
        text: String,
        directory: PathBuf,
    ) -> io::Result<PendingCommit<NoteRecord>> {
        let mut note = original.clone();
        note.id = uuid::Uuid::new_v4().to_string();
        note.frame.x += 28.;
        note.frame.y += 28.;
        note.state = NoteState::Active;
        note.modified_at = timestamp();
        note.state_changed_at = 0;
        note.open = true;
        let destination = self.root.join("notes").join(&note.id);
        let mut index = self.index.clone();
        index.notes.push(note.clone());
        let notes = self.root.join("notes");
        self.request_commit_creating(
            index,
            vec![destination.clone()],
            move || {
                let images = intact_assets(&directory)
                    .0
                    .into_iter()
                    .map(|(name, bytes)| (format!("attachments/{name}"), bytes))
                    .collect();
                let (mut store, _) = NoteStore::open(destination.clone())?;
                crate::attachments::publish(&destination, &images)?;
                store.save(&text)?;
                sync_directory(&notes)
            },
            note,
        )
    }
    pub fn request_restore_copies(
        &mut self,
        sources: Vec<RecoveredNote>,
    ) -> io::Result<PendingCommit<Vec<NoteRecord>>> {
        let mut index = self.index.clone();
        let mut additions = Vec::new();
        let mut files = Vec::new();
        for source in sources {
            let mut note = source.record;
            note.id = uuid::Uuid::new_v4().to_string();
            note.state = NoteState::Active;
            note.source_base = None;
            note.open = true;
            note.state_changed_at = timestamp();
            files.push((
                self.root.join("notes").join(&note.id),
                source.text,
                source.images,
            ));
            index.notes.push(note.clone());
            additions.push(note);
        }
        let notes = self.root.join("notes");
        let added = !files.is_empty();
        let created = files.iter().map(|(directory, ..)| directory.clone()).collect();
        self.request_commit_creating(
            index,
            created,
            move || {
                let schema = markraft_commonmark::commonmark_schema();
                for (_, text, _) in &files {
                    markraft_commonmark::SourceDocument::parse(&schema, text).map_err(|error| {
                        log::warn!("Cannot parse a note: {error}");
                        io::Error::other("A note couldn’t be read as Markdown.")
                    })?;
                }
                for (directory, text, images) in files {
                    let (mut store, _) = NoteStore::open(directory.clone())?;
                    crate::attachments::publish(&directory, &images)?;
                    store.save(&text)?;
                }
                if added {
                    sync_directory(&notes)?;
                }
                Ok(())
            },
            additions,
        )
    }
}
