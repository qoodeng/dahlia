use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// One isolated Markdown file; a lock keeps two prototypes from sharing it.
pub struct NoteStore {
    path: PathBuf,
    saved: Option<String>,
    _lock: File,
    directory_sync_pending: bool,
    checkpoint_at: Option<Instant>,
}

impl NoteStore {
    pub fn directory(&self) -> &std::path::Path {
        self.path.parent().expect("note has a parent")
    }
    pub fn open(directory: PathBuf) -> io::Result<(Self, String)> {
        reject_symlink(&directory)?;
        fs::create_dir_all(&directory)?;
        reject_symlink(&directory.join("note.lock"))?;
        reject_symlink(&directory.join("note.md"))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("note.lock"))?;
        lock.try_lock().map_err(io::Error::from)?;
        let path = directory.join("note.md");
        let saved = match fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let text = saved.clone().unwrap_or_default();
        Ok((
            Self {
                path,
                saved,
                _lock: lock,
                directory_sync_pending: false,
                checkpoint_at: None,
            },
            text,
        ))
    }

    pub fn save(&mut self, text: &str) -> io::Result<()> {
        self.save_using(text, |file, bytes| file.write_all(bytes))
    }
    fn save_using(
        &mut self,
        text: &str,
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        if self.saved.as_deref() == Some(text) {
            if self.directory_sync_pending {
                sync_directory(self.directory())?;
                self.directory_sync_pending = false;
            }
            return Ok(());
        }
        reject_symlink(&self.path)?;
        let disk = match fs::read_to_string(&self.path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if disk != self.saved {
            return Err(io::Error::other(StorageError::ExternalChange));
        }
        // Save the previous exact source at most once per five minutes, before publication.
        if self
            .checkpoint_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(300))
            && let Some(previous) = &self.saved
        {
            // History is a convenience; a damaged history folder must never block saving.
            match preserve(self.directory(), previous) {
                Ok(()) => self.checkpoint_at = Some(Instant::now()),
                Err(error) => log::warn!("Could not retain source history: {error}"),
            }
        }
        // Unique pending files preserve interrupted saves instead of truncating their sources.
        let temporary = self
            .directory()
            .join(format!("note-{}.md.pending", uuid::Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        write(&mut file, text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &self.path)?;
        // After rename the new content is live, even if directory sync fails.
        self.saved = Some(text.to_owned());
        self.directory_sync_pending = true;
        sync_directory(self.directory())?;
        self.directory_sync_pending = false;
        Ok(())
    }
}

/// Typed failures at the managed-file trust boundary. OS errors retain their original sources.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("This note was changed in another app.")]
    ExternalChange,
    #[error("Dahlia can’t save here because part of its folder is an alias.")]
    Alias,
    #[error("This item isn’t a file Dahlia can use.")]
    UnreadableFile,
    #[error("This item isn’t a file Dahlia can use.")]
    NotRegular,
}
pub fn is_external_change(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|inner| {
        matches!(
            inner.downcast_ref::<StorageError>(),
            Some(StorageError::ExternalChange)
        )
    })
}

/// Keep an exact copy of `text` in the note's bounded source history.
pub fn preserve(directory: &Path, text: &str) -> io::Result<()> {
    let history = directory.join("history");
    reject_symlink(&history)?;
    fs::create_dir_all(&history)?;
    let backup = history.join(format!("{}-{}.md", timestamp(), uuid::Uuid::new_v4()));
    write_new(&backup, text.as_bytes())?;
    // Pruning is housekeeping: an undeletable or linked old entry must not fail the capture.
    if let Err(error) = prune_history(&history, 20) {
        log::warn!("Could not trim source history: {error}");
    }
    Ok(())
}

/// Keep unsaved source outside the note file, for a quit whose save was refused.
/// The same text is kept once, as a quit can be refused by more than one hook.
pub fn rescue(root: &Path, id: &str, text: &str) -> io::Result<()> {
    let directory = root.join("recovery").join("preserved");
    reject_symlink(&root.join("recovery"))?;
    reject_symlink(&directory)?;
    fs::create_dir_all(&directory)?;
    let safe_id: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let prefix = format!("unsaved-{safe_id}-");
    let already = fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .any(|entry| read_regular(&entry.path()).is_ok_and(|kept| kept == text.as_bytes()));
    if already {
        return Ok(());
    }
    write_new(
        &directory.join(format!("{prefix}{}-{}.md", timestamp(), uuid::Uuid::new_v4())),
        text.as_bytes(),
    )
}

