use markraft_commonmark::{SourceDocument, commonmark_schema};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub struct Imported {
    pub text: String,
    pub title: String,
    pub linked_files: bool,
    pub images: std::collections::BTreeMap<String, Vec<u8>>,
    pub disclosures: Vec<String>,
}

pub fn prepare(paths: Vec<PathBuf>) -> io::Result<Vec<Imported>> {
    let schema = commonmark_schema();
    let mut prepared = Vec::new();
    let mut files = Vec::new();
    let mut skipped = 0usize;
    for path in paths {
        if path
            .symlink_metadata()
            .map_err(|error| {
                io::Error::other(format!(
                    "{}: {}",
                    crate::copy::file(&path),
                    crate::copy::plain(&error)
                ))
            })?
            .file_type()
            .is_symlink()
        {
            return Err(io::Error::other(format!(
                "{} is an alias. Choose the original file.",
                crate::copy::file(&path)
            )));
        }
        if path.is_dir() {
            safe_directory(&path)?;
            let mut children = fs::read_dir(&path)?
                .map(|entry| entry.map(|e| e.path()))
                .collect::<io::Result<Vec<_>>>()?;
            children.sort();
            let before = files.len();
            for child in children {
                let is_note = child.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                    ["md", "markdown", "txt"].contains(&s.to_ascii_lowercase().as_str())
                }) && child.file_name().is_some_and(|s| s != "Export Report.txt");
                if !is_note {
                    continue;
                }
                match child.symlink_metadata() {
                    Ok(metadata) if metadata.file_type().is_file() => files.push(child),
                    // An alias or folder named like a note must not fail the whole batch.
                    _ => skipped += 1,
                }
            }
            if files.len() == before {
                return Err(io::Error::other(format!(
                    "{} has no Markdown or plain-text files.",
                    crate::copy::file(&path)
                )));
            }
        } else {
            files.push(path);
        }
    }
    if files.len() > 256 {
        return Err(io::Error::other("Choose 256 files or fewer."));
    }
    let mut total_bytes = 0usize;
    for path in files {
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !["md", "markdown", "txt"].contains(&extension.as_str()) {
            return Err(io::Error::other("Choose a Markdown or plain-text file."));
        }
        if !path
            .symlink_metadata()
            .map_err(|error| {
                io::Error::other(format!(
                    "{}: {}",
                    crate::copy::file(&path),
                    crate::copy::plain(&error)
                ))
            })?
            .file_type()
            .is_file()
        {
            return Err(io::Error::other(format!(
                "{} can’t be imported. Choose a Markdown or plain-text file.",
                crate::copy::file(&path)
            )));
        }
        safe_directory(
            path.parent()
                .ok_or_else(|| io::Error::other("Choose a folder."))?,
        )?;
        let bytes = read_bounded(&path, 10 * 1024 * 1024).map_err(|error| {
            io::Error::other(format!(
                "{}: {}",
                crate::copy::file(&path),
                crate::copy::plain(&error)
            ))
        })?;
        if bytes.len() > 10 * 1024 * 1024 {
            return Err(io::Error::other(format!(
                "{} is larger than 10 MB. Choose a smaller file.",
                crate::copy::file(&path)
            )));
        }
        let (text, disclosure) = decode_source(&bytes).map_err(|error| {
            io::Error::other(format!(
                "{}: {}",
                crate::copy::file(&path),
                crate::copy::plain(&error)
            ))
        })?;
        let base = path
            .canonicalize()?
            .parent()
            .ok_or_else(|| io::Error::other("Choose a folder."))?
            .to_owned();
        let (text, images) = crate::attachments::managed_source(&text, &base)?;
        total_bytes += text.len() + images.values().map(Vec::len).sum::<usize>();
        if total_bytes > 256 * 1024 * 1024 {
            return Err(io::Error::other(
                "These files are too large to import together. Import fewer at a time.",
            ));
        }
        let document = SourceDocument::parse(&schema, &text).map_err(|error| {
            log::warn!("Cannot parse an import: {error}");
            io::Error::other(format!(
                "{} couldn’t be read as Markdown.",
                crate::copy::file(&path)
            ))
        })?;
        let title = crate::title_of(&schema, document.document());
        let linked_files = source_has_linked_files(&text)?;
        prepared.push(Imported {
            text,
            title,
            linked_files,
            images,
            disclosures: disclosure.into_iter().collect(),
        });
    }
    if skipped > 0
        && let Some(first) = prepared.first_mut()
    {
        first.disclosures.push(if skipped == 1 {
            "Skipped 1 alias or folder that is named like a note.".to_owned()
        } else {
            format!("Skipped {skipped} aliases or folders that are named like notes.")
        });
    }
    Ok(prepared)
}

