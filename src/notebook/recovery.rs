//! Recovery: listing, previewing and restoring checkpoints, history and archived or deleted notes.
use super::*;

pub(super) fn read_open(file: impl io::Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut file = file;
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}
/// The managed image names in a note's attachments folder.
pub(super) fn managed_names(directory: &Path) -> Vec<String> {
    let folder = directory.join("attachments");
    let Ok(entries) = reject_symlink(&folder).and_then(|()| fs::read_dir(&folder)) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| managed_filename(name))
        .collect()
}

/// Recovery provenance decides the read protocol, never the current path's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryKind {
    Source,
    Metadata,
    Checkpoint,
}
impl RecoveryKind {
    pub fn is_snapshot(self) -> bool {
        self != Self::Source
    }
}
#[derive(Clone, Debug)]
pub struct RecoveryEntry {
    pub path: PathBuf,
    pub title: String,
    pub modified_at: u64,
    pub kind: RecoveryKind,
    pub note: Option<NoteRecord>,
}
#[derive(Clone)]
pub struct RecoveredNote {
    pub record: NoteRecord,
    pub text: String,
    pub images: BTreeMap<String, Vec<u8>>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Checkpoint {
    pub(super) version: u32,
    pub(super) created_at: u64,
    pub(super) index: Index,
    pub(super) sources: BTreeMap<String, SourceIntegrity>,
    /// Notes whose sources could not be read when the checkpoint was taken.
    #[serde(default)]
    pub(super) skipped: Vec<String>,
}
#[derive(Default)]
pub(super) struct Skipped {
    pub(super) notes: usize,
    pub(super) images: usize,
}
impl Skipped {
    pub(super) fn describe(&self) -> String {
        let count = |n: usize, noun: &str| format!("{n} {noun}{}", if n == 1 { "" } else { "s" });
        let what = match (self.notes, self.images) {
            (0, images) => count(images, "image"),
            (notes, 0) => count(notes, "note"),
            (notes, images) => format!("{} and {}", count(notes, "note"), count(images, "image")),
        };
        format!(
            "The snapshot left out {what} that couldn’t be read or that another app changed. Everything else was saved."
        )
    }
}
#[derive(Serialize, Deserialize)]
pub(super) struct SourceIntegrity {
    pub(super) sha256: String,
    pub(super) assets: BTreeMap<String, String>,
}
pub(super) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub(super) fn empty_index() -> Index {
    Index {
        version: 1,
        notes: vec![],
        defaults: Defaults::default(),
        glass_tint: DEFAULT_TINT,
        arrange_policy: Policy::default(),
    }
}
/// The marker a forgotten note's folder holds, so the next launch does not list it again.
pub(super) const FORGOTTEN: &str = ".forgotten";
/// Keep a distinct copy of index bytes in recovery, bounded to the newest twenty.
/// The same bytes are never stored twice, so repeated launches stay quiet.
pub(super) fn preserve_index_bytes(root: &Path, bytes: &[u8]) -> io::Result<()> {
    let problems = root.join("recovery").join("preserved");
    reject_symlink(&root.join("recovery"))?;
    reject_symlink(&problems)?;
    fs::create_dir_all(&problems)?;
    let preserved_indexes = || -> io::Result<Vec<PathBuf>> {
        Ok(fs::read_dir(&problems)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("index-"))
            })
            .collect())
    };
    let wanted = digest(bytes);
    if !preserved_indexes()?
        .iter()
        .any(|path| read_regular(path).is_ok_and(|existing| digest(&existing) == wanted))
    {
        write_new(
            &problems.join(format!(
                "index-{}-{}.json",
                timestamp(),
                uuid::Uuid::new_v4()
            )),
            bytes,
        )?;
    }
    let mut preserved = preserved_indexes()?;
    preserved.sort();
    let excess = preserved.len().saturating_sub(20);
    for path in preserved.into_iter().take(excess) {
        if let Err(error) = reject_symlink(&path).and_then(|()| fs::remove_file(&path)) {
            log::warn!("Could not trim preserved index {}: {error}", path.display());
        }
    }
    Ok(())
}
pub(super) fn valid_id(id: &str) -> bool {
    id == "primary" || uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
}
/// The list of notes was saved by a newer Dahlia. Unlike damage it is not repaired: opening
/// it would rebuild the list and overwrite the newer one, so the notes are left untouched.
#[derive(Debug, thiserror::Error)]
#[error("These notes were saved by a newer version of Dahlia. Update Dahlia to open them.")]
pub struct NewerVersion;
/// Whether `error` is `NewerVersion`, from `Notebook::open`.
pub fn is_newer_version(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|inner| inner.is::<NewerVersion>())
}
pub(super) fn parse_index(bytes: &[u8]) -> io::Result<Index> {
    // A newer version may have changed the list's shape, so read its version first.
    #[derive(Deserialize)]
    struct Versioned {
        version: u64,
    }
    if serde_json::from_slice::<Versioned>(bytes).is_ok_and(|saved| saved.version > 1) {
        return Err(io::Error::other(NewerVersion));
    }
    let index: Index = serde_json::from_slice(bytes).map_err(|error| {
        log::warn!("Cannot read the list of notes: {error}");
        io::Error::other("Dahlia’s list of notes is damaged.")
    })?;
    validate_index(&index)?;
    Ok(index)
}
pub(super) fn validate_index(index: &Index) -> io::Result<()> {
    if index.version > 1 {
        return Err(io::Error::other(NewerVersion));
    }
    if index.version != 1 {
        return Err(io::Error::other("Dahlia’s list of notes is damaged."));
    }
    if !index.defaults.style.valid()
        || !index.glass_tint.is_finite()
        || !(0. ..=0.5).contains(&index.glass_tint)
        || !index.defaults.width.is_finite()
        || !(230. ..=MAX_NOTE_SIZE).contains(&index.defaults.width)
        || !index.defaults.height.is_finite()
        || !(160. ..=MAX_NOTE_SIZE).contains(&index.defaults.height)
    {
        return Err(io::Error::other("Dahlia’s list of notes is damaged."));
    }
    let mut ids = std::collections::HashSet::new();
    for note in &index.notes {
        if !valid_id(&note.id) || !ids.insert(&note.id) || !note.style.valid() {
            return Err(io::Error::other("Dahlia’s list of notes is damaged."));
        }
    }
    Ok(())
}
pub(super) fn source_title(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Untitled")
        .chars()
        .take(80)
        .collect()
}
pub(super) fn note_directory(root: &Path, id: &str) -> io::Result<PathBuf> {
    if !valid_id(id) {
        return Err(io::Error::other("Dahlia’s list of notes is damaged."));
    }
    let directory = if id == "primary" {
        root.to_owned()
    } else {
        root.join("notes").join(id)
    };
    reject_symlink(&root.join("notes"))?;
    reject_symlink(&directory)?;
    Ok(directory)
}
/// Sources on disk that the index does not list, skipping any the person asked Dahlia to forget.
/// This runs on every launch, so an unreadable entry is skipped rather than stopping it.
pub(super) fn scan_sources(
    root: &Path,
    known: &std::collections::HashSet<String>,
) -> Vec<(String, Vec<u8>)> {
    let mut sources = Vec::new();
    if !known.contains("primary")
        && !root.join(FORGOTTEN).exists()
        && root.join("note.md").exists()
        && let Ok(bytes) = read_regular(&root.join("note.md"))
    {
        sources.push(("primary".into(), bytes));
    }
    let directory = root.join("notes");
    let Ok(entries) = reject_symlink(&directory).and_then(|()| fs::read_dir(&directory)) else {
        return sources;
    };
    for entry in entries.filter_map(Result::ok) {
        let id = entry.file_name().to_string_lossy().into_owned();
        if id == "primary"
            || !valid_id(&id)
            || known.contains(&id)
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
            || entry.path().join(FORGOTTEN).exists()
        {
            continue;
        }
        if let Ok(bytes) = read_regular(&entry.path().join("note.md")) {
            sources.push((id, bytes));
        }
    }
    sources
}
/// A recovered file that isn't UTF-8 text; the decoder's wording stays in the log.
fn not_text(error: std::string::FromUtf8Error) -> io::Error {
    log::warn!("Cannot read a recovered note as text: {error}");
    io::Error::other("This note isn’t text Dahlia can read.")
}
pub(super) fn safe_recovery_path(root: &Path, path: &Path) -> io::Result<()> {
    let canonical_root = root.canonicalize()?;
    let relative = path
        .strip_prefix(root)
        .or_else(|_| path.strip_prefix(&canonical_root))
        .map_err(|_| io::Error::other("This item can’t be restored."))?;
    let mut current = canonical_root;
    for component in relative.components() {
        if let std::path::Component::Normal(name) = component {
            current.push(name);
            reject_symlink(&current)?;
        } else {
            return Err(io::Error::other("This item can’t be restored."));
        }
    }
    Ok(())
}
pub(super) fn managed_filename(name: &str) -> bool {
    name.strip_suffix(".png").is_some_and(|stem| {
        stem.len() == 64
            && stem
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub(super) fn managed_assets(directory: &Path) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let folder = directory.join("attachments");
    reject_symlink(&folder)?;
    let mut images = BTreeMap::new();
    if !folder.exists() {
        return Ok(images);
    }
    for entry in fs::read_dir(&folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !managed_filename(&name) {
            continue;
        }
        let bytes = read_regular(&entry.path())?;
        if format!("{}.png", digest(&bytes)) != name {
            return Err(io::Error::other(
                "An image in this note was changed by another app.",
            ));
        }
        images.insert(format!("attachments/{name}"), bytes);
    }
    Ok(images)
}
/// Managed images whose bytes still match their names, and how many did not.
pub(super) fn intact_assets(directory: &Path) -> (Vec<(String, Vec<u8>)>, usize) {
    let folder = directory.join("attachments");
    let Ok(entries) = reject_symlink(&folder).and_then(|()| fs::read_dir(&folder)) else {
        return (Vec::new(), 0);
    };
    let mut images = Vec::new();
    let mut changed = 0;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !managed_filename(&name) {
            continue;
        }
        match read_regular(&entry.path()) {
            Ok(bytes) if format!("{}.png", digest(&bytes)) == name => images.push((name, bytes)),
            _ => changed += 1,
        }
    }
    (images, changed)
}
pub(super) fn load_checkpoint(root: &Path, entry: &RecoveryEntry) -> io::Result<Checkpoint> {
    safe_recovery_path(root, &entry.path)?;
    let checkpoint: Checkpoint =
        serde_json::from_slice(&read_regular(&entry.path.join("snapshot.json"))?).map_err(
            |error| {
                log::warn!("Cannot read a snapshot: {error}");
                io::Error::other("This snapshot can’t be read.")
            },
        )?;
    if checkpoint.version != 1 {
        return Err(io::Error::other("This snapshot can’t be read."));
    }
    validate_index(&checkpoint.index)?;
    if checkpoint.sources.len() != checkpoint.index.notes.len()
        || checkpoint
            .index
            .notes
            .iter()
            .any(|note| !checkpoint.sources.contains_key(&note.id))
    {
        return Err(io::Error::other("This snapshot is incomplete."));
    }
    Ok(checkpoint)
}

/// `result` if it succeeded. A failure is logged and skipped, so one unreadable entry never
/// hides everything else Recovery could offer.
fn skippable<T>(what: &str, result: io::Result<T>) -> Option<T> {
    result
        .inspect_err(|error| log::warn!("Recovery skipped {what}: {error}"))
        .ok()
}
/// Whether `folder` is an existing folder of Dahlia's own that can be listed, without aliases.
fn safe_listing(root: &Path, folder: &Path) -> bool {
    skippable("a folder", safe_recovery_path(root, folder)).is_some() && folder.exists()
}
/// The entries of `folder` that can be read, in no particular order.
fn listed(folder: &Path) -> Vec<fs::DirEntry> {
    skippable("a folder", fs::read_dir(folder))
        .into_iter()
        .flatten()
        .filter_map(|entry| skippable("an entry", entry))
        .collect()
}
impl Notebook {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn recovery_notice(&self) -> Option<&str> {
        self.recovery_notice.as_deref()
    }
    /// Write the in-memory index, then checkpoint the library on the metadata thread.
    #[cfg(test)]
    pub fn checkpoint(&mut self) -> io::Result<PathBuf> {
        futures::executor::block_on(self.request_checkpoint())
    }
    /// `checkpoint`, resolving once the checkpoint is published or has failed.
    pub fn request_checkpoint(
        &self,
    ) -> impl std::future::Future<Output = io::Result<PathBuf>> + use<> {
        let (reply, receiver) = oneshot::channel();
        let sent = if self.pending_commit.is_some() {
            Err(Self::busy())
        } else {
            self.metadata
                .sender
                .send(Request::Checkpoint {
                    index: self.index.clone(),
                    reply,
                })
                .map_err(|_| Metadata::stopped())
        };
        async move {
            sent?;
            receiver.await.map_err(|_| Metadata::stopped())?
        }
    }
    #[cfg(test)]
    pub fn recovery_entries(&self) -> io::Result<Vec<RecoveryEntry>> {
        Self::inventory(&self.root, &self.index.notes)
    }
    pub fn inventory(root: &Path, notes: &[NoteRecord]) -> io::Result<Vec<RecoveryEntry>> {
        let mut entries = Vec::new();
        for (path, title) in [
            (root.join("index.json"), "Current Note List"),
            (root.join("index.backup.json"), "Previous Note List"),
        ] {
            if path.exists() {
                entries.push(metadata_entry(path, title));
            }
        }
        for entry in fs::read_dir(root)? {
            let Some(entry) = skippable("a library entry", entry) else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("index") && name.ends_with(".pending") {
                entries.push(metadata_entry(entry.path(), "Incomplete Note List"));
            }
        }
        let preserved = root.join("recovery/preserved");
        if safe_listing(root, &preserved) {
            for entry in listed(&preserved) {
                if !entry.file_type().ok().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                if entry.file_name().to_string_lossy().starts_with("unsaved-") {
                    entries.push(RecoveryEntry {
                        modified_at: modified_at(&entry.path()),
                        path: entry.path(),
                        title: "Unsaved text from a failed save".to_owned(),
                        kind: RecoveryKind::Source,
                        note: None,
                    });
                } else {
                    entries.push(metadata_entry(entry.path(), "Damaged Note List"));
                }
            }
        }
        let snapshots = root.join("recovery/snapshots");
        if safe_listing(root, &snapshots) {
            for entry in listed(&snapshots) {
                if entry.file_type().ok().is_some_and(|kind| kind.is_dir()) {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    entries.push(RecoveryEntry {
                        modified_at: modified_at(&entry.path()),
                        path: entry.path(),
                        title: if name.starts_with('.') {
                            "Incomplete Snapshot".to_owned()
                        } else {
                            "Snapshot".to_owned()
                        },
                        kind: RecoveryKind::Checkpoint,
                        note: None,
                    });
                }
            }
        }
        let mut directories = vec![("primary".to_owned(), root.to_owned())];
        let folder = root.join("notes");
        if safe_listing(root, &folder) {
            for entry in listed(&folder) {
                let id = entry.file_name().to_string_lossy().into_owned();
                if valid_id(&id)
                    && id != "primary"
                    && entry.file_type().ok().is_some_and(|kind| kind.is_dir())
                {
                    directories.push((id, entry.path()));
                }
            }
        }
        for (id, directory) in directories {
            let record = notes.iter().find(|note| note.id == id).cloned();
            let history = directory.join("history");
            if safe_listing(root, &history) {
                for entry in listed(&history) {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "md") {
                        entries.push(source_entry(path, "Earlier Version", record.clone()));
                    }
                }
            }
            for entry in listed(&directory) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("note") && name.ends_with(".pending") {
                    entries.push(source_entry(
                        entry.path(),
                        "Unsaved Changes",
                        record.clone(),
                    ));
                }
            }
            if record.is_none() && directory.join("note.md").exists() {
                entries.push(source_entry(directory.join("note.md"), "Found Note", None));
            }
        }
        entries.sort_by(|a, b| {
            b.modified_at
                .cmp(&a.modified_at)
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok(entries)
    }
    pub fn recovery_records(root: &Path, entry: &RecoveryEntry) -> io::Result<Vec<NoteRecord>> {
        safe_recovery_path(root, &entry.path)?;
        if entry.kind.is_snapshot() {
            return Ok(if entry.kind == RecoveryKind::Metadata {
                parse_index(&read_regular(&entry.path)?)?.notes
            } else {
                load_checkpoint(root, entry)?.index.notes
            });
        }
        // A source history entry is one note; source bytes remain lazy until selected.
        let mut record = entry
            .note
            .clone()
            .unwrap_or_else(|| NoteRecord::new(uuid::Uuid::new_v4().to_string(), Frame::default()));
        if entry.note.is_none() {
            record.title = entry.title.clone();
        }
        Ok(vec![record])
    }
    pub fn preview_source(root: &Path, entry: &RecoveryEntry, id: &str) -> io::Result<String> {
        safe_recovery_path(root, &entry.path)?;
        let bytes = if entry.kind == RecoveryKind::Metadata {
            let index = parse_index(&read_regular(&entry.path)?)?;
            if !index.notes.iter().any(|note| note.id == id) {
                return Err(io::Error::other("This note isn’t in the selected item."));
            }
            read_regular(&note_directory(root, id)?.join("note.md"))?
        } else if entry.kind == RecoveryKind::Checkpoint {
            let checkpoint = load_checkpoint(root, entry)?;
            if !valid_id(id) {
                return Err(io::Error::other("Dahlia’s list of notes is damaged."));
            }
            let integrity = checkpoint
                .sources
                .get(id)
                .ok_or_else(|| io::Error::other("This note isn’t in the selected item."))?;
            let bytes = read_regular(&entry.path.join(format!("{id}.md")))?;
            if digest(&bytes) != integrity.sha256 {
                return Err(io::Error::other(
                    "This version was changed by another app and can’t be shown.",
                ));
            }
            bytes
        } else {
            read_regular(&entry.path)?
        };
        String::from_utf8(bytes).map_err(not_text)
    }
    pub fn export_snapshot(
        root: &Path,
        entry: &RecoveryEntry,
        destination: &Path,
    ) -> io::Result<()> {
        if !entry.kind.is_snapshot() {
            return Err(io::Error::other("Select a snapshot."));
        }
        safe_recovery_path(root, &entry.path)?;
        if entry.kind == RecoveryKind::Metadata {
            let bytes = read_regular(&entry.path)?;
            fs::create_dir(destination)?;
            return write_new(&destination.join("preserved-index.json"), &bytes)
                .inspect_err(|_| Self::discard_export(destination));
        }
        let checkpoint = load_checkpoint(root, entry)?;
        // Verify the complete bundle before creating the destination.
        let mut recovered = Vec::new();
        for note in &checkpoint.index.notes {
            recovered.push(Self::read_recovery_note(root, entry, &note.id)?);
        }
        fs::create_dir(destination)?;
        (|| {
            let assets = destination.join("assets");
            fs::create_dir(&assets)?;
            for note in recovered {
                write_new(
                    &destination.join(format!("{}.md", note.record.id)),
                    note.text.as_bytes(),
                )?;
                for (relative, bytes) in note.images {
                    let target = assets.join(
                        relative
                            .strip_prefix("attachments/")
                            .expect("managed image"),
                    );
                    if !target.exists() {
                        write_new(&target, &bytes)?;
                    }
                }
            }
            write_new(
                &destination.join("snapshot.json"),
                &read_regular(&entry.path.join("snapshot.json"))?,
            )?;
            sync_directory(destination)
        })()
        .inspect_err(|_| Self::discard_export(destination))
    }
    /// Remove the folder an export failed to finish; it was created by that export alone.
    fn discard_export(destination: &Path) {
        if let Err(error) = fs::remove_dir_all(destination) {
            log::warn!(
                "Could not remove the unfinished export {}: {error}",
                destination.display()
            );
        }
    }
    pub fn read_recovery_note(
        root: &Path,
        entry: &RecoveryEntry,
        id: &str,
    ) -> io::Result<RecoveredNote> {
        safe_recovery_path(root, &entry.path)?;
        if entry.kind == RecoveryKind::Metadata {
            let index = parse_index(&read_regular(&entry.path)?)?;
            let record = index
                .notes
                .into_iter()
                .find(|note| note.id == id)
                .ok_or_else(|| io::Error::other("This note isn’t in the selected item."))?;
            let directory = note_directory(root, id)?;
            let text = String::from_utf8(read_regular(&directory.join("note.md"))?)
                .map_err(not_text)?;
            let images = managed_assets(&directory)?;
            return Ok(RecoveredNote {
                record,
                text,
                images,
            });
        }
        if entry.kind.is_snapshot() {
            let checkpoint = load_checkpoint(root, entry)?;
            let record = checkpoint
                .index
                .notes
                .into_iter()
                .find(|note| note.id == id)
                .ok_or_else(|| io::Error::other("This note isn’t in the selected item."))?;
            let integrity = checkpoint
                .sources
                .get(id)
                .ok_or_else(|| io::Error::other("This note isn’t in the selected item."))?;
            let bytes = read_regular(&entry.path.join(format!("{id}.md")))?;
            if digest(&bytes) != integrity.sha256 {
                return Err(io::Error::other(
                    "This snapshot was changed by another app, so it wasn’t restored.",
                ));
            }
            let text = String::from_utf8(bytes).map_err(not_text)?;
            let mut images = BTreeMap::new();
            for (name, hash) in &integrity.assets {
                if !managed_filename(name) {
                    return Err(io::Error::other("This snapshot is damaged. Choose another one."));
                }
                let path = root.join("recovery/assets").join(name);
                safe_recovery_path(root, &path)?;
                let bytes = read_regular(&path)?;
                if digest(&bytes) != *hash || format!("{hash}.png") != *name {
                    return Err(io::Error::other(
                        "This snapshot was changed by another app, so it wasn’t restored.",
                    ));
                }
                images.insert(format!("attachments/{name}"), bytes);
            }
            Ok(RecoveredNote {
                record,
                text,
                images,
            })
        } else {
            safe_recovery_path(root, &entry.path)?;
            let text = String::from_utf8(read_regular(&entry.path)?).map_err(not_text)?;
            let mut record = entry.note.clone().unwrap_or_else(|| {
                NoteRecord::new(uuid::Uuid::new_v4().to_string(), Frame::default())
            });
            if entry.note.is_none() {
                record.title = source_title(text.as_bytes());
            }
            let parent = entry
                .path
                .parent()
                .ok_or_else(|| io::Error::other("This version’s folder is missing."))?;
            let directory = if parent.file_name().is_some_and(|name| name == "history") {
                parent.parent().unwrap_or(parent)
            } else {
                parent
            };
            let images = managed_assets(directory)?;
            Ok(RecoveredNote {
                record,
                text,
                images,
            })
        }
    }
    #[cfg(test)]
    pub fn restore_copies(&mut self, sources: Vec<RecoveredNote>) -> io::Result<Vec<NoteRecord>> {
        let pending = self.request_restore_copies(sources)?;
        self.complete(futures::executor::block_on(pending.wait()))
    }
}
pub(super) fn modified_at(path: &Path) -> u64 {
    fs::symlink_metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .unwrap_or_default()
        .as_millis() as u64
}
pub(super) fn metadata_entry(path: PathBuf, title: &str) -> RecoveryEntry {
    RecoveryEntry {
        modified_at: modified_at(&path),
        path,
        title: title.to_owned(),
        kind: RecoveryKind::Metadata,
        note: None,
    }
}
pub(super) fn source_entry(path: PathBuf, kind: &str, note: Option<NoteRecord>) -> RecoveryEntry {
    let title = format!(
        "{kind}: {}",
        note.as_ref()
            .map(|note| note.title.as_str())
            .unwrap_or("Untitled")
    );
    RecoveryEntry {
        modified_at: modified_at(&path),
        path,
        title,
        kind: RecoveryKind::Source,
        note,
    }
}