pub fn sync_directory(directory: &std::path::Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(directory)?.sync_all()?;
    // Windows std::fs cannot open a directory handle for FlushFileBuffers.
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

pub fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(io::Error::other(StorageError::Alias))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn read_regular(path: &Path) -> io::Result<Vec<u8>> {
    reject_symlink(path)?;
    if !fs::metadata(path)?.is_file() {
        return Err(io::Error::other(StorageError::UnreadableFile));
    }
    fs::read(path)
}

pub fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    sync_directory(path.parent().expect("managed file has a parent"))
}

fn prune_history(directory: &Path, keep: usize) -> io::Result<()> {
    let mut paths = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md") && path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    let count = paths.len().saturating_sub(keep);
    for path in paths.into_iter().take(count) {
        reject_symlink(&path)?;
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    reject_symlink(path)?;
    let pending = path.with_extension(format!("{}.pending", uuid::Uuid::new_v4()));
    if let Err(error) = write_new(&pending, bytes).and_then(|()| fs::rename(&pending, path)) {
        // A handled failure knows the partial file is useless, and keeping it would eat into
        // the space that is probably short. Only this call's unique temporary file goes.
        let _ = fs::remove_file(&pending);
        return Err(error);
    }
    sync_directory(path.parent().expect("managed file has a parent"))
}

/// A regular file opened without following a final symbolic link.
pub fn open_regular(path: &Path) -> io::Result<File> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(StorageError::NotRegular));
    }
    Ok(file)
}

/// Identifies one version of a file: saves replace note.md by rename, so any change gives
/// a new inode, and edits in place change the modification time or size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileKey {
    device: u64,
    inode: u64,
    modified: (i64, i64),
    size: u64,
}
impl FileKey {
    pub fn of(file: &File) -> Option<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata().ok()?;
            Some(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
                modified: (metadata.mtime(), metadata.mtime_nsec()),
                size: metadata.size(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = file;
            None
        }
    }
}