fn decode_source(bytes: &[u8]) -> io::Result<(String, Option<String>)> {
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        // Byte-order marks are removed silently, as every Mac editor does.
        return String::from_utf8(bytes.to_vec())
            .map(|text| (text, None))
            .map_err(|_| io::Error::other("This file isn’t text Dahlia can read."));
    }
    let utf16 = bytes
        .strip_prefix(&[0xff, 0xfe])
        .map(|bytes| (bytes, true))
        .or_else(|| {
            bytes
                .strip_prefix(&[0xfe, 0xff])
                .map(|bytes| (bytes, false))
        });
    if let Some((bytes, little)) = utf16 {
        if bytes.len() % 2 != 0 {
            return Err(io::Error::other(
                "This file is damaged and can’t be imported.",
            ));
        }
        let units = bytes
            .chunks_exact(2)
            .map(|pair| {
                if little {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect::<Vec<_>>();
        return String::from_utf16(&units)
            .map(|text| (text, None))
            .map_err(|_| io::Error::other("This file is damaged and can’t be imported."));
    }
    String::from_utf8(bytes.to_vec())
        .map(|text| (text, None))
        .map_err(|_| io::Error::other("This file isn’t text Dahlia can read."))
}

pub fn safe_directory(path: &Path) -> io::Result<()> {
    if !path.is_dir() {
        return Err(io::Error::other(format!(
            "{} isn’t a folder.",
            crate::copy::file(path)
        )));
    }
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        if ancestor.symlink_metadata()?.file_type().is_symlink() {
            // macOS exposes its system temporary locations through these fixed aliases.
            #[cfg(target_os = "macos")]
            if ancestor == Path::new("/var") || ancestor == Path::new("/tmp") {
                continue;
            }
            return Err(io::Error::other(format!(
                "{} is inside an alias. Choose the original folder.",
                crate::copy::file(path)
            )));
        }
    }
    Ok(())
}

pub fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    // The flag's value differs by OS and architecture (e.g. Linux x86-64 vs AArch64).
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(format!(
            "{} can’t be opened. Choose a file, not a folder or alias.",
            crate::copy::file(path)
        )));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn new_destination(path: &Path) -> io::Result<()> {
    safe_directory(
        path.parent()
            .ok_or_else(|| io::Error::other("Choose an export folder."))?,
    )?;
    match path.symlink_metadata() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists. Choose a different name.",
                crate::copy::file(path)
            ),
        )),
    }
}

/// Whether a link destination names a file on a disk rather than a web page or an anchor.
/// Images are not asked about here: copying them in is `attachments`' job, and one that
/// cannot be copied already fails the transfer.
fn links_to_a_file(url: &str) -> bool {
    if url.is_empty() || url.starts_with('#') || url.starts_with("//") {
        return false;
    }
    let bytes = url.as_bytes();
    let drive_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    url.starts_with("file:") || drive_path || !url.contains(':') // Other URI schemes are external.
}
/// Whether a note links to files that a transfer does not carry along.
pub fn source_has_linked_files(text: &str) -> io::Result<bool> {
    let document = SourceDocument::parse(&commonmark_schema(), text).map_err(|error| {
        log::warn!("Cannot parse a note: {error}");
        io::Error::other("A note couldn’t be read as Markdown.")
    })?;
    fn linked(node: &markraft_core::Node) -> bool {
        node.marks().iter().any(|mark| {
            mark.attrs
                .get("href")
                .and_then(|v| v.as_str())
                .is_some_and(links_to_a_file)
        }) || node.children().any(linked)
    }
    Ok(linked(document.document()))
}

