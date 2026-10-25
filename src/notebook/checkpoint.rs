//! Immutable recovery checkpoints, written by the metadata thread.
use super::*;

impl Worker {
    /// Sources are copied once; immutable, content-addressed assets share a recovery pool.
    /// Unreadable notes and externally changed images are skipped and flagged, never fatal.
    pub(super) fn checkpoint(&mut self, index: &Index) -> io::Result<PathBuf> {
        let recovery = self.root.join("recovery");
        let snapshots = recovery.join("snapshots");
        let assets = recovery.join("assets");
        for folder in [&recovery, &snapshots, &assets] {
            reject_symlink(folder)?;
            fs::create_dir_all(folder)?;
        }
        let created_at = timestamp();
        let identity = format!("{created_at}-{}", uuid::Uuid::new_v4());
        let staging = snapshots.join(format!(".pending-{identity}"));
        fs::create_dir(&staging)?;
        let published = snapshots.join(identity);
        let staged = self.stage_checkpoint(index, &staging, &assets, created_at);
        let skipped = match staged.and_then(|skipped| {
            fs::rename(&staging, &published)?;
            Ok(skipped)
        }) {
            Ok(skipped) => skipped,
            Err(error) => {
                // A failed stage is ours, not an interrupted one; never leave it to pile up.
                let _ = fs::remove_dir_all(&staging);
                return Err(error);
            }
        };
        sync_directory(&snapshots)?;
        self.checkpoint_at = Some(Instant::now());
        if skipped.notes > 0 || skipped.images > 0 {
            self.report(skipped.describe());
        }
        // Keep twenty published checkpoints. Interrupted stages and the asset pool are retained.
        let mut history = fs::read_dir(&snapshots)?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && !entry.file_name().to_string_lossy().starts_with('.')
            })
            .map(|entry| entry.path())
            .filter(|path| *path != published)
            .collect::<Vec<_>>();
        history.sort();
        let count = history.len().saturating_sub(19);
        for path in history.into_iter().take(count) {
            safe_recovery_path(&self.root, &path)?;
            fs::remove_dir_all(path)?;
        }
        Ok(published)
    }
    pub(super) fn stage_checkpoint(
        &mut self,
        index: &Index,
        staging: &Path,
        assets: &Path,
        created_at: u64,
    ) -> io::Result<Skipped> {
        let mut skipped = Skipped::default();
        let mut checkpoint = Checkpoint {
            version: 1,
            created_at,
            index: Index {
                notes: Vec::new(),
                ..index.clone()
            },
            sources: BTreeMap::new(),
            skipped: Vec::new(),
        };
        let mut pooled = false;
        for note in &index.notes {
            let staged = match note_directory(&self.root, &note.id) {
                Ok(directory) => {
                    let target = staging.join(format!("{}.md", note.id));
                    self.stage_source(&directory, &target, &note.id)?
                        .map(|sha256| (directory, sha256))
                }
                Err(_) => None,
            };
            let Some((directory, sha256)) = staged else {
                skipped.notes += 1;
                checkpoint.skipped.push(note.id.clone());
                continue;
            };
            let mut integrity = SourceIntegrity {
                sha256,
                assets: BTreeMap::new(),
            };
            for name in managed_names(&directory) {
                match self.pool_image(&directory.join("attachments").join(&name), assets, &name) {
                    Ok(Pooled::Kept { written }) => {
                        pooled |= written;
                        let hash = name.trim_end_matches(".png").to_owned();
                        integrity.assets.insert(name, hash);
                    }
                    Ok(Pooled::Skipped) => skipped.images += 1,
                    Err(error) => return Err(error),
                }
            }
            checkpoint.index.notes.push(note.clone());
            checkpoint.sources.insert(note.id.clone(), integrity);
        }
        // One full sync makes every staged file durable: copies were handed to the disk as
        // they were written, and clones share blocks that are already durable.
        write_new(
            &staging.join("snapshot.json"),
            &serde_json::to_vec_pretty(&checkpoint).map_err(io::Error::other)?,
        )?;
        if pooled {
            sync_directory(assets)?;
        }
        Ok(skipped)
    }
    /// Stage the note's source at `target` and return its digest, or `None` when the source
    /// cannot be read. A source this session already read is cloned, not read and copied.
    pub(super) fn stage_source(
        &mut self,
        directory: &Path,
        target: &Path,
        id: &str,
    ) -> io::Result<Option<String>> {
        let Ok(file) = open_regular(&directory.join("note.md")) else {
            return Ok(None);
        };
        let key = FileKey::of(&file);
        if let Some((_, sha256)) = self
            .verified
            .sources
            .get(id)
            .filter(|(known, _)| Some(*known) == key)
            && clone_new(&file, target).is_ok()
        {
            return Ok(Some(sha256.clone()));
        }
        let Ok(bytes) = read_open(file) else {
            return Ok(None);
        };
        write_new_unflushed(target, &bytes)?;
        let sha256 = digest(&bytes);
        if let Some(key) = key {
            self.verified
                .sources
                .insert(id.to_owned(), (key, sha256.clone()));
        }
        Ok(Some(sha256))
    }
    /// Keep the managed image at `live` in the content-addressed pool. An image whose bytes
    /// no longer match its name, or whose pool copy changed outside this app, is skipped.
    pub(super) fn pool_image(&mut self, live: &Path, assets: &Path, name: &str) -> io::Result<Pooled> {
        let Ok(file) = open_regular(live) else {
            return Ok(Pooled::Skipped);
        };
        let key = FileKey::of(&file);
        let mut bytes = None;
        if key.is_none() || self.verified.images.get(live) != key.as_ref() {
            let Ok(read) = read_open(&file) else {
                return Ok(Pooled::Skipped);
            };
            if format!("{}.png", digest(&read)) != name {
                return Ok(Pooled::Skipped);
            }
            if let Some(key) = key {
                self.verified.images.insert(live.to_owned(), key);
            }
            bytes = Some(read);
        }
        let target = assets.join(name);
        match open_regular(&target) {
            Ok(pool) => {
                let pool_key = FileKey::of(&pool);
                if pool_key.is_none() || self.verified.pool.get(name) != pool_key.as_ref() {
                    if !read_open(&pool).is_ok_and(|read| format!("{}.png", digest(&read)) == name)
                    {
                        return Ok(Pooled::Skipped);
                    }
                    if let Some(pool_key) = pool_key {
                        self.verified.pool.insert(name.to_owned(), pool_key);
                    }
                }
                Ok(Pooled::Kept { written: false })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if clone_new(&file, &target).is_err() {
                    let bytes = match bytes {
                        Some(bytes) => bytes,
                        None => read_open(&file)?,
                    };
                    write_new_unflushed(&target, &bytes)?;
                }
                if let Some(pool_key) = open_regular(&target).ok().as_ref().and_then(FileKey::of) {
                    self.verified.pool.insert(name.to_owned(), pool_key);
                }
                Ok(Pooled::Kept { written: true })
            }
            Err(_) => Ok(Pooled::Skipped),
        }
    }
}
pub(super) enum Pooled {
    Kept { written: bool },
    Skipped,
}
/// What this session's checkpoints have already read and verified, by file version.
#[derive(Default)]
pub(super) struct Verified {
    /// Each note's source version and its digest.
    pub(super) sources: HashMap<String, (FileKey, String)>,
    /// Live managed images whose bytes match their names.
    pub(super) images: HashMap<PathBuf, FileKey>,
    /// Pool images whose bytes match their names.
    pub(super) pool: HashMap<String, FileKey>,
}
