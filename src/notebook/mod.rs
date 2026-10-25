#[cfg(test)]
use crate::appearance::ColorIndex;
use crate::window_commands::Policy;
use crate::appearance::{DEFAULT_TINT, Defaults, NoteStyle, clamp_tint};
use crate::storage::{
    FileKey, NoteStore, atomic_write, clone_new, open_regular, read_regular, reject_symlink,
    sync_directory, timestamp, write_new, write_new_unflushed,
};
use futures::channel::{
    mpsc::{UnboundedReceiver, UnboundedSender},
    oneshot,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};


mod checkpoint;
mod commit;
mod metadata;
mod recovery;
#[cfg(test)]
mod tests;

use checkpoint::*;
pub use commit::*;
use metadata::*;
pub use recovery::*;

pub const STRIP_HEIGHT: f32 = 12.0; // Matches Swift Ortensia; Apple perimeter parity is separately unmeasured.
pub const MAX_NOTE_SIZE: f32 = 32768.;
/// How far each new note is offset from the front one.
const CASCADE_STEP: f32 = 28.;
const CASCADE_LAP: f32 = 14.;
const CASCADE_TRIES: usize = 256;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NoteState {
    #[default]
    Active,
    Archived,
    Trash,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            x: 120.,
            y: 120.,
            width: 360.,
            height: 350.,
        }
    }
}
impl Frame {
    /// The display this frame belongs on: the one with the greatest overlap, else the nearest,
    /// preferring the first listed (the primary) on ties.
    fn home<'a>(&self, displays: &'a [Frame]) -> Option<&'a Frame> {
        let frame = self;
        let overlap = |display: &Frame| {
            (frame.x + frame.width)
                .min(display.x + display.width)
                .sub(frame.x.max(display.x))
                .max(0.)
                * (frame.y + frame.height)
                    .min(display.y + display.height)
                    .sub(frame.y.max(display.y))
                    .max(0.)
        };
        // Squared gap between the rectangles; zero when they touch or overlap.
        let distance = |display: &Frame| {
            let dx = (display.x - (frame.x + frame.width))
                .max(frame.x - (display.x + display.width))
                .max(0.);
            let dy = (display.y - (frame.y + frame.height))
                .max(frame.y - (display.y + display.height))
                .max(0.);
            dx * dx + dy * dy
        };
        let mut best: Option<(&Frame, f32, f32)> = None;
        for display in displays.iter().filter(|d| {
            [d.x, d.y, d.width, d.height].iter().all(|n| n.is_finite())
                && d.width > 0.
                && d.height > 0.
        }) {
            let (area, gap) = (overlap(display), distance(display));
            // Strict comparisons keep the earlier display on ties.
            if best.is_none_or(|(_, best_area, best_gap)| {
                area > best_area || (area == best_area && gap < best_gap)
            }) {
                best = Some((display, area, gap));
            }
        }
        best.map(|(display, _, _)| display)
    }
    /// This strip grown to `height` for expanding: its top edge and sides stay, and the note
    /// slides up only as far as it must to end above the bottom of its display's visible area
    /// (a note taller than the display is cut to it). Frames and displays share global
    /// top-left coordinates.
    pub fn expanded_to(self, height: f32, displays: &[Frame]) -> Self {
        let mut frame = Self { height, ..self };
        if let Some(display) = self.home(displays) {
            frame.height = height.min(display.height);
            frame.y = frame
                .y
                .min(display.y + display.height - frame.height)
                .max(display.y);
        }
        frame
    }
    /// Where the next note opens, given this frame (the front note's position at the new note's
    /// size): one step down and right of it, as on macOS. Past the edge of the visible area
    /// the cascade wraps to the top-left of the display, like `cascadeTopLeftFromPoint:`, and
    /// steps on from any position another note (`taken`) already holds, so notes never pile
    /// up in a corner or on each other. Frames and displays share global top-left coordinates.
    pub fn cascaded(self, displays: &[Frame], taken: &[Frame]) -> Self {
        let Some(display) = self.home(displays) else {
            return Self {
                x: self.x + CASCADE_STEP,
                y: self.y + CASCADE_STEP,
                ..self
            };
        };
        let fits = |frame: &Frame| {
            frame.x + frame.width <= display.x + display.width
                && frame.y + frame.height <= display.y + display.height
        };
        let mut laps = 0;
        let mut next = self;
        // Bounded: a note too big for its display never fits, and every spot may be held.
        for _ in 0..CASCADE_TRIES {
            next = Self {
                x: next.x + CASCADE_STEP,
                y: next.y + CASCADE_STEP,
                ..next
            };
            if !fits(&next) {
                // Each further lap starts a little to the right of the last, so a long run of
                // notes keeps its own diagonals instead of landing on the first lap's.
                next = Self {
                    x: display.x + CASCADE_STEP + laps as f32 * CASCADE_LAP,
                    y: display.y + CASCADE_STEP,
                    ..next
                };
                laps += 1;
            }
            let held = taken
                .iter()
                .any(|other| (other.x - next.x).abs() < 1. && (other.y - next.y).abs() < 1.);
            if !held {
                break;
            }
        }
        next
    }
    /// Choose the display with the greatest overlap, else the nearest one, preferring the
    /// first listed (the primary) on ties; then keep the entire note visible on it.
    /// Frames and displays share global top-left coordinates.
    pub fn restored(self, displays: &[Frame], collapsed: bool) -> Self {
        let mut frame = if [self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite())
        {
            self
        } else {
            Self::default()
        };
        frame.width = frame.width.clamp(230., MAX_NOTE_SIZE);
        frame.height = if collapsed {
            STRIP_HEIGHT
        } else {
            frame.height.clamp(160., MAX_NOTE_SIZE)
        };
        if let Some(display) = frame.home(displays) {
            frame.width = frame.width.min(display.width);
            frame.height = frame.height.min(display.height);
            frame.x = frame
                .x
                .clamp(display.x, display.x + display.width - frame.width);
            frame.y = frame
                .y
                .clamp(display.y, display.y + display.height - frame.height);
        }
        frame
    }
}
use std::ops::Sub;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NoteRecord {
    pub id: String,
    pub title: String,
    pub open: bool,
    pub frame: Frame,
    pub expanded_height: f32,
    pub collapsed: bool,
    pub floating: bool,
    #[serde(default = "NoteStyle::legacy")]
    pub style: NoteStyle,
    #[serde(default)]
    pub state: NoteState,
    #[serde(default)]
    pub state_changed_at: u64,
    #[serde(default)]
    pub source_base: Option<PathBuf>,
    #[serde(default)]
    pub modified_at: u64,
    /// Frame to return to when the zoom toggle is used again, including across launches.
    #[serde(default)]
    pub zoom_restore: Option<Frame>,
}
impl NoteRecord {
    fn new(id: String, frame: Frame) -> Self {
        Self {
            id,
            title: "Untitled".into(),
            open: true,
            frame,
            expanded_height: frame.height,
            collapsed: false,
            floating: false,
            style: NoteStyle::legacy(),
            state: NoteState::Active,
            state_changed_at: 0,
            source_base: None,
            modified_at: crate::storage::timestamp(),
            zoom_restore: None,
        }
    }
    /// Save the expanded geometry (x, y, width and the expanded height as the frame's height)
    /// before the window is zoomed, whether or not it is collapsed when the zoom ends. A strip
    /// is not resizable, so it does not zoom: `false` leaves it alone, as it does a note that
    /// is already zoomed. Capture the window first so the record is current.
    pub fn begin_zoom(&mut self) -> bool {
        if self.collapsed || self.zoom_restore.is_some() {
            return false;
        }
        self.zoom_restore = Some(Frame {
            height: self.expanded_height,
            ..self.frame
        });
        true
    }
    /// Take the saved pre-zoom geometry back as the note's own; `None` when the note is not
    /// zoomed. Its height always becomes the expanded height, and the frame keeps the strip
    /// height while the note is collapsed.
    pub fn unzoom(&mut self) -> Option<Frame> {
        let mut frame = self.zoom_restore.take()?;
        // A strip height saved by an earlier build would shrink the note on expanding.
        frame.height = frame.height.clamp(160., MAX_NOTE_SIZE);
        self.expanded_height = frame.height;
        if self.collapsed {
            frame.height = STRIP_HEIGHT;
        }
        self.frame = frame;
        Some(frame)
    }
    /// The width and expanded height the note settles at: its pre-zoom size while zoomed,
    /// so a screen-filling zoom never becomes the default for new notes.
    pub fn resting_size(&self) -> (f32, f32) {
        match self.zoom_restore {
            Some(saved) => (saved.width, saved.height),
            None => (self.frame.width, self.expanded_height),
        }
    }
    /// Place the note by an arrangement. Its zoom is over: the saved frame belongs to a
    /// position the note no longer has.
    pub fn rearrange(&mut self, frame: Frame, expanded_height: f32, collapsed: bool) {
        self.frame = frame;
        self.expanded_height = expanded_height;
        self.collapsed = collapsed;
        self.zoom_restore = None;
    }
    pub fn restored(&self, displays: &[Frame]) -> Self {
        let mut note = self.clone();
        note.expanded_height = if note.expanded_height.is_finite() {
            note.expanded_height.clamp(160., MAX_NOTE_SIZE)
        } else {
            350.
        };
        note.frame.height = if note.collapsed {
            STRIP_HEIGHT
        } else {
            note.expanded_height
        };
        note.frame = note.frame.restored(displays, note.collapsed);
        // The saved geometry is the expanded one even while the note is a strip.
        note.zoom_restore = note.zoom_restore.map(|frame| frame.restored(displays, false));
        note
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Index {
    version: u32,
    notes: Vec<NoteRecord>,
    #[serde(default)]
    defaults: Defaults,
    #[serde(default = "default_tint")]
    glass_tint: f32,
    #[serde(default)]
    arrange_policy: Policy,
}
fn default_tint() -> f32 {
    DEFAULT_TINT
}

/// Typing pauses change only a note's modified time and title, both rebuilt from its source;
/// their metadata waits for this long a quiet moment instead of costing a durable write each.
const IDLE: Duration = Duration::from_secs(2);
/// The longest that metadata waits while typing never pauses for `IDLE`.
const MAX_DEFER: Duration = Duration::from_secs(30);
/// Metadata writes checkpoint the library at most this often.
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(300);

pub struct Notebook {
    root: PathBuf,
    index: Index,
    metadata: Metadata,
    problems: Option<UnboundedReceiver<String>>,
    recovery_notice: Option<String>,
    pending_commit: Option<u64>,
    next_commit: u64,
    // Drop transfers this lock to the worker reaper, retaining exclusion until I/O ends.
    _lock: Option<File>,
}
impl Notebook {
    pub fn open(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        reject_symlink(&root.join("library.lock"))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("library.lock"))?;
        lock.try_lock().map_err(io::Error::from)?;
        let root = root.canonicalize()?;
        reject_symlink(&root.join("notes"))?;
        reject_symlink(&root.join("recovery"))?;
        let path = root.join("index.json");
        let saved_index = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        reject_symlink(&path)?;
        let mut recovery_notice = None;
        let mut index = match saved_index.as_deref().map(parse_index).transpose() {
            Ok(Some(index)) => index,
            Ok(None) => empty_index(),
            // Never rebuild or overwrite notes a newer Dahlia saved; they open there again.
            Err(error) if is_newer_version(&error) => return Err(error),
            Err(error) => {
                // Keeping the unreadable bytes is best effort: it must never stop the app opening.
                if let Err(problem) =
                    preserve_index_bytes(&root, saved_index.as_deref().unwrap_or_default())
                {
                    log::warn!("Could not preserve the unreadable list of notes: {problem}");
                }
                recovery_notice = Some(format!(
                    "Dahlia couldn’t read its list of notes. {} It rebuilt the list from your files, and the notes it found are in Archived Notes.",
                    crate::copy::plain(&error)
                ));
                read_regular(&root.join("index.backup.json"))
                    .ok()
                    .and_then(|bytes| parse_index(&bytes).ok())
                    .unwrap_or_else(empty_index)
            }
        };
        // Recover unindexed sources as archived; never invent blank replacement sources.
        let known = index.notes.iter().map(|note| note.id.clone()).collect();
        for (id, source) in scan_sources(&root, &known) {
            let mut note = NoteRecord::new(id, Frame::default());
            note.title = source_title(&source);
            note.state = NoteState::Archived;
            note.open = false;
            index.notes.push(note);
        }
        if !path.exists() && index.notes.iter().any(|note| note.id != "primary") {
            recovery_notice = Some("Dahlia’s list of notes was missing, so it rebuilt it from your files. Notes it found are in Archived Notes.".into());
        }
        let (metadata, problems) = Metadata::start(root.clone(), saved_index, index.clone());
        let mut library = Self {
            root,
            index,
            metadata,
            problems: Some(problems),
            recovery_notice,
            pending_commit: None,
            next_commit: 0,
            _lock: Some(lock),
        };
        if !path.exists() && library.root.join("note.md").exists() {
            library.commit(Index {
                version: 1,
                notes: library
                    .index
                    .notes
                    .iter()
                    .cloned()
                    .map(|mut note| {
                        if note.id == "primary" {
                            note.state = NoteState::Active;
                            note.open = true;
                        }
                        note
                    })
                    .collect(),
                defaults: Defaults::default(),
                glass_tint: DEFAULT_TINT,
                arrange_policy: Policy::default(),
            })?;
        }
        Ok(library)
    }
    pub fn notes(&self) -> &[NoteRecord] {
        &self.index.notes
    }
    pub fn note(&self, id: &str) -> Option<NoteRecord> {
        self.index.notes.iter().find(|n| n.id == id).cloned()
    }
    pub fn open_store(&self, id: &str) -> io::Result<(NoteStore, String)> {
        if self.note(id).is_none() {
            return Err(io::Error::other("This note is no longer available."));
        }
        let directory = note_directory(&self.root, id)?;
        // A missing source is recoverable damage, never permission to publish a blank note.
        read_regular(&directory.join("note.md"))?;
        NoteStore::open(directory)
    }
    /// A note's saved source and the folder its relative images resolve against,
    /// read without taking the note's lock.
    #[cfg(test)]
    pub fn saved_source(&self, id: &str) -> io::Result<(String, PathBuf)> {
        let note = self
            .note(id)
            .ok_or_else(|| io::Error::other("This note is no longer available."))?;
        Self::read_saved_source(&self.root, note)
    }
    pub fn read_saved_source(root: &Path, note: NoteRecord) -> io::Result<(String, PathBuf)> {
        let directory = note_directory(root, &note.id)?;
        let text = String::from_utf8(read_regular(&directory.join("note.md"))?)
            .map_err(|_| io::Error::other("This note isn’t text Dahlia can read."))?;
        Ok((text, note.source_base.unwrap_or(directory)))
    }
    #[cfg(test)]
    pub fn create(&mut self, frame: Frame) -> io::Result<NoteRecord> {
        let pending = self.request_create(frame)?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    /// Change one note's metadata in memory and queue its write on the metadata thread.
    /// Write failures arrive later through `take_problems`; use `update_durable` when the
    /// caller must know the record is on disk.
    pub fn update(&mut self, note: NoteRecord) -> io::Result<()> {
        let Some(existing) = self.index.notes.iter_mut().find(|n| n.id == note.id) else {
            return Err(io::Error::other("This note is no longer available."));
        };
        if *existing == note {
            return Ok(());
        }
        if !note.style.valid() {
            return Err(io::Error::other("Dahlia’s list of notes is damaged."));
        }
        let urgency = if cosmetic_only(existing, &note) {
            Urgency::Idle
        } else {
            Urgency::Now
        };
        *existing = note;
        if self.pending_commit.is_some() {
            Ok(()) // Completion rebases these newer fields before queueing another snapshot.
        } else {
            self.metadata.queue(self.index.clone(), urgency)
        }
    }
    /// `update`, waiting until the record is durably written.
    #[cfg(test)]
    pub fn update_durable(&mut self, note: NoteRecord) -> io::Result<()> {
        let pending = self.request_update_many(vec![note])?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    #[cfg(test)]
    pub fn update_many(&mut self, notes: Vec<NoteRecord>) -> io::Result<()> {
        let pending = self.request_update_many(notes)?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    /// Files stay in place, retaining source, relative resources and stable identities.
    #[cfg(test)]
    pub fn transition(
        &mut self,
        note: NoteRecord,
        expected: NoteState,
        destination: NoteState,
    ) -> io::Result<NoteRecord> {
        let pending = self.request_transition(note, expected, destination)?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    #[cfg(test)]
    pub fn restore(&mut self, id: &str, expected: NoteState) -> io::Result<NoteRecord> {
        let pending = self.request_restore(id, expected)?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    #[cfg(test)]
    pub fn import_sources(
        &mut self,
        sources: Vec<crate::transfer::Imported>,
        frame: Frame,
    ) -> io::Result<Vec<NoteRecord>> {
        let pending = self.request_import(sources, frame)?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    #[cfg(test)]
    pub fn forget(&mut self, id: &str) -> io::Result<()> {
        let pending = self.request_forget(id)?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    pub fn defaults(&self) -> &Defaults {
        &self.index.defaults
    }
    pub fn glass_tint(&self) -> f32 {
        self.index.glass_tint
    }
    /// Stage continuous changes in memory; the host coalesces index writes.
    pub fn stage_glass_tint(&mut self, tint: f32) {
        self.index.glass_tint = clamp_tint(tint);
    }
    pub fn arrange_policy(&self) -> Policy {
        self.index.arrange_policy
    }
    /// Stage the "When Arranging" choice in memory and queue the index write.
    pub fn set_arrange_policy(&mut self, policy: Policy) -> io::Result<()> {
        if self.index.arrange_policy == policy {
            return Ok(());
        }
        self.index.arrange_policy = policy;
        self.save_index()
    }
    /// Queue the in-memory index for writing on the metadata thread.
    pub fn save_index(&mut self) -> io::Result<()> {
        if self.pending_commit.is_some() {
            Ok(())
        } else {
            self.metadata.queue(self.index.clone(), Urgency::Now)
        }
    }
    /// Write the in-memory index and wait until it is durable, as quitting must.
    #[cfg(test)]
    pub fn flush(&mut self) -> io::Result<()> {
        let pending = self.request_flush()?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    #[cfg(test)]
    pub fn use_as_default(&mut self, note: &NoteRecord) -> io::Result<()> {
        let pending = self.request_defaults(note)?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
    /// Write `index` and wait until it is durable; on failure memory keeps the previous index.
    fn commit(&mut self, index: Index) -> io::Result<()> {
        self.commit_with(index, || Ok(()))
    }
    /// `commit`, after `prepare` has written the files the new index refers to.
    /// Synchronous startup/test compatibility; UI callers use request_commit instead.
    fn commit_with(
        &mut self,
        index: Index,
        prepare: impl FnOnce() -> io::Result<()> + Send + 'static,
    ) -> io::Result<()> {
        if self.pending_commit.is_some() {
            return Err(Self::busy());
        }
        validate_index(&index)?;
        let (published, result) = self.metadata.wait(|reply| Request::Write {
            index: index.clone(),
            urgency: Urgency::Now,
            prepare: Some(Box::new(prepare)),
            undo: None,
            reply: Some(reply),
        })?;
        if published {
            self.index = index;
        }
        result
    }
    /// Failures from queued metadata writes and checkpoints, for the host to show.
    /// Explicitly drain native I/O when a test will inspect/reopen/delete its fixture.
    #[cfg(test)]
    pub(crate) fn finish_for_test(mut self) {
        if self.pending_commit.is_none() {
            let _ = self.metadata.queue(self.index.clone(), Urgency::Now);
        }
        let _ = self.metadata.sender.send(Request::Stop);
        if let Some(thread) = self.metadata.thread.take() {
            thread.join().unwrap();
        }
        self._lock.take();
    }
    #[cfg(test)]
    pub fn barrier(&self) {
        let (reply, receiver) = mpsc::channel();
        if self.metadata.sender.send(Request::Barrier(reply)).is_ok() {
            let _ = receiver.recv();
        }
    }
    pub fn take_problems(&mut self) -> Option<UnboundedReceiver<String>> {
        self.problems.take()
    }
    /// The next reported problem, while the host has not taken the channel.
    #[cfg(test)]
    pub fn take_problem(&mut self) -> Option<String> {
        self.problems.as_mut()?.try_recv().ok()
    }
}
impl Drop for Notebook {
    fn drop(&mut self) {
        if self.pending_commit.is_none() {
            let _ = self.metadata.queue(self.index.clone(), Urgency::Now);
        }
        self.metadata.stop(self._lock.take());
    }
}