/// Publish one new source file without overwriting an existing destination.
pub fn export_new(path: &Path, text: &str) -> io::Result<()> {
    export_bytes_new(path, text.as_bytes())
}
pub fn export_bytes_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    export_bytes_new_with(path, bytes, |staged, path| fs::hard_link(staged, path))
}
/// `export_bytes_new` publishing the staged file with `link`, which must refuse to replace
/// an existing destination.
fn export_bytes_new_with(
    path: &Path,
    bytes: &[u8],
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    new_destination(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Choose an export folder."))?;
    let staged = parent.join(format!(".dahlia-export-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        // A same-directory hard link publishes atomically and refuses destination collisions.
        match link(&staged, path) {
            Err(error) if lacks_exclusive_publish(&error) => {
                // exFAT, FAT32 and many network volumes have no hard links. `create_new`
                // still refuses an existing destination; only atomicity is lost.
                log::warn!("Exporting without a hard link: {error}");
                write_destination_new(path, bytes)?;
            }
            published => published?,
        }
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(&staged); // Only this operation's unique temporary file.
    result
}
/// Whether a failed hard link or exclusive rename means the volume cannot do it at all,
/// rather than that the destination is taken.
fn lacks_exclusive_publish(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
    ) || cfg!(unix)
        && matches!(
            error.raw_os_error(),
            Some(libc::EPERM | libc::ENOTSUP | libc::EOPNOTSUPP | libc::ENOSYS | libc::EINVAL)
        )
}
/// Write `bytes` to a destination that must not exist yet. A half-written file is removed.
fn write_destination_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        let _ = fs::remove_file(path); // Created just now by `create_new`, so ours alone.
    }
    written
}

pub fn filename(title: &str) -> String {
    let stem: String = title
        .chars()
        .take(60)
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!(
        "{}.md",
        if stem.trim().is_empty() {
            "Untitled"
        } else {
            stem.trim()
        }
    )
}

pub struct ExportSource {
    pub title: String,
    pub text: String,
    pub base: PathBuf,
}
pub struct ExportReport {
    pub files: Vec<PathBuf>,
    pub issues: Vec<String>,
}

/// Rescue exports preserve each captured source even when one of its resources cannot be copied.
pub fn export_bundle(destination: &Path, sources: &[ExportSource]) -> io::Result<ExportReport> {
    new_destination(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("Choose an export folder."))?;
    let staging = parent.join(format!(".dahlia-bundle-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    let result = (|| {
        let mut used = std::collections::BTreeSet::from(["export report.txt".to_owned()]);
        let mut files = Vec::new();
        let mut issues = Vec::new();
        for source in sources {
            let base_name = filename(&source.title);
            let mut name = base_name.clone();
            let mut counter = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{}-{counter}.md", base_name.trim_end_matches(".md"));
                counter += 1;
            }
            let text = match crate::attachments::portable_source(&source.text, &source.base) {
                Ok((text, images)) => {
                    crate::attachments::publish(&staging, &images)?;
                    if source_has_linked_files(&text)? {
                        issues.push(format!("{name}: links to files that weren’t copied."));
                    }
                    text
                }
                Err(error) => {
                    issues.push(format!(
                        "{name}: images weren’t copied ({}).",
                        crate::copy::plain(&error)
                    ));
                    source.text.clone()
                }
            };
            export_new(&staging.join(&name), &text)?;
            files.push(destination.join(name));
        }
        // A report only when something was left out; a clean export needs none.
        if !issues.is_empty() {
            let report = format!("Some files weren’t included:\n{}\n", issues.join("\n"));
            export_new(&staging.join("Export Report.txt"), &report)?;
        }
        crate::storage::sync_directory(&staging)?;
        rename_directory_new(&staging, destination)?;
        crate::storage::sync_directory(parent)?;
        Ok(ExportReport { files, issues })
    })();
    // The operation owns only its unique staging directory, never an existing destination.
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// Build a new folder at `destination` in a staging folder beside it, then publish it with
/// one rename. If `build` fails part way, nothing is left at `destination` or beside it.
pub fn publish_directory(
    destination: &Path,
    build: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    new_destination(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("Choose an export folder."))?;
    let staging = parent.join(format!(".dahlia-export-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    let result = (|| {
        let built = staging.join("contents");
        build(&built)?;
        crate::storage::sync_directory(&built)?;
        rename_directory_new(&built, destination)?;
        crate::storage::sync_directory(parent)
    })();
    // The operation owns only its unique staging directory, never an existing destination.
    let _ = fs::remove_dir_all(&staging);
    result
}

