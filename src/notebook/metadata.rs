//! The metadata writer thread: coalesced, atomic index writes and their acknowledgements.
use super::*;

/// Whether `new` differs from `old` only in what a typing pause changes.
pub(super) fn cosmetic_only(old: &NoteRecord, new: &NoteRecord) -> bool {
    *old == NoteRecord {
        title: old.title.clone(),
        modified_at: old.modified_at,
        ..new.clone()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Urgency {
    /// Write as soon as the thread is free, coalescing a burst into one write.
    Now,
    /// Write after `IDLE` without newer requests, or `MAX_DEFER` at the latest.
    Idle,
}
pub(super) type Prepare = Box<dyn FnOnce() -> io::Result<()> + Send>;
/// Runs on the metadata thread when a change never reached the index, to remove the
/// folders its `Prepare` created.
pub(super) type Undo = Box<dyn FnOnce() + Send>;
/// Whether the index reached disk, and how the write went. A failed directory sync can
/// leave the new index in place, which memory must then follow.
pub(super) type Outcome = (bool, io::Result<()>);
pub(super) enum Request {
    Write {
        index: Index,
        urgency: Urgency,
        prepare: Option<Prepare>,
        undo: Option<Undo>,
        reply: Option<oneshot::Sender<Outcome>>,
    },
    Checkpoint {
        index: Index,
        reply: oneshot::Sender<io::Result<PathBuf>>,
    },
    #[cfg(test)]
    Barrier(mpsc::Sender<()>),
    Stop,
}
/// The UI side of the metadata thread, which owns every index and checkpoint write.
pub(super) struct Metadata {
    pub(super) sender: mpsc::Sender<Request>,
    pub(super) thread: Option<std::thread::JoinHandle<()>>,
}
impl Metadata {
    pub(super) fn start(
        root: PathBuf,
        saved: Option<Vec<u8>>,
        index: Index,
    ) -> (Self, UnboundedReceiver<String>) {
        let (problems, receiver) = futures::channel::mpsc::unbounded();
        let (sender, requests) = mpsc::channel();
        let worker = Worker {
            root,
            saved,
            written: index,
            backed_up: false,
            checkpoint_at: None,
            verified: Verified::default(),
            problems,
            reported: None,
            directory_sync_pending: false,
        };
        let thread = std::thread::Builder::new()
            .name("metadata".into())
            .spawn(move || worker.run(requests))
            .ok();
        (Self { sender, thread }, receiver)
    }
    pub(super) fn stopped() -> io::Error {
        io::Error::other(CommitError::WriterStopped)
    }
    pub(super) fn queue(&self, index: Index, urgency: Urgency) -> io::Result<()> {
        self.sender
            .send(Request::Write {
                index,
                urgency,
                prepare: None,
                undo: None,
                reply: None,
            })
            .map_err(|_| Self::stopped())
    }
    pub(super) fn wait<T>(&self, request: impl FnOnce(oneshot::Sender<T>) -> Request) -> io::Result<T> {
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(request(reply))
            .map_err(|_| Self::stopped())?;
        futures::executor::block_on(receiver).map_err(|_| Self::stopped())
    }
    /// Write `index`, then end the thread and wait for it.
    pub(super) fn stop(&mut self, lock: Option<File>) {
        let _ = self.sender.send(Request::Stop);
        if let Some(thread) = self.thread.take() {
            // Keep the library locked until the worker finishes, without joining on UI.
            std::thread::spawn(move || {
                if thread.join().is_err() {
                    log::error!("The notebook writer stopped unexpectedly.");
                }
                drop(lock);
            });
        }
    }
}
pub(super) struct Worker {
    pub(super) root: PathBuf,
    /// The index bytes this app last read or wrote; anything else on disk is an outside edit.
    pub(super) saved: Option<Vec<u8>>,
    /// The index `saved` holds, which a due checkpoint records.
    pub(super) written: Index,
    /// The previous index is backed up once per session, before the first write.
    pub(super) backed_up: bool,
    pub(super) checkpoint_at: Option<Instant>,
    pub(super) verified: Verified,
    pub(super) problems: UnboundedSender<String>,
    /// The last write failure reported, so a failing disk is reported once, not per write.
    pub(super) reported: Option<String>,
    pub(super) directory_sync_pending: bool,
}
impl Worker {
    pub(super) fn run(mut self, requests: mpsc::Receiver<Request>) {
        let mut deferred: Option<(Index, Instant)> = None;
        let mut backlog = std::collections::VecDeque::new();
        loop {
            let request = if let Some(request) = backlog.pop_front() {
                request
            } else if let Some((_, since)) = &deferred {
                let wait = IDLE.min(MAX_DEFER.saturating_sub(since.elapsed()));
                match requests.recv_timeout(wait) {
                    Ok(request) => request,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some((index, _)) = deferred.take() {
                            self.write_reporting(index);
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => Request::Stop,
                }
            } else {
                requests.recv().unwrap_or(Request::Stop)
            };
            match request {
                Request::Write {
                    index,
                    urgency: Urgency::Idle,
                    prepare: None,
                    undo: None,
                    reply: None,
                } => {
                    let since = deferred
                        .take()
                        .map_or_else(Instant::now, |(_, since)| since);
                    deferred = Some((index, since));
                }
                Request::Write {
                    mut index,
                    prepare,
                    undo,
                    mut reply,
                    ..
                } => {
                    // Every request carries the whole index, so a queued burst is written once.
                    if prepare.is_none() {
                        while let Ok(next) = requests.try_recv() {
                            match next {
                                Request::Write {
                                    index: newer,
                                    prepare: None,
                                    reply: newer_reply,
                                    ..
                                } if reply.is_none() => {
                                    index = newer;
                                    reply = newer_reply;
                                }
                                other => {
                                    backlog.push_back(other);
                                    break;
                                }
                            }
                        }
                    }
                    deferred = None;
                    match reply {
                        Some(reply) => {
                            let outcome = match prepare.map_or(Ok(()), |prepare| prepare()) {
                                Ok(()) => self.write(index),
                                Err(error) => (false, Err(error)),
                            };
                            if !outcome.0
                                && let Some(undo) = undo
                            {
                                undo();
                            }
                            let _ = reply.send(outcome);
                        }
                        None => self.write_reporting(index),
                    }
                }
                Request::Checkpoint { index, reply } => {
                    deferred = None;
                    self.write_reporting(index.clone());
                    let _ = reply.send(self.checkpoint(&index));
                }
                #[cfg(test)]
                Request::Barrier(reply) => {
                    let _ = reply.send(());
                }
                Request::Stop => {
                    if let Some((index, _)) = deferred.take() {
                        self.write_reporting(index);
                    }
                    break;
                }
            }
        }
    }
    pub(super) fn report(&self, problem: String) {
        log::warn!("{problem}");
        let _ = self.problems.unbounded_send(problem);
    }
    pub(super) fn write_reporting(&mut self, index: Index) {
        if let (_, Err(error)) = self.write(index) {
            let problem = crate::copy::failed("Couldn’t save Dahlia’s list of notes.", &error);
            if self.reported.as_ref() != Some(&problem) {
                self.report(problem.clone());
                self.reported = Some(problem);
            }
        }
    }
    pub(super) fn write(&mut self, index: Index) -> Outcome {
        let bytes = match serde_json::to_vec_pretty(&index) {
            Ok(bytes) => bytes,
            Err(error) => {
                log::warn!("Cannot encode the list of notes: {error}");
                return (
                    false,
                    Err(io::Error::other("Dahlia’s list of notes can’t be saved.")),
                );
            }
        };
        if self.saved.as_deref() == Some(&bytes[..]) {
            self.written = index;
            let result = if self.directory_sync_pending {
                sync_directory(&self.root)
            } else {
                Ok(())
            };
            self.directory_sync_pending = result.is_err();
            return (true, result);
        }
        let path = self.root.join("index.json");
        let disk = match read_regular(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return (false, Err(error)),
        };
        if disk != self.saved {
            // A second writer (sync tool, backup restore, manual edit) must not lock every
            // later change out. Keep its bytes in Recovery, report it once, then own the file.
            let problem = match &disk {
                Some(disk) => {
                    if let Err(error) = preserve_index_bytes(&self.root, disk) {
                        log::warn!("Could not preserve the externally changed index: {error}");
                    }
                    "Another app changed Dahlia’s list of notes. Dahlia kept that version in Recovery and saved its own."
                }
                None => "Another app removed Dahlia’s list of notes. Dahlia saved it again.",
            };
            self.report(problem.into());
            self.saved = disk;
        }
        if !self.backed_up {
            // Never replace a good metadata backup with the corrupt startup index.
            // The backup is a convenience: an unwritable one is reported once, never a reason
            // to refuse the real write.
            if let Some(previous) = self
                .saved
                .as_deref()
                .filter(|bytes| parse_index(bytes).is_ok())
                && let Err(error) = atomic_write(&self.root.join("index.backup.json"), previous)
            {
                self.report(crate::copy::failed(
                    "Couldn’t back up Dahlia’s list of notes. Your notes were saved.",
                    &error,
                ));
            }
            self.backed_up = true;
        }
        if !self.written.notes.is_empty()
            && self
                .checkpoint_at
                .is_none_or(|at| at.elapsed() >= CHECKPOINT_INTERVAL)
        {
            // Recovery history must never block saving, closing or quitting.
            let previous = self.written.clone();
            if let Err(error) = self.checkpoint(&previous) {
                self.checkpoint_at = Some(Instant::now());
                self.report(crate::copy::failed(
                    "Couldn’t save a recovery snapshot. Your notes were saved.",
                    &error,
                ));
            }
        }
        let result = atomic_write(&path, &bytes);
        let published = result.is_ok() || fs::read(&path).is_ok_and(|disk| disk == bytes);
        if published {
            self.saved = Some(bytes);
            self.written = index;
            self.directory_sync_pending = result.is_err();
        }
        if result.is_ok() {
            self.reported = None;
        }
        (published, result)
    }
}