/// Copy the open `file` to the new path `target` as an APFS clone, which shares the source's
/// blocks and needs no data sync. Fails where cloning is unsupported; callers then copy.
pub fn clone_new(file: &File, target: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::{ffi::OsStrExt, io::AsRawFd};
        let name =
            std::ffi::CString::new(target.as_os_str().as_bytes()).map_err(io::Error::other)?;
        // CLONE_NOFOLLOW: the new path itself is never followed.
        const CLONE_NOFOLLOW: u32 = 1;
        // SAFETY: `file` is an open descriptor for this call, and `name` is a NUL-terminated
        // path that outlives it; AT_FDCWD resolves a relative path like any other call.
        let status = unsafe {
            libc::fclonefileat(
                file.as_raw_fd(),
                libc::AT_FDCWD,
                name.as_ptr(),
                CLONE_NOFOLLOW,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (file, target);
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

/// Create `path` with `bytes`, handing its data to the disk without waiting for the disk's
/// own cache; a later `full_sync` makes every such write durable at once.
pub fn write_new_unflushed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::io::AsRawFd;
        // SAFETY: the descriptor is open for the duration of the call.
        if unsafe { libc::fsync(file.as_raw_fd()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    file.sync_data()
}

enum WriteRequest {
    Save(
        Box<dyn FnOnce() -> io::Result<String> + Send>,
        futures::channel::oneshot::Sender<io::Result<()>>,
    ),
    Close(futures::channel::oneshot::Sender<io::Result<()>>),
    #[cfg(test)]
    Barrier(std::sync::mpsc::Sender<()>),
    Stop,
}

/// One serial writer per open note. Dropping an autosave waiter does not cancel its write.
pub struct NoteWriter {
    sender: std::sync::mpsc::Sender<WriteRequest>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl NoteWriter {
    pub fn new(mut store: NoteStore) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel::<WriteRequest>();
        let thread = std::thread::spawn(move || {
            while let Ok(request) = receiver.recv() {
                match request {
                    WriteRequest::Save(prepare, reply) => {
                        let _ = reply.send(prepare().and_then(|text| store.save(&text)));
                    }
                    WriteRequest::Close(reply) => {
                        drop(store);
                        let _ = reply.send(Ok(()));
                        return;
                    }
                    #[cfg(test)]
                    WriteRequest::Barrier(reply) => {
                        let _ = reply.send(());
                    }
                    WriteRequest::Stop => break,
                }
            }
        });
        Self {
            sender,
            thread: Some(thread),
        }
    }
    #[cfg(test)]
    pub fn save(&self, text: String) -> futures::channel::oneshot::Receiver<io::Result<()>> {
        self.save_with(move || Ok(text))
    }
    /// Preparation runs in the same serial queue as publication, so a slow render cannot
    /// overtake a newer save. No rendering or fsync happens in the caller.
    pub fn save_with(
        &self,
        prepare: impl FnOnce() -> io::Result<String> + Send + 'static,
    ) -> futures::channel::oneshot::Receiver<io::Result<()>> {
        let (reply, receiver) = futures::channel::oneshot::channel();
        let _ = self
            .sender
            .send(WriteRequest::Save(Box::new(prepare), reply));
        receiver
    }
    pub fn request_close(&mut self) -> futures::channel::oneshot::Receiver<io::Result<()>> {
        let (reply, receiver) = futures::channel::oneshot::channel();
        let _ = self.sender.send(WriteRequest::Close(reply));
        self.thread.take(); // The acknowledgement arrives only after its file lock is released.
        receiver
    }
    #[cfg(test)]
    pub fn flush(&self, text: String) -> io::Result<()> {
        futures::executor::block_on(self.save(text)).map_err(|_| {
            io::Error::other("Couldn’t save this note. Copy its text, then quit and reopen Dahlia.")
        })?
    }
    #[cfg(test)]
    pub fn barrier(&self) {
        let (reply, receiver) = std::sync::mpsc::channel();
        if self.sender.send(WriteRequest::Barrier(reply)).is_ok() {
            let _ = receiver.recv();
        }
    }
    #[cfg(test)]
    pub fn close(&mut self) -> io::Result<()> {
        let _ = self.sender.send(WriteRequest::Stop);
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| io::Error::other("Couldn’t finish saving this note. Try again."))?;
        }
        Ok(())
    }
}
impl Drop for NoteWriter {
    fn drop(&mut self) {
        let _ = self.sender.send(WriteRequest::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_source_render_is_nonblocking_serial_and_close_releases_lock() {
        let root = tempfile::tempdir().unwrap();
        let (store, _) = NoteStore::open(root.path().to_owned()).unwrap();
        let mut writer = NoteWriter::new(store);
        let (started, working) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let first = writer.save_with(move || {
            started.send(()).unwrap();
            gate.recv().unwrap();
            Ok("older".into())
        });
        working.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = writer.save("newer".into());
        assert!(NoteStore::open(root.path().to_owned()).is_err());
        release.send(()).unwrap();
        futures::executor::block_on(first).unwrap().unwrap();
        futures::executor::block_on(second).unwrap().unwrap();
        let closed = writer.request_close();
        futures::executor::block_on(closed).unwrap().unwrap();
        assert_eq!(NoteStore::open(root.path().to_owned()).unwrap().1, "newer");
    }
    #[test]
    fn dropped_save_waiter_does_not_cancel_render_or_durable_publication() {
        let root = tempfile::tempdir().unwrap();
        let (store, _) = NoteStore::open(root.path().to_owned()).unwrap();
        let mut writer = NoteWriter::new(store);
        drop(writer.save_with(|| Ok("retained".into())));
        futures::executor::block_on(writer.request_close())
            .unwrap()
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("note.md")).unwrap(),
            "retained"
        );
    }
    #[test]
    fn source_history_is_exact_throttled_and_bounded_and_pending_is_retained() {
        let _directory_fixture = tempfile::tempdir().unwrap();
        let directory = _directory_fixture.path().to_path_buf();
        let (mut store, _) = NoteStore::open(directory.clone()).unwrap();
        let original = "---\r\ntitle: unchanged\r\n---\r\n\r\n<!-- exact -->\r\n";
        store.save(original).unwrap();
        fs::write(directory.join("note.md.pending"), "interrupted source").unwrap();
        store.save("second source").unwrap();
        let history = directory.join("history");
        let paths = fs::read_dir(&history)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 1);
        assert_eq!(fs::read_to_string(&paths[0]).unwrap(), original);
        store.save("third source").unwrap();
        assert_eq!(fs::read_dir(&history).unwrap().count(), 1);
        for n in 0..23 {
            store.checkpoint_at = None;
            store.save(&format!("source {n}")).unwrap();
        }
        assert_eq!(fs::read_dir(&history).unwrap().count(), 20);
        assert_eq!(
            fs::read_to_string(directory.join("note.md.pending")).unwrap(),
            "interrupted source"
        );
        drop(store);
    }
    #[test]
    fn saves_exact_source_reopens_and_refuses_external_overwrite() {
        let _directory_fixture = tempfile::tempdir().unwrap();
        let directory = _directory_fixture.path().to_path_buf();
        let source = "---\ntitle: original\n---\n\n* [ ] Task\n\n<!-- untouched -->\n";
        let (mut store, text) = NoteStore::open(directory.clone()).unwrap();
        assert!(text.is_empty());
        assert!(NoteStore::open(directory.clone()).is_err());
        store.save(source).unwrap();
        assert_eq!(
            fs::read_to_string(directory.join("note.md")).unwrap(),
            source
        );
        drop(store);
        let (mut store, text) = NoteStore::open(directory.clone()).unwrap();
        assert_eq!(text, source);
        fs::write(directory.join("note.md"), "external edit").unwrap();
        assert!(is_external_change(&store.save("local edit").unwrap_err()));
        assert_eq!(
            fs::read_to_string(directory.join("note.md")).unwrap(),
            "external edit"
        );
        drop(store);
    }
}

#[cfg(test)]
mod fault_tests {
    use super::*;
    #[test]
    fn partial_disk_full_and_write_failures_never_replace_durable_source() {
        for kind in [
            io::ErrorKind::StorageFull,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::WriteZero,
        ] {
            for prefix in [0, 7, 19] {
                let fixture = tempfile::tempdir().unwrap();
                let path = fixture.path().join("note.md");
                let original = "---\r\ntitle: exact 🍃\r\n---\r\n\r\n<!-- keep -->\r\n";
                let (mut store, _) = NoteStore::open(fixture.path().to_owned()).unwrap();
                store.save(original).unwrap();
                let replacement = "A replacement that must remain unpublished";
                let error = store
                    .save_using(replacement, |file, bytes| {
                        file.write_all(&bytes[..prefix])?;
                        Err(io::Error::from(kind))
                    })
                    .unwrap_err();
                assert_eq!(error.kind(), kind);
                assert_eq!(fs::read(&path).unwrap(), original.as_bytes());
                assert_eq!(store.saved.as_deref(), Some(original));
                assert!(!store.directory_sync_pending);
                let pending = fs::read_dir(fixture.path())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "pending"))
                    .collect::<Vec<_>>();
                assert_eq!(pending.len(), 1, "interrupted work remains recoverable");
                assert_eq!(
                    fs::read(&pending[0]).unwrap(),
                    replacement.as_bytes()[..prefix]
                );
                store.save(replacement).unwrap();
                drop(store);
                assert_eq!(
                    NoteStore::open(fixture.path().to_owned()).unwrap().1,
                    replacement
                );
                assert_eq!(
                    fs::read(&pending[0]).unwrap(),
                    replacement.as_bytes()[..prefix]
                );
            }
        }
    }
    #[test]
    fn missing_published_source_is_a_conflict_and_retains_the_loaded_baseline() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("note.md");
        let (mut store, _) = NoteStore::open(fixture.path().to_owned()).unwrap();
        let original = "Exact loaded source\r\n";
        store.save(original).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(is_external_change(&store.save("local edit").unwrap_err()));
        assert!(
            !path.exists(),
            "an externally deleted note is never silently recreated"
        );
        assert_eq!(store.saved.as_deref(), Some(original));
        fs::write(&path, original).unwrap();
        store.save("local edit").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "local edit");
    }
    #[test]
    fn arbitrary_external_bytes_are_never_overwritten_or_adopted_as_the_baseline() {
        for original in [
            "",
            "exact\r\n",
            "résumé 🍃\n",
            "---\nkey: value\n---\n",
            "a\0b",
        ] {
            for external in [b"external".as_slice(), b"", b"\xff\xfe", b"changed\r\n"] {
                if original.as_bytes() == external {
                    continue;
                }
                let fixture = tempfile::tempdir().unwrap();
                let path = fixture.path().join("note.md");
                let (mut store, _) = NoteStore::open(fixture.path().to_owned()).unwrap();
                store.save(original).unwrap();
                fs::write(&path, external).unwrap();
                let error = store.save("my replacement").unwrap_err();
                assert!(is_external_change(&error) || error.kind() == io::ErrorKind::InvalidData);
                assert_eq!(fs::read(&path).unwrap(), external);
                assert_eq!(store.saved.as_deref(), Some(original));
                fs::write(&path, original).unwrap();
                store.save("my replacement").unwrap();
                assert_eq!(fs::read_to_string(&path).unwrap(), "my replacement");
            }
        }
    }
}