fn rename_directory_new(source: &Path, destination: &Path) -> io::Result<()> {
    rename_directory_new_with(source, destination, rename_directory_exclusive)
}
/// `rename_directory_new` renaming with `exclusive`, which must refuse an existing destination.
fn rename_directory_new_with(
    source: &Path,
    destination: &Path,
    exclusive: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    match exclusive(source, destination) {
        Err(error) if lacks_exclusive_publish(&error) => {
            // The volume cannot refuse an existing destination while renaming, so check
            // first. Another writer could still slip in between the two.
            log::warn!("Exporting without an exclusive rename: {error}");
            new_destination(destination)?;
            fs::rename(source, destination)
        }
        published => published,
    }
}
fn rename_directory_exclusive(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let source =
            std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(io::Error::other)?;
        let destination =
            std::ffi::CString::new(destination.as_os_str().as_bytes()).map_err(io::Error::other)?;
        // SAFETY: matches `int renamex_np(const char *, const char *, unsigned int)` in
        // macOS's <stdio.h>.
        #[cfg(target_os = "macos")]
        unsafe extern "C" {
            fn renamex_np(
                source: *const std::ffi::c_char,
                destination: *const std::ffi::c_char,
                flags: u32,
            ) -> i32;
        }
        // SAFETY: matches glibc's `int renameat2(int, const char *, int, const char *,
        // unsigned int)` from <stdio.h> (glibc 2.28 and later).
        #[cfg(target_os = "linux")]
        unsafe extern "C" {
            fn renameat2(
                source_fd: i32,
                source: *const std::ffi::c_char,
                destination_fd: i32,
                destination: *const std::ffi::c_char,
                flags: u32,
            ) -> i32;
        }
        #[cfg(target_os = "macos")]
        // SAFETY: both CStrings are live NUL-terminated paths; RENAME_EXCL forbids overwrite.
        let result =
            unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
        #[cfg(target_os = "linux")]
        // SAFETY: live CStrings and AT_FDCWD resolve ordinary paths; NOREPLACE forbids overwrite.
        let result = unsafe {
            renameat2(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        fs::rename(source, destination)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (source, destination);
        Err(io::Error::other("Dahlia can’t export on this system."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_import_skips_non_regular_children_with_a_disclosure() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("note.md"), "# kept\n").unwrap();
        fs::create_dir(root.path().join("folder.md")).unwrap();
        let prepared = prepare(vec![root.path().to_path_buf()]).unwrap();
        assert_eq!(prepared.len(), 1);
        assert_eq!(
            prepared[0].disclosures,
            ["Skipped 1 alias or folder that is named like a note."]
        );
        // An alias is skipped too, and the note it points to is still imported on its own.
        #[cfg(unix)]
        {
            let target = root.path().join("target.txt");
            fs::write(&target, "# target\n").unwrap();
            std::os::unix::fs::symlink(&target, root.path().join("alias.md")).unwrap();
            let prepared = prepare(vec![root.path().to_path_buf()]).unwrap();
            assert_eq!(prepared.len(), 2);
            assert_eq!(
                prepared[0].disclosures,
                ["Skipped 2 aliases or folders that are named like notes."]
            );
        }
        // A folder holding nothing importable is still refused.
        let empty = tempfile::tempdir().unwrap();
        fs::create_dir(empty.path().join("folder.md")).unwrap();
        assert!(prepare(vec![empty.path().to_path_buf()]).is_err());
    }
    #[test]
    fn a_failed_directory_export_leaves_nothing_behind() {
        let fixture = tempfile::tempdir().unwrap();
        let destination = fixture.path().join("Export");
        let result = publish_directory(&destination, |staged| {
            fs::create_dir(staged)?;
            fs::write(staged.join("first.md"), "written")?;
            Err(io::Error::other("disk full"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read_dir(fixture.path()).unwrap().count(), 0);
        publish_directory(&destination, |staged| {
            fs::create_dir(staged)?;
            fs::write(staged.join("first.md"), "written")
        })
        .unwrap();
        assert_eq!(fs::read_to_string(destination.join("first.md")).unwrap(), "written");
        assert_eq!(fs::read_dir(fixture.path()).unwrap().count(), 1);
        // An existing folder is refused and left as it was.
        assert!(publish_directory(&destination, |staged| fs::create_dir(staged)).is_err());
        assert_eq!(fs::read_to_string(destination.join("first.md")).unwrap(), "written");
        assert_eq!(fs::read_dir(fixture.path()).unwrap().count(), 1);
    }
    #[test]
    fn linked_files_are_local_paths_and_not_web_or_anchor_links() {
        let linked = |link: &str| source_has_linked_files(&format!("[x]({link})\n")).unwrap();
        for local in [
            "other.md",
            "../notes/other.md",
            "/Users/me/other.md",
            "file:///Users/me/other.md",
            "C:/Users/me/other.md",
            "c:\\Users\\me\\other.md",
        ] {
            assert!(linked(local), "{local}");
        }
        for external in [
            "https://example.com",
            "mailto:me@example.com",
            "//example.com/a.md",
            "#heading",
            "C:other",
        ] {
            assert!(!linked(external), "{external}");
        }
        // Images are the attachments' business: this asks about links only.
        assert!(!source_has_linked_files("![x](file:///Users/me/a.png)\n").unwrap());
    }
    /// What `link()` and `renamex_np` return on volumes that lack them.
    fn unsupported() -> [io::Error; 3] {
        [
            io::Error::from(io::ErrorKind::Unsupported),
            io::Error::from(io::ErrorKind::PermissionDenied),
            #[cfg(unix)]
            io::Error::from_raw_os_error(libc::ENOTSUP),
            #[cfg(not(unix))]
            io::Error::from(io::ErrorKind::Unsupported),
        ]
    }
    #[test]
    fn a_file_export_falls_back_when_the_volume_has_no_hard_links() {
        let fixture = tempfile::tempdir().unwrap();
        for (index, error) in unsupported().into_iter().enumerate() {
            let path = fixture.path().join(format!("note-{index}.md"));
            export_bytes_new_with(&path, b"exported", |_, _| Err(error)).unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"exported");
        }
        assert_eq!(fs::read_dir(fixture.path()).unwrap().count(), 3, "no staged file is left");
        // The fallback still never replaces an existing destination.
        let taken = fixture.path().join("note-0.md");
        let raced = || Err(io::Error::from(io::ErrorKind::Unsupported));
        let error = export_bytes_new_with(&taken, b"other", |_, _| raced()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&taken).unwrap(), b"exported");
        // A destination that appears after the first check is refused by `create_new`.
        let late = fixture.path().join("late.md");
        let error = export_bytes_new_with(&late, b"other", |_, path| {
            fs::write(path, "theirs")?;
            raced()
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&late).unwrap(), b"theirs");
        // A taken destination reported by the link itself is not mistaken for a missing feature.
        let busy = fixture.path().join("busy.md");
        let error = export_bytes_new_with(&busy, b"x", |_, _| {
            Err(io::Error::from(io::ErrorKind::AlreadyExists))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(!busy.exists());
    }
    #[test]
    fn a_folder_export_falls_back_when_the_volume_has_no_exclusive_rename() {
        let fixture = tempfile::tempdir().unwrap();
        for (index, error) in unsupported().into_iter().enumerate() {
            let source = fixture.path().join(format!("built-{index}"));
            let destination = fixture.path().join(format!("Export {index}"));
            fs::create_dir(&source).unwrap();
            fs::write(source.join("first.md"), "written").unwrap();
            rename_directory_new_with(&source, &destination, |_, _| Err(error)).unwrap();
            assert_eq!(fs::read_to_string(destination.join("first.md")).unwrap(), "written");
            assert!(!source.exists());
        }
        // An existing destination is refused and left as it was.
        let source = fixture.path().join("again");
        fs::create_dir(&source).unwrap();
        let destination = fixture.path().join("Export 0");
        let error = rename_directory_new_with(&source, &destination, |_, _| {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(source.exists());
        assert_eq!(fs::read_to_string(destination.join("first.md")).unwrap(), "written");
    }
    #[cfg(unix)]
    #[test]
    fn bounded_reads_never_follow_a_final_symlink() {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("target.png"), b"outside").unwrap();
        std::os::unix::fs::symlink(root.join("target.png"), root.join("link.png")).unwrap();
        assert!(read_bounded(&root.join("link.png"), 1024).is_err());
        assert_eq!(
            read_bounded(&root.join("target.png"), 1024).unwrap(),
            b"outside"
        );
    }
    #[test]
    fn source_transfer_is_exact_and_never_overwrites_or_changes_inputs() {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        fs::create_dir_all(&root).unwrap();
        let text = "---\r\ntitle: preserve\r\n---\r\n\r\n* [ ] Task\r\n\r\n![remote](https://example.com/a.png)\r\n\r\n<!-- keep -->\r\n";
        let input = root.join("input.md");
        fs::write(&input, text).unwrap();
        let prepared = prepare(vec![input.clone()]).unwrap();
        assert_eq!(prepared[0].text, text);
        assert!(!prepared[0].linked_files);
        let output = root.join("output.md");
        export_new(&output, text).unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), text);
        assert!(export_new(&output, "replacement").is_err());
        assert_eq!(fs::read_to_string(&output).unwrap(), text);
        fs::write(root.join("invalid.md"), [0xff]).unwrap();
        assert!(prepare(vec![input.clone(), root.join("invalid.md")]).is_err());
        assert_eq!(fs::read_to_string(&input).unwrap(), text);
        assert!(!source_has_linked_files("[web](https://example.com)\n").unwrap());
        assert!(source_has_linked_files("[file](other.md)\n").unwrap());
        fs::write(root.join("missing.md"), "![local](images/missing.png)\n").unwrap();
        let error = prepare(vec![root.join("missing.md")]).err().unwrap();
        assert!(error.to_string().contains("missing.png"));
        assert_eq!(
            fs::read_to_string(root.join("missing.md")).unwrap(),
            "![local](images/missing.png)\n"
        );
    }

    #[test]
    fn encodings_and_bundle_folder_roundtrip_preserve_source() {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        fs::create_dir_all(&root).unwrap();
        let text = "# Résumé 🍃\r\n\r\n<!-- exact -->\r\n";
        for little in [true, false] {
            let mut bytes = if little {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for unit in text.encode_utf16() {
                bytes.extend(if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            assert_eq!(decode_source(&bytes).unwrap().0, text);
        }
        assert!(decode_source(&[0xff, 0xfe, 0x00]).is_err());
        assert!(decode_source(&[0xff, 0xfe, 0x00, 0xd8]).is_err());
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend(text.as_bytes());
        assert_eq!(decode_source(&bom).unwrap().0, text);
        let sources = vec![
            ExportSource {
                title: "Same".into(),
                text: text.into(),
                base: root.clone(),
            },
            ExportSource {
                title: "same".into(),
                text: "second\n".into(),
                base: root.clone(),
            },
        ];
        let destination = root.join("bundle");
        let report = export_bundle(&destination, &sources).unwrap();
        assert_eq!(report.files.len(), 2);
        assert!(report.issues.is_empty());
        assert_eq!(fs::read_to_string(&report.files[0]).unwrap(), text);
        let imported = prepare(vec![destination.clone()]).unwrap();
        assert_eq!(imported.len(), 2);
        assert_eq!(imported[0].text, text);
        assert!(export_bundle(&destination, &sources).is_err());
        assert_eq!(fs::read_to_string(&report.files[0]).unwrap(), text);
    }

    #[test]
    fn rescue_reports_missing_images_and_retains_captured_source() {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        fs::create_dir_all(&root).unwrap();
        let text = "---\r\nx: y\r\n---\r\n\r\n![missing](missing.png)\r\n";
        let report = export_bundle(
            &root.join("rescue"),
            &[ExportSource {
                title: "Rescue".into(),
                text: text.into(),
                base: root.clone(),
            }],
        )
        .unwrap();
        assert_eq!(report.issues.len(), 1);
        assert!(report.issues[0].contains("missing.png"));
        assert_eq!(fs::read_to_string(&report.files[0]).unwrap(), text);
        assert!(
            fs::read_to_string(root.join("rescue/Export Report.txt"))
                .unwrap()
                .contains("missing.png")
        );
    }
}