/// Seeded property tests: random sequences of saves, injected write faults, external edits,
/// deletions and reopenings, checked against a model of what must be on disk. Seeds are
/// fixed so failures reproduce; `DAHLIA_STORAGE_SEEDS` raises the count for longer runs.
#[cfg(test)]
mod property_tests {
    use super::*;

    /// xorshift64*: deterministic and dependency-free.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn text(&mut self) -> String {
            const PIECES: [&str; 12] = [
                "", "a", "Title\n", "\r\n", "🍃", "résumé", "---\nk: v\n---\n", "\0", "* [ ] t\n",
                "<!-- x -->", " ", "\u{feff}",
            ];
            (0..self.below(8))
                .map(|_| PIECES[self.below(PIECES.len())])
                .collect()
        }
    }

    fn seeds() -> u64 {
        std::env::var("DAHLIA_STORAGE_SEEDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(48)
    }
    fn pending(directory: &Path) -> Vec<PathBuf> {
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "pending"))
            .collect()
    }
    fn history(directory: &Path) -> Vec<String> {
        match fs::read_dir(directory.join("history")) {
            Ok(entries) => entries
                .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    #[test]
    fn random_save_fault_and_external_change_sequences_keep_the_model() {
        for seed in 1..=seeds() {
            let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
            let fixture = tempfile::tempdir().unwrap();
            let directory = fixture.path().to_path_buf();
            let path = directory.join("note.md");
            let (mut store, _) = NoteStore::open(directory.clone()).unwrap();
            // What note.md must hold (None: absent), and what the store believes it holds.
            let mut disk: Option<Vec<u8>> = None;
            let mut baseline: Option<String> = None;
            // Every source the store ever held as its baseline (saved or loaded); history may
            // only hold exact copies of these.
            let mut published: Vec<String> = Vec::new();
            let mut attempts: Vec<Vec<u8>> = Vec::new();
            for step in 0..40 {
                let context = format!("seed {seed} step {step}");
                match rng.below(10) {
                    // An ordinary save succeeds exactly when disk still matches the baseline.
                    0..=3 => {
                        let text = rng.text();
                        let unchanged = disk.as_deref() == baseline.as_deref().map(str::as_bytes);
                        if baseline.as_deref() == Some(text.as_str()) {
                            // Nothing new to save: never touches disk, even if it changed.
                            store.save(&text).unwrap();
                        } else {
                            match store.save(&text) {
                            Ok(()) => {
                                assert!(unchanged, "{context}: saved over an external change");
                                disk = Some(text.clone().into_bytes());
                                baseline = Some(text.clone());
                                published.push(text);
                            }
                            Err(error) => {
                                assert!(!unchanged, "{context}: unexpected {error}");
                                assert!(
                                    is_external_change(&error)
                                        || error.kind() == io::ErrorKind::InvalidData,
                                    "{context}: {error}"
                                );
                            }
                        }
                        }
                    }
                    // A write that fails part-way never publishes and leaves its bytes pending.
                    4 | 5 => {
                        let text = rng.text();
                        let prefix = rng.below(text.len() + 1);
                        let kind = [
                            io::ErrorKind::StorageFull,
                            io::ErrorKind::WriteZero,
                            io::ErrorKind::Interrupted,
                        ][rng.below(3)];
                        let result = store.save_using(&text, |file, bytes| {
                            file.write_all(&bytes[..prefix])?;
                            Err(io::Error::from(kind))
                        });
                        if baseline.as_deref() != Some(text.as_str()) {
                            assert!(result.is_err(), "{context}: fault was swallowed");
                            if disk.as_deref() == baseline.as_deref().map(str::as_bytes) {
                                assert_eq!(result.unwrap_err().kind(), kind, "{context}");
                                attempts.push(text.as_bytes()[..prefix].to_vec());
                            }
                        }
                    }
                    // Another app edits the file.
                    6 => {
                        let bytes = rng.text().into_bytes();
                        fs::write(&path, &bytes).unwrap();
                        disk = Some(bytes);
                    }
                    // Another app deletes it.
                    7 => {
                        let _ = fs::remove_file(&path);
                        disk = None;
                    }
                    // The other app puts back exactly what Dahlia last saved.
                    8 => {
                        if let Some(text) = &baseline {
                            fs::write(&path, text).unwrap();
                            disk = Some(text.clone().into_bytes());
                        }
                    }
                    // Quit and relaunch: the store reloads whatever is on disk.
                    _ => {
                        drop(store);
                        match NoteStore::open(directory.clone()) {
                            Ok((reopened, text)) => {
                                let expected = disk
                                    .as_deref()
                                    .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
                                    .unwrap_or_default();
                                assert_eq!(text, expected, "{context}: reload");
                                baseline = disk.is_some().then_some(text);
                                published.extend(baseline.clone());
                                store = reopened;
                            }
                            Err(error) => {
                                assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{context}");
                                // Non-UTF-8 external bytes: restore valid text to continue.
                                fs::write(&path, "").unwrap();
                                disk = Some(Vec::new());
                                baseline = Some(String::new());
                                store = NoteStore::open(directory.clone()).unwrap().0;
                            }
                        }
                    }
                }
                assert_eq!(fs::read(&path).ok(), disk, "{context}: note.md");
                let kept = history(&directory);
                assert!(kept.len() <= 20, "{context}: history unbounded");
                for source in kept {
                    assert!(published.contains(&source), "{context}: invented history");
                }
                let mut leftovers: Vec<Vec<u8>> = pending(&directory)
                    .iter()
                    .map(|path| fs::read(path).unwrap())
                    .collect();
                leftovers.sort();
                let mut expected = attempts.clone();
                expected.sort();
                assert_eq!(leftovers, expected, "{context}: interrupted saves");
            }
        }
    }

    #[test]
    fn a_failed_atomic_write_removes_its_temporary_file() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("index.json");
        atomic_write(&path, b"whole").unwrap();
        // A folder in the way makes the final rename fail after the bytes were written.
        let blocked = fixture.path().join("blocked.json");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("inside"), "kept").unwrap();
        assert!(atomic_write(&blocked, b"never published").is_err());
        assert!(pending(fixture.path()).is_empty(), "no partial file is left behind");
        assert_eq!(fs::read(blocked.join("inside")).unwrap(), b"kept");
        assert_eq!(fs::read(&path).unwrap(), b"whole");
    }
    #[test]
    fn atomic_write_publishes_whole_contents_or_nothing() {
        let mut rng = Rng(0x5eed);
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("index.json");
        let mut current: Option<Vec<u8>> = None;
        for _ in 0..200 {
            let bytes: Vec<u8> = (0..rng.below(4096)).map(|_| rng.next() as u8).collect();
            atomic_write(&path, &bytes).unwrap();
            current = Some(bytes);
            assert_eq!(fs::read(&path).ok(), current);
            assert!(pending(fixture.path()).is_empty(), "no pending file survives success");
        }
        // A directory that refuses new files: the old contents stay whole.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(fixture.path()).unwrap().permissions();
            permissions.set_mode(0o500);
            fs::set_permissions(fixture.path(), permissions.clone()).unwrap();
            // Superusers ignore the mode, so only assert where it is enforced.
            let enforced = File::create(fixture.path().join("probe")).is_err();
            let result = atomic_write(&path, b"must not appear");
            permissions.set_mode(0o700);
            fs::set_permissions(fixture.path(), permissions).unwrap();
            if enforced {
                assert!(result.is_err());
                assert_eq!(fs::read(&path).ok(), current);
            }
        }
    }

    #[test]
    fn history_keeps_the_newest_twenty_exact_sources() {
        let fixture = tempfile::tempdir().unwrap();
        let mut rng = Rng(0xabcdef);
        let mut written = Vec::new();
        for n in 0..45 {
            let text = format!("{n:03} {}", rng.text());
            preserve(fixture.path(), &text).unwrap();
            written.push(text);
            // Same-millisecond names still sort by time; wait out the clock.
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut kept = history(fixture.path());
        kept.sort();
        let mut newest = written[written.len() - 20..].to_vec();
        newest.sort();
        assert_eq!(kept, newest);
    }

    #[test]
    fn rescue_preserves_source_outside_the_note() {
        let root = tempfile::tempdir().unwrap();
        rescue(root.path(), "../note", "unsaved **text**").unwrap();
        // A second quit hook refusing the same text keeps one copy.
        rescue(root.path(), "../note", "unsaved **text**").unwrap();
        rescue(root.path(), "../note", "typed since").unwrap();
        let preserved = root.path().join("recovery/preserved");
        let mut texts: Vec<_> = fs::read_dir(&preserved)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                assert!(name.starts_with("unsaved-___note-") && !name.contains(".."));
                fs::read_to_string(path).unwrap()
            })
            .collect();
        texts.sort();
        assert_eq!(texts, ["typed since", "unsaved **text**"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_damaged_history_folder_never_blocks_saving() {
        let fixture = tempfile::tempdir().unwrap();
        let directory = fixture.path().join("note");
        let (mut store, _) = NoteStore::open(directory.clone()).unwrap();
        store.save("one").unwrap();
        // History that is an alias is refused, but the save still lands.
        let outside = fixture.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, directory.join("history")).unwrap();
        store.save("two").unwrap();
        assert_eq!(fs::read_to_string(directory.join("note.md")).unwrap(), "two");
        assert!(preserve(&directory, "kept").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        // Retention is retried, so history resumes once the folder is sound again.
        fs::remove_file(directory.join("history")).unwrap();
        store.save("three").unwrap();
        assert_eq!(history(&directory), vec!["two".to_owned()]);
        // An old entry that cannot be pruned does not fail the capture either.
        let folder = directory.join("history");
        let target = fixture.path().join("target.md");
        fs::write(&target, "alias").unwrap();
        std::os::unix::fs::symlink(&target, folder.join("0-alias.md")).unwrap();
        for n in 0..21 {
            preserve(&directory, &format!("v{n}")).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(history(&directory).len(), 23);
    }

    #[test]
    fn symlinked_note_paths_are_refused_without_touching_the_target() {
        #[cfg(unix)]
        {
            let fixture = tempfile::tempdir().unwrap();
            let outside = fixture.path().join("outside.md");
            fs::write(&outside, "outside").unwrap();
            let directory = fixture.path().join("note");
            fs::create_dir(&directory).unwrap();
            std::os::unix::fs::symlink(&outside, directory.join("note.md")).unwrap();
            assert!(NoteStore::open(directory.clone()).is_err());
            assert!(atomic_write(&directory.join("note.md"), b"x").is_err());
            assert_eq!(fs::read_to_string(&outside).unwrap(), "outside");
        }
    }
}
