use super::*;
fn isolated(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("dahlia-{label}-"))
        .tempdir()
        .unwrap()
}
#[test]
fn structural_request_never_waits_and_publishes_only_after_acknowledgement() {
    use futures::FutureExt;
    let root = tempfile::tempdir().unwrap();
    let mut book = Notebook::open(root.path().to_owned()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let mut candidate = book.index.clone();
    candidate.notes[0].state = NoteState::Archived;
    candidate.notes[0].open = false;
    let (started, working) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let pending = book
        .request_commit(
            candidate,
            move || {
                started.send(()).unwrap();
                gate.recv().unwrap();
                Ok(())
            },
            (),
        )
        .unwrap();
    working.recv_timeout(Duration::from_secs(2)).unwrap();
    // The worker is deliberately blocked, but its caller already returned.
    assert_eq!(book.note(&note.id).unwrap().state, NoteState::Active);
    assert_eq!(
        book.request_create(Frame::default()).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    let mut waiting = Box::pin(pending.wait());
    assert!((&mut waiting).now_or_never().is_none());
    let mut changed = note.clone();
    changed.title = "Typed while saving another field".into();
    changed.style.color = ColorIndex::ALL[4];
    book.update(changed).unwrap();
    release.send(()).unwrap();
    let completed = futures::executor::block_on(waiting);
    book.complete(completed).unwrap();
    assert_eq!(book.note(&note.id).unwrap().state, NoteState::Archived);
    assert_eq!(
        book.note(&note.id).unwrap().title,
        "Typed while saving another field"
    );
    assert_eq!(book.note(&note.id).unwrap().style.color, ColorIndex::ALL[4]);
    book.flush().unwrap();
    let disk = parse_index(&fs::read(root.path().join("index.json")).unwrap()).unwrap();
    assert_eq!(disk.notes[0], book.note(&note.id).unwrap());
}
#[test]
fn failed_structural_prepare_keeps_original_index_and_all_sources() {
    let root = tempfile::tempdir().unwrap();
    let mut book = Notebook::open(root.path().to_owned()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let before = fs::read(root.path().join("index.json")).unwrap();
    let candidate = Index {
        notes: Vec::new(),
        ..book.index.clone()
    };
    let pending = book
        .request_commit(candidate, || Err(io::Error::other("disk full")), ())
        .unwrap();
    let completed = futures::executor::block_on(pending.wait());
    assert!(book.complete(completed).is_err());
    assert_eq!(book.note(&note.id).unwrap(), note);
    assert_eq!(fs::read(root.path().join("index.json")).unwrap(), before);
    assert!(
        root.path()
            .join("notes")
            .join(note.id)
            .join("note.md")
            .exists()
    );
    let pending = book.request_flush().unwrap();
    let completed = futures::executor::block_on(pending.wait());
    book.complete(completed).unwrap();
}
#[test]
fn failed_sync_after_publication_rolls_back_without_claiming_a_durable_close() {
    let root = tempfile::tempdir().unwrap();
    let mut book = Notebook::open(root.path().to_owned()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let pending = book
        .request_transition(note.clone(), NoteState::Active, NoteState::Trash)
        .unwrap();
    let mut completed = futures::executor::block_on(pending.wait());
    assert!(completed.outcome.1.is_ok());
    // Inject the post-rename failure, when disk has changed but success is not durable.
    completed.outcome = (true, Err(io::Error::other("directory sync failed")));
    assert!(book.complete(completed).is_err());
    assert_eq!(book.note(&note.id).unwrap().state, NoteState::Active);
    book.flush().unwrap();
    let disk = parse_index(&fs::read(root.path().join("index.json")).unwrap()).unwrap();
    assert_eq!(disk.notes[0].state, NoteState::Active);
}
#[test]
fn async_create_restore_import_and_defaults_acknowledge_before_success() {
    let root = tempfile::tempdir().unwrap();
    let mut book = Notebook::open(root.path().to_owned()).unwrap();
    let pending = book.request_create(Frame::default()).unwrap();
    assert!(book.notes().is_empty());
    let note = book
        .complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    let pending = book
        .request_transition(note.clone(), NoteState::Active, NoteState::Archived)
        .unwrap();
    book.complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    let pending = book.request_restore(&note.id, NoteState::Archived).unwrap();
    book.complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    let mut defaults = note.clone();
    defaults.style.color = ColorIndex::ALL[5];
    let pending = book.request_defaults(&defaults).unwrap();
    assert_ne!(book.defaults().style.color, ColorIndex::ALL[5]);
    book.complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    assert_eq!(book.defaults().style.color, ColorIndex::ALL[5]);
    let pending = book
        .request_import(
            vec![crate::transfer::Imported {
                title: "Imported".into(),
                text: "source".into(),
                images: BTreeMap::new(),
                linked_files: false,
                disclosures: Vec::new(),
            }],
            Frame::default(),
        )
        .unwrap();
    assert_eq!(book.notes().len(), 1);
    let notes = book
        .complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(book.notes().len(), 2);
    assert_eq!(book.saved_source(&notes[0].id).unwrap().0, "source");
}
#[test]
fn imported_notes_cascade_from_the_first_frame() {
    let _root_fixture = isolated("import-cascade");
    let mut book = Notebook::open(_root_fixture.path().to_path_buf()).unwrap();
    let imported = |title: &str| crate::transfer::Imported {
        title: title.into(),
        text: "source".into(),
        images: BTreeMap::new(),
        linked_files: false,
        disclosures: Vec::new(),
    };
    let start = Frame {
        x: 100.,
        y: 120.,
        ..Frame::default()
    };
    let pending = book
        .request_import(vec![imported("A"), imported("B"), imported("C")], start)
        .unwrap();
    let notes = book
        .complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    let corners: Vec<_> = notes.iter().map(|n| (n.frame.x, n.frame.y)).collect();
    assert_eq!(corners, [(100., 120.), (128., 148.), (156., 176.)]);
    book.finish_for_test();
}
#[test]
fn batch_metadata_updates_are_atomic_and_checkpoint_history_is_bounded() {
    let _root_fixture = isolated("batch-metadata");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let mut a = book.create(Frame::default()).unwrap();
    let mut b = book.create(Frame::default()).unwrap();
    a.title = "A".into();
    b.title = "B".into();
    book.update_many(vec![a.clone(), b.clone()]).unwrap();
    assert_eq!(book.note(&a.id).unwrap().title, "A");
    assert_eq!(book.note(&b.id).unwrap().title, "B");
    let before = fs::read(root.join("index.json")).unwrap();
    a.title = "failed change".into();
    assert!(book.update_many(vec![a.clone(), a.clone()]).is_err());
    b.id = "unknown".into();
    assert!(book.update_many(vec![a, b]).is_err());
    assert_eq!(fs::read(root.join("index.json")).unwrap(), before);
    for _ in 0..22 {
        book.checkpoint().unwrap();
    }
    let published = fs::read_dir(root.join("recovery/snapshots"))
        .unwrap()
        .filter(|entry| {
            !entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        })
        .count();
    assert_eq!(published, 20);
    book.finish_for_test();
}
fn note_folders(root: &Path) -> usize {
    fs::read_dir(root.join("notes"))
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0)
}
fn imported(text: &str, images: BTreeMap<String, Vec<u8>>) -> crate::transfer::Imported {
    crate::transfer::Imported {
        text: text.into(),
        title: text.trim().into(),
        linked_files: false,
        images,
        disclosures: Vec::new(),
    }
}
#[test]
fn failed_structural_changes_leave_no_orphan_note_folders() {
    let _root_fixture = isolated("no-orphans");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let kept = book.create(Frame::default()).unwrap();
    let before = fs::read(root.join("index.json")).unwrap();
    assert_eq!(note_folders(&root), 1);
    // A source whose image is damaged fails after an earlier one in the batch was written.
    let damaged = BTreeMap::from([("attachments/not-its-hash.png".to_owned(), vec![1, 2, 3])]);
    let batch = || {
        vec![
            imported("first\n", BTreeMap::new()),
            imported("second\n", BTreeMap::new()),
            imported("third\n", damaged.clone()),
        ]
    };
    assert!(book.import_sources(batch(), Frame::default()).is_err());
    assert_eq!(note_folders(&root), 1, "a failed import must leave no folders");
    let recovered = |text: &str, images| RecoveredNote {
        record: NoteRecord::new(uuid::Uuid::new_v4().to_string(), Frame::default()),
        text: text.into(),
        images,
    };
    assert!(
        book.restore_copies(vec![
            recovered("one\n", BTreeMap::new()),
            recovered("two\n", damaged.clone()),
        ])
        .is_err()
    );
    assert_eq!(note_folders(&root), 1, "a failed restore must leave no folders");
    assert_eq!(book.notes().len(), 1);
    assert_eq!(fs::read(root.join("index.json")).unwrap(), before);
    // The index cannot be read, so the write fails after the folders were prepared.
    let held = root.join("index.held");
    fs::rename(root.join("index.json"), &held).unwrap();
    fs::create_dir(root.join("index.json")).unwrap();
    assert!(book.create(Frame::default()).is_err());
    assert!(
        book.import_sources(vec![imported("fine\n", BTreeMap::new())], Frame::default())
            .is_err()
    );
    let (original, text, copy_folder) = (
        book.note(&kept.id).unwrap(),
        "copy\n".to_owned(),
        root.join("notes").join(&kept.id),
    );
    let pending = book.request_copy(&original, text, copy_folder).unwrap();
    assert!(book.complete(futures::executor::block_on(pending.wait())).is_err());
    assert_eq!(note_folders(&root), 1, "an unwritable index must leave no folders");
    fs::remove_dir(root.join("index.json")).unwrap();
    fs::rename(&held, root.join("index.json")).unwrap();
    // Changes work again afterwards, and the surviving note is untouched.
    book.import_sources(vec![imported("fine\n", BTreeMap::new())], Frame::default())
        .unwrap();
    assert_eq!(note_folders(&root), 2);
    assert_eq!(book.open_store(&kept.id).unwrap().1, "");
    book.finish_for_test();
}
#[test]
fn a_failed_forget_leaves_no_marker_hiding_the_note() {
    let _root_fixture = isolated("failed-forget");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let marker = root.join("notes").join(&note.id).join(FORGOTTEN);
    // The index cannot be written, so Forget fails after it marked the folder.
    let held = root.join("index.held");
    fs::rename(root.join("index.json"), &held).unwrap();
    fs::create_dir(root.join("index.json")).unwrap();
    assert!(book.forget(&note.id).is_err());
    assert!(!marker.exists(), "a failed forget must not hide the note");
    assert!(book.note(&note.id).is_some());
    fs::remove_dir(root.join("index.json")).unwrap();
    fs::rename(&held, root.join("index.json")).unwrap();
    // Once the index writes again, Forget marks the folder and drops the note.
    book.forget(&note.id).unwrap();
    assert!(marker.exists());
    assert!(book.note(&note.id).is_none());
    book.finish_for_test();
    // A successful forget survives a relaunch.
    let kept = Notebook::open(root.clone()).unwrap();
    assert!(kept.note(&note.id).is_none(), "a forgotten note stays forgotten");
    kept.finish_for_test();
}
#[test]
fn metadata_writes_leave_the_ui_thread_defer_typing_pauses_and_back_up_once() {
    let _root_fixture = isolated("metadata-worker");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    book.finish_for_test();
    let mut book = Notebook::open(root.clone()).unwrap();
    let session_start = fs::read(root.join("index.json")).unwrap();
    // A typing pause changes memory now and disk only when the notebook is idle or flushed.
    let mut typed = note.clone();
    typed.modified_at += 1;
    typed.title = "Typed".into();
    book.update(typed.clone()).unwrap();
    assert_eq!(book.note(&note.id).unwrap().title, "Typed");
    assert_eq!(fs::read(root.join("index.json")).unwrap(), session_start);
    book.flush().unwrap();
    let flushed = parse_index(&fs::read(root.join("index.json")).unwrap()).unwrap();
    assert_eq!(flushed.notes[0], typed);
    // The backup is the index from before this session's first write, kept for the session.
    let backup = fs::read(root.join("index.backup.json")).unwrap();
    assert_eq!(backup, session_start);
    let mut moved = typed.clone();
    moved.frame.x += 40.;
    book.update(moved.clone()).unwrap();
    book.flush().unwrap();
    assert_eq!(fs::read(root.join("index.backup.json")).unwrap(), backup);
    // An outside edit is kept in Recovery and reported once; it never locks metadata out.
    fs::write(root.join("index.json"), "edited elsewhere").unwrap();
    moved.frame.y += 40.;
    book.update(moved.clone()).unwrap();
    book.checkpoint().unwrap();
    assert!(
        book.take_problem().is_some_and(
            |problem| problem.contains("Another app changed Dahlia’s list of notes")
        ),
        "an outside change is reported"
    );
    book.flush().unwrap();
    let kept = fs::read_dir(root.join("recovery/preserved"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| fs::read(entry.path()).is_ok_and(|bytes| bytes == b"edited elsewhere"));
    assert!(kept, "the replaced index must be preserved");
    let disk = parse_index(&fs::read(root.join("index.json")).unwrap()).unwrap();
    assert_eq!(disk.notes[0], moved);
    assert!(book.take_problem().is_none(), "the change is reported once");
    book.finish_for_test();
}
#[cfg(unix)]
#[test]
fn an_unwritable_backup_is_reported_once_and_never_blocks_index_writes() {
    let _root_fixture = isolated("backup-blocked");
    let root = _root_fixture.path().to_path_buf();
    let _outside_fixture = isolated("backup-outside");
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    book.finish_for_test();
    // The backup is an alias, which Dahlia refuses to write through.
    let outside = _outside_fixture.path().join("elsewhere.json");
    fs::write(&outside, "not ours").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("index.backup.json")).unwrap();
    let mut book = Notebook::open(root.clone()).unwrap();
    let mut moved = note.clone();
    moved.frame.x += 40.;
    book.update(moved.clone()).unwrap();
    book.flush().unwrap();
    let disk = parse_index(&fs::read(root.join("index.json")).unwrap()).unwrap();
    assert_eq!(disk.notes[0], moved, "the index is written despite the backup");
    assert!(
        book.take_problem()
            .is_some_and(|problem| problem.contains("back up")),
        "the backup failure is reported"
    );
    assert_eq!(fs::read(&outside).unwrap(), b"not ours");
    // Later writes neither fail nor repeat the report.
    moved.frame.y += 40.;
    book.update(moved.clone()).unwrap();
    book.flush().unwrap();
    let disk = parse_index(&fs::read(root.join("index.json")).unwrap()).unwrap();
    assert_eq!(disk.notes[0], moved);
    assert!(book.take_problem().is_none(), "reported once");
    book.finish_for_test();
}
#[test]
fn repeated_checkpoints_reuse_unchanged_files_and_still_catch_every_change() {
    let _root_fixture = isolated("checkpoint-reuse");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let a = book.create(Frame::default()).unwrap();
    let b = book.create(Frame::default()).unwrap();
    let image = b"pooled image".to_vec();
    let relative = crate::attachments::reference(&image);
    let directory_b = root.join("notes").join(&b.id);
    crate::attachments::publish(&directory_b, &BTreeMap::from([(relative.clone(), image)]))
        .unwrap();
    let (mut store_a, _) = book.open_store(&a.id).unwrap();
    store_a.save("A one\n").unwrap();
    fs::write(directory_b.join("note.md"), format!("![b]({relative})\n")).unwrap();
    let source = |book: &Notebook, snapshot: &Path, id: &str| {
        let entry = book
            .recovery_entries()
            .unwrap()
            .into_iter()
            .find(|entry| entry.path == snapshot)
            .unwrap();
        Notebook::read_recovery_note(&root, &entry, id)
            .unwrap()
            .text
    };
    let first = book.checkpoint().unwrap();
    // A saved source is replaced by rename; an outside edit may rewrite it in place.
    store_a.save("A two\n").unwrap();
    let second = book.checkpoint().unwrap();
    assert_eq!(source(&book, &second, &a.id), "A two\n");
    assert_eq!(source(&book, &second, &b.id), format!("![b]({relative})\n"));
    fs::write(directory_b.join("note.md"), "B edited in place\n").unwrap();
    let third = book.checkpoint().unwrap();
    assert_eq!(source(&book, &third, &b.id), "B edited in place\n");
    // Earlier checkpoints are independent copies, not links to the live files.
    assert_eq!(source(&book, &first, &a.id), "A one\n");
    assert_eq!(source(&book, &second, &b.id), format!("![b]({relative})\n"));
    assert!(book.take_problem().is_none());
    // A pool image changed outside this app is skipped and flagged, not trusted.
    let pooled = root
        .join("recovery/assets")
        .join(relative.trim_start_matches("attachments/"));
    fs::write(&pooled, "tampered").unwrap();
    book.checkpoint().unwrap();
    assert!(
        book.take_problem()
            .is_some_and(|problem| problem.contains("1 image"))
    );
    drop(store_a);
    book.finish_for_test();
}
#[test]
fn corrupt_index_preserves_bytes_and_recovers_backup_and_orphan_sources() {
    let _root_fixture = isolated("corrupt-index");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let mut note = book.create(Frame::default()).unwrap();
    let (mut store, _) = book.open_store(&note.id).unwrap();
    store.save("# exact\r\n\r\n<!-- preserve -->\r\n").unwrap();
    drop(store);
    note.title = "Exact".into();
    book.update(note.clone()).unwrap();
    book.finish_for_test();
    let orphan = uuid::Uuid::new_v4().to_string();
    fs::create_dir_all(root.join("notes").join(&orphan)).unwrap();
    fs::write(
        root.join("notes").join(&orphan).join("note.md"),
        "orphan source\n",
    )
    .unwrap();
    let corrupt = b"{broken index\xff";
    fs::write(root.join("index.json"), corrupt).unwrap();
    let book = Notebook::open(root.clone()).unwrap();
    assert!(book.recovery_notice().is_some());
    assert!(book.note(&note.id).is_some());
    assert_eq!(book.note(&orphan).unwrap().state, NoteState::Archived);
    assert_eq!(fs::read(root.join("index.json")).unwrap(), corrupt);
    let preserved = fs::read_dir(root.join("recovery/preserved"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(preserved).unwrap(), corrupt);
    assert_eq!(
        book.open_store(&note.id).unwrap().1,
        "# exact\r\n\r\n<!-- preserve -->\r\n"
    );
    book.finish_for_test();
}
/// Every file under `root` with its bytes, except the lock, which opening always creates.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap().map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() {
                files.insert(path.strip_prefix(root).unwrap().to_owned(), Vec::new());
                walk(root, &path, files);
            } else if path.file_name().is_some_and(|name| name != "library.lock") {
                files.insert(path.strip_prefix(root).unwrap().to_owned(), fs::read(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}
#[test]
fn an_index_from_a_newer_version_is_refused_and_nothing_is_written() {
    let _root_fixture = isolated("newer-index");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let mut archived = book.create(Frame::default()).unwrap();
    archived.state = NoteState::Archived;
    archived.open = false;
    book.update(archived).unwrap();
    book.create(Frame::default()).unwrap();
    book.finish_for_test();
    let saved = String::from_utf8(fs::read(root.join("index.json")).unwrap()).unwrap();
    assert!(saved.contains("\"version\": 1"));
    let readable = saved.replacen("\"version\": 1", "\"version\": 2", 1).into_bytes();
    // A newer build may also have changed the list's shape, which this build cannot parse.
    let reshaped = br#"{"version": 2, "notes": "a different shape", "future": true}"#.to_vec();
    for newer in [readable, reshaped] {
        fs::write(root.join("index.json"), &newer).unwrap();
        fs::write(root.join("index.backup.json"), &newer).unwrap();
        let before = tree(&root);
        let error = Notebook::open(root.clone()).err().unwrap();
        assert!(is_newer_version(&error));
        assert!(crate::copy::plain(&error).contains("Update Dahlia"));
        assert_eq!(tree(&root), before);
        assert_eq!(fs::read(root.join("index.json")).unwrap(), newer);
        assert_eq!(fs::read(root.join("index.backup.json")).unwrap(), newer);
        assert!(!root.join("recovery/preserved").exists());
    }
}
#[test]
fn an_index_with_no_usable_version_is_still_rebuilt_from_the_files() {
    let _root_fixture = isolated("versionless-index");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    book.finish_for_test();
    for damaged in [&br#"{"version": 0, "notes": []}"#[..], br#"{"notes": []}"#] {
        fs::write(root.join("index.json"), damaged).unwrap();
        let book = Notebook::open(root.clone()).unwrap();
        assert!(book.recovery_notice().is_some());
        assert!(book.note(&note.id).is_some());
        book.finish_for_test();
    }
}
#[test]
fn preserved_index_copies_are_deduplicated_capped_and_never_block_opening() {
    let _root_fixture = isolated("preserved-index");
    let root = _root_fixture.path().to_path_buf();
    let preserved = root.join("recovery/preserved");
    let count = || {
        fs::read_dir(&preserved)
            .unwrap()
            .filter_map(Result::ok)
            .count()
    };
    preserve_index_bytes(&root, b"same").unwrap();
    preserve_index_bytes(&root, b"same").unwrap();
    assert_eq!(count(), 1);
    for n in 0..30 {
        std::thread::sleep(Duration::from_millis(2));
        preserve_index_bytes(&root, format!("distinct {n}").as_bytes()).unwrap();
    }
    assert_eq!(count(), 20);
    // The newest copy survives the trim.
    let newest = fs::read_dir(&preserved)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| fs::read(entry.path()).is_ok_and(|bytes| bytes == b"distinct 29"));
    assert!(newest);
    // A recovery folder that cannot hold copies does not stop an unreadable index opening.
    fs::remove_dir_all(&preserved).unwrap();
    fs::write(&preserved, "not a folder").unwrap();
    fs::write(root.join("index.json"), "{broken").unwrap();
    let book = Notebook::open(root.clone()).unwrap();
    assert!(book.recovery_notice().is_some());
    book.finish_for_test();
}
#[test]
fn unindexed_sources_are_recovered_even_with_a_valid_index() {
    let _root_fixture = isolated("orphan-scan");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let kept = book.create(Frame::default()).unwrap();
    book.finish_for_test();
    let orphan = uuid::Uuid::new_v4().to_string();
    let stored = root.join("notes").join(&orphan).join("note.md");
    fs::create_dir_all(stored.parent().unwrap()).unwrap();
    fs::write(&stored, "orphan **source**\n").unwrap();
    let forgotten = root.join("notes").join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&forgotten).unwrap();
    fs::write(forgotten.join("note.md"), "forgotten source\n").unwrap();
    fs::write(forgotten.join(".forgotten"), "").unwrap();
    let mut book = Notebook::open(root.clone()).unwrap();
    assert!(book.recovery_notice().is_none());
    let record = book.note(&orphan).expect("orphan is indexed");
    assert_eq!(record.state, NoteState::Archived);
    assert!(!record.open);
    assert_eq!(book.note(&kept.id).unwrap(), kept);
    assert_eq!(book.notes().len(), 2, "a forgotten folder is not listed");
    // Forgetting keeps the file but stops it being indexed again.
    book.forget(&orphan).unwrap();
    assert!(stored.exists());
    assert!(book.note(&orphan).is_none());
    assert!(book.forget(&orphan).is_err());
    book.finish_for_test();
    let book = Notebook::open(root.clone()).unwrap();
    assert!(book.note(&orphan).is_none());
    assert!(book.note(&kept.id).is_some());
    // It is still offered in Recovery, where a copy can be restored.
    assert!(
        book.recovery_entries()
            .unwrap()
            .iter()
            .any(|entry| entry.path == stored.canonicalize().unwrap())
    );
    book.finish_for_test();
}
#[test]
fn invalid_identity_recovers_without_following_untrusted_paths() {
    let _root_fixture = isolated("invalid-index");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let mut invalid = book.index.clone();
    invalid.notes[0].id = "../../outside".into();
    book.finish_for_test();
    fs::write(
        root.join("index.json"),
        serde_json::to_vec(&invalid).unwrap(),
    )
    .unwrap();
    let book = Notebook::open(root.clone()).unwrap();
    assert!(book.recovery_notice().is_some());
    assert!(book.note("../../outside").is_none());
    assert!(book.note(&note.id).is_some());
    book.finish_for_test();
}
#[test]
fn immutable_snapshot_restores_exact_copies_and_retains_assets() {
    let _root_fixture = isolated("restore-snapshot");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let directory = root.join("notes").join(&note.id);
    let bytes = b"immutable managed image".to_vec();
    let relative = crate::attachments::reference(&bytes);
    crate::attachments::publish(
        &directory,
        &BTreeMap::from([(relative.clone(), bytes.clone())]),
    )
    .unwrap();
    let source = format!(
        "---\r\ntitle: exact\r\n---\r\n\r\n![kept]({relative})\r\n\r\n<!-- exact -->\r\n"
    );
    let (mut store, _) = book.open_store(&note.id).unwrap();
    store.save(&source).unwrap();
    let snapshot = book.checkpoint().unwrap();
    store.save("current note\n").unwrap();
    drop(store);
    let entry = book
        .recovery_entries()
        .unwrap()
        .into_iter()
        .find(|entry| entry.path == snapshot)
        .unwrap();
    fs::write(directory.join(&relative), "externally changed live image").unwrap();
    let recovered = Notebook::read_recovery_note(&root, &entry, &note.id).unwrap();
    assert_eq!(recovered.text, source);
    assert_eq!(recovered.images[&relative], bytes);
    // An outside metadata change must not block restoring; its bytes stay recoverable.
    let snapshot_bytes = fs::read(snapshot.join("snapshot.json")).unwrap();
    fs::write(root.join("index.json"), "external metadata edit").unwrap();
    let copies = book.restore_copies(vec![recovered]).unwrap();
    assert_ne!(copies[0].id, note.id);
    assert_eq!(book.notes().len(), 2);
    assert!(
        book.take_problem()
            .is_some_and(|problem| problem.contains("Another app changed Dahlia’s list of notes")),
        "an outside change is reported, not silent"
    );
    let kept = fs::read_dir(root.join("recovery/preserved"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| fs::read(entry.path()).is_ok_and(|bytes| bytes == b"external metadata edit"));
    assert!(kept, "the replaced index must be preserved");
    assert_eq!(
        fs::read_to_string(directory.join("note.md")).unwrap(),
        "current note\n"
    );
    assert_eq!(
        fs::read_to_string(directory.join(&relative)).unwrap(),
        "externally changed live image"
    );
    assert_eq!(book.open_store(&copies[0].id).unwrap().1, source);
    assert_eq!(
        fs::read(root.join("notes").join(&copies[0].id).join(&relative)).unwrap(),
        bytes
    );
    assert_eq!(
        fs::read(snapshot.join("snapshot.json")).unwrap(),
        snapshot_bytes
    );
    assert_eq!(
        fs::read_to_string(directory.join("note.md")).unwrap(),
        "current note\n"
    );
    let export = root.join("exported-snapshot");
    Notebook::export_snapshot(&root, &entry, &export).unwrap();
    assert_eq!(
        fs::read(export.join("snapshot.json")).unwrap(),
        snapshot_bytes
    );
    assert_eq!(
        fs::read_to_string(export.join(format!("{}.md", note.id))).unwrap(),
        source
    );
    book.finish_for_test();
}
#[test]
fn lazy_inventory_keeps_interrupted_sources_and_rejects_changed_snapshots() {
    let _root_fixture = isolated("interrupted-save");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let directory = root.join("notes").join(&note.id);
    fs::write(
        directory.join("note.md.pending"),
        "interrupted **source**\r\n",
    )
    .unwrap();
    let (mut store, _) = book.open_store(&note.id).unwrap();
    store.save("current\n").unwrap();
    drop(store);
    let snapshot = book.checkpoint().unwrap();
    let entries = book.recovery_entries().unwrap();
    let pending = entries
        .iter()
        .find(|entry| entry.path.ends_with("note.md.pending"))
        .unwrap();
    assert_eq!(
        Notebook::preview_source(&root, pending, &note.id).unwrap(),
        "interrupted **source**\r\n"
    );
    let selected = entries.iter().find(|entry| entry.path == snapshot).unwrap();
    fs::write(snapshot.join(format!("{}.md", note.id)), "tampered").unwrap();
    // Inventory and note metadata do not eagerly load a library's Markdown or images.
    assert!(!book.recovery_entries().unwrap().is_empty());
    assert_eq!(
        Notebook::recovery_records(&root, selected).unwrap().len(),
        1
    );
    assert!(Notebook::read_recovery_note(&root, selected, &note.id).is_err());
    assert_eq!(
        fs::read_to_string(directory.join("note.md.pending")).unwrap(),
        "interrupted **source**\r\n"
    );
    book.finish_for_test();
}
#[cfg(unix)]
#[test]
fn one_unreadable_note_never_empties_the_recovery_inventory() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let _root_fixture = isolated("inventory-skips");
    let root = _root_fixture.path().to_path_buf();
    let _outside_fixture = isolated("inventory-outside");
    let mut book = Notebook::open(root.clone()).unwrap();
    let healthy = book.create(Frame::default()).unwrap();
    let aliased = book.create(Frame::default()).unwrap();
    let unreadable = book.create(Frame::default()).unwrap();
    for note in [&healthy, &aliased, &unreadable] {
        let (mut store, _) = book.open_store(&note.id).unwrap();
        store.save("first\n").unwrap();
        store.save("second\n").unwrap();
    }
    let root = root.canonicalize().unwrap();
    let history = |note: &NoteRecord| root.join("notes").join(&note.id).join("history");
    let earlier = |note: &NoteRecord| {
        book.recovery_entries()
            .unwrap()
            .into_iter()
            .filter(|entry| entry.path.starts_with(history(note)))
            .count()
    };
    assert_eq!(earlier(&healthy), 1);
    // One note's history folder is an alias, another's cannot be listed.
    fs::remove_dir_all(history(&aliased)).unwrap();
    symlink(_outside_fixture.path(), history(&aliased)).unwrap();
    fs::set_permissions(history(&unreadable), fs::Permissions::from_mode(0o000)).unwrap();
    fs::write(
        root.join("notes").join(&aliased.id).join("note.md.pending"),
        "interrupted\n",
    )
    .unwrap();
    let entries = book.recovery_entries().unwrap();
    fs::set_permissions(history(&unreadable), fs::Permissions::from_mode(0o700)).unwrap();
    // The healthy note's version and the other notes' interrupted sources are still listed.
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.path.starts_with(history(&healthy)))
            .count(),
        1
    );
    assert!(entries.iter().any(|entry| entry.path.ends_with(
        Path::new(&aliased.id).join("note.md.pending")
    )));
    assert!(!entries.iter().any(|entry| entry.path.starts_with(history(&aliased))));
    book.finish_for_test();
}
#[cfg(unix)]
#[test]
fn symlinked_sources_and_recovery_assets_are_never_followed() {
    use std::os::unix::fs::symlink;
    let _root_fixture = isolated("recovery-links");
    let root = _root_fixture.path().to_path_buf();
    let _outside_fixture = isolated("outside-source");
    let outside = _outside_fixture.path().to_path_buf();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("note.md"), "outside source\n").unwrap();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let source = root.join("notes").join(&note.id).join("note.md");
    fs::remove_file(&source).unwrap();
    symlink(outside.join("note.md"), &source).unwrap();
    assert!(book.open_store(&note.id).is_err());
    // The link is skipped and flagged, never copied into recovery.
    let snapshot = book.checkpoint().unwrap();
    assert!(!snapshot.join(format!("{}.md", note.id)).exists());
    assert!(book.take_problem().is_some());
    assert_eq!(
        fs::read_to_string(outside.join("note.md")).unwrap(),
        "outside source\n"
    );
    book.finish_for_test();
}
#[test]
fn archive_trash_restore_are_durable_and_failed_transitions_keep_source() {
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    let mut notebook = Notebook::open(root.clone()).unwrap();
    let mut note = notebook.create(Frame::default()).unwrap();
    note.style.color = ColorIndex::ALL[3];
    note.collapsed = true;
    note.expanded_height = 440.;
    let source = "---\ntitle: kept\n---\n\nOriginal **source**\n\n<!-- preserved -->\n";
    let (mut store, _) = notebook.open_store(&note.id).unwrap();
    store.save(source).unwrap();
    drop(store);
    let archived = notebook
        .transition(note.clone(), NoteState::Active, NoteState::Archived)
        .unwrap();
    assert!(!archived.open);
    assert_eq!(archived.style.color, ColorIndex::ALL[3]);
    assert!(notebook.restore(&note.id, NoteState::Trash).is_err());
    let restored = notebook.restore(&note.id, NoteState::Archived).unwrap();
    assert_eq!(restored.id, note.id);
    assert_eq!(restored.expanded_height, 440.);
    notebook
        .transition(restored, NoteState::Active, NoteState::Trash)
        .unwrap();
    notebook.finish_for_test();
    let mut notebook = Notebook::open(root.clone()).unwrap();
    assert_eq!(notebook.note(&note.id).unwrap().state, NoteState::Trash);
    let restored = notebook.restore(&note.id, NoteState::Trash).unwrap();
    assert_eq!(notebook.open_store(&note.id).unwrap().1, source);
    let index = fs::read(root.join("index.json")).unwrap();
    fs::remove_file(root.join("index.json")).unwrap();
    fs::create_dir(root.join("index.json")).unwrap();
    assert!(
        notebook
            .transition(restored, NoteState::Active, NoteState::Archived)
            .is_err()
    );
    assert_eq!(notebook.note(&note.id).unwrap().state, NoteState::Active);
    assert_eq!(notebook.open_store(&note.id).unwrap().1, source);
    fs::remove_dir(root.join("index.json")).unwrap();
    fs::write(root.join("index.json"), index).unwrap();
    notebook.finish_for_test();
}
#[test]
fn appearance_defaults_and_shared_tint_preserve_legacy_notes() {
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("note.md"), "legacy **source**\n").unwrap();
    let mut notebook = Notebook::open(root.clone()).unwrap();
    assert_eq!(notebook.note("primary").unwrap().style, NoteStyle::legacy());
    let mut styled = notebook.create(Frame::default()).unwrap();
    assert!(styled.style.glass());
    styled.style.color = ColorIndex::ALL[4];
    styled.style.reading_size = 17.;
    styled.style.translucent = true;
    styled.collapsed = true;
    styled.expanded_height = 460.;
    notebook.update(styled.clone()).unwrap();
    notebook.use_as_default(&styled).unwrap();
    notebook.stage_glass_tint(0.);
    notebook.save_index().unwrap();
    notebook.finish_for_test();
    let mut notebook = Notebook::open(root.clone()).unwrap();
    assert_eq!(notebook.glass_tint(), 0.);
    assert_eq!(notebook.note(&styled.id).unwrap().style, styled.style);
    assert_eq!(notebook.defaults().height, 460.);
    assert_eq!(
        notebook.create(Frame::default()).unwrap().style,
        styled.style
    );
    assert_eq!(
        fs::read_to_string(root.join("note.md")).unwrap(),
        "legacy **source**\n"
    );
    // A pre-appearance index has no style/default/tint fields.
    let legacy = serde_json::to_value(notebook.index.clone()).unwrap();
    let mut legacy = legacy.as_object().unwrap().clone();
    legacy.remove("defaults");
    legacy.remove("glass_tint");
    for note in legacy.get_mut("notes").unwrap().as_array_mut().unwrap() {
        note.as_object_mut().unwrap().remove("style");
    }
    notebook.finish_for_test();
    fs::write(
        root.join("index.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let notebook = Notebook::open(root.clone()).unwrap();
    assert!(
        notebook
            .notes()
            .iter()
            .all(|note| note.style == NoteStyle::legacy())
    );
    notebook.finish_for_test();
}
#[test]
fn independent_notes_close_reopen_and_legacy_preservation() {
    let _root_fixture = tempfile::tempdir().unwrap();
    let root = _root_fixture.path().to_path_buf();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("note.md"), "old **source**\n").unwrap();
    let mut notebook = Notebook::open(root.clone()).unwrap();
    assert_eq!(notebook.notes().len(), 1);
    assert!(Notebook::open(root.clone()).is_err());
    let a = notebook.create(Frame::default()).unwrap();
    let b = notebook.create(Frame::default()).unwrap();
    let (mut sa, _) = notebook.open_store(&a.id).unwrap();
    let (mut sb, _) = notebook.open_store(&b.id).unwrap();
    sa.save("A\n").unwrap();
    sb.save("B\n").unwrap();
    let mut closed = a.clone();
    closed.open = false;
    closed.collapsed = true;
    closed.expanded_height = 520.;
    closed.frame.x = 99999.;
    notebook.update(closed).unwrap();
    drop(sa);
    drop(sb);
    notebook.finish_for_test();
    let notebook = Notebook::open(root.clone()).unwrap();
    assert!(!notebook.note(&a.id).unwrap().open);
    assert_eq!(notebook.open_store(&a.id).unwrap().1, "A\n");
    assert_eq!(notebook.open_store(&b.id).unwrap().1, "B\n");
    assert_eq!(
        fs::read_to_string(root.join("note.md")).unwrap(),
        "old **source**\n"
    );
    let restored = notebook.note(&a.id).unwrap().restored(&[Frame {
        x: 0.,
        y: 0.,
        width: 800.,
        height: 600.,
    }]);
    assert_eq!(restored.frame.height, STRIP_HEIGHT);
    assert_eq!(restored.expanded_height, 520.);
    assert!(restored.frame.x + restored.frame.width <= 800.);
    notebook.finish_for_test();
}
#[test]
fn restoration_keeps_notes_on_a_second_display_including_negative_x() {
    let primary = Frame {
        x: 0.,
        y: 25.,
        width: 1440.,
        height: 875.,
    };
    let left = Frame {
        x: -1920.,
        y: 20.,
        width: 1920.,
        height: 1080.,
    };
    let displays = [primary, left];
    let on_left = Frame {
        x: -1500.,
        y: 300.,
        width: 360.,
        height: 350.,
    };
    assert_eq!(on_left.restored(&displays, false), on_left);
    // Straddling both, mostly on the left display: clamped into the left display.
    let straddling = Frame {
        x: -300.,
        y: 900.,
        ..on_left
    };
    let restored = straddling.restored(&displays, false);
    assert!(restored.x >= left.x && restored.x + restored.width <= left.x + left.width);
    assert!(restored.y + restored.height <= left.y + left.height);
    // Off every display: the nearest display wins, not the last one listed.
    let far_right = Frame {
        x: 5000.,
        y: 300.,
        ..on_left
    };
    let restored = far_right.restored(&displays, false);
    assert_eq!(restored.x, primary.x + primary.width - restored.width);
    let far_left = Frame {
        x: -9000.,
        ..far_right
    };
    assert_eq!(far_left.restored(&displays, false).x, left.x);
    // Equally far from two side-by-side displays: the primary display wins.
    let primary = Frame {
        x: 0.,
        y: 0.,
        width: 1000.,
        height: 800.,
    };
    let right = Frame {
        x: 1000.,
        ..primary
    };
    let below = Frame {
        x: 820.,
        y: 5000.,
        ..on_left
    };
    let restored = below.restored(&[primary, right], false);
    assert_eq!(restored.x, primary.width - restored.width, "{restored:?}");
    let restored = below.restored(&[right, primary], false);
    assert_eq!(restored.x, right.x, "{restored:?}");
}
#[test]
fn restoration_handles_corrupt_bounds_and_removed_displays() {
    let screen = Frame {
        x: 100.,
        y: 50.,
        width: 800.,
        height: 600.,
    };
    for frame in [
        Frame {
            x: f32::NAN,
            ..Frame::default()
        },
        Frame {
            x: -99999.,
            y: 99999.,
            width: 9000.,
            height: 9000.,
        },
    ] {
        let restored = frame.restored(&[screen], false);
        assert!(restored.x >= screen.x && restored.y >= screen.y);
        assert!(restored.x + restored.width <= 900. && restored.y + restored.height <= 650.);
    }
    let large_display = Frame {
        x: 1470.,
        y: -200.,
        width: 3200.,
        height: 2400.,
    };
    let expanded = large_display.restored(&[large_display], false);
    assert_eq!(
        expanded, large_display,
        "zoomed notes must fill displays larger than 2000 points"
    );
    let collapsed = expanded.restored(&[large_display], true);
    assert_eq!(collapsed.width, expanded.width);
    assert_eq!(collapsed.x, expanded.x);
    assert_eq!(collapsed.y, expanded.y);
    assert_eq!(collapsed.height, STRIP_HEIGHT);
}
fn pending_checkpoints(root: &Path) -> usize {
    fs::read_dir(root.join("recovery/snapshots"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".pending-"))
                .count()
        })
        .unwrap_or(0)
}
#[test]
fn unreadable_trash_note_never_blocks_metadata_commits() {
    let _root_fixture = isolated("unreadable-trash");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let keep = book.create(Frame::default()).unwrap();
    let lost = book.create(Frame::default()).unwrap();
    let lost = book
        .transition(lost, NoteState::Active, NoteState::Trash)
        .unwrap();
    book.finish_for_test();
    fs::remove_file(root.join("notes").join(&lost.id).join("note.md")).unwrap();
    // A fresh session checkpoints on its first commit.
    let mut book = Notebook::open(root.clone()).unwrap();
    let created = book.create(Frame::default()).unwrap();
    let mut renamed = book.note(&keep.id).unwrap();
    renamed.title = "Still saves".into();
    book.update(renamed).unwrap();
    assert_eq!(book.note(&keep.id).unwrap().title, "Still saves");
    assert!(book.note(&created.id).is_some());
    assert_eq!(pending_checkpoints(&root), 0);
    assert!(
        book.take_problem()
            .is_some_and(|problem| problem.contains("1 note")),
        "skipped sources must be flagged"
    );
    // The checkpoint is published, loadable, and holds every readable note.
    let entry = book
        .recovery_entries()
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Snapshot")
        .unwrap();
    let records = Notebook::recovery_records(&root, &entry).unwrap();
    assert!(records.iter().any(|note| note.id == keep.id));
    assert!(!records.iter().any(|note| note.id == lost.id));
    book.finish_for_test();
}
#[test]
fn externally_changed_archived_image_never_blocks_metadata_commits() {
    let _root_fixture = isolated("changed-archived-image");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    let directory = root.join("notes").join(&note.id);
    let bytes = b"managed image".to_vec();
    let relative = crate::attachments::reference(&bytes);
    crate::attachments::publish(&directory, &BTreeMap::from([(relative.clone(), bytes)]))
        .unwrap();
    let (mut store, _) = book.open_store(&note.id).unwrap();
    store.save(&format!("![kept]({relative})\n")).unwrap();
    drop(store);
    book.transition(note.clone(), NoteState::Active, NoteState::Archived)
        .unwrap();
    book.finish_for_test();
    fs::write(directory.join(&relative), "edited elsewhere").unwrap();
    let mut book = Notebook::open(root.clone()).unwrap();
    book.create(Frame::default()).unwrap();
    assert_eq!(pending_checkpoints(&root), 0);
    assert!(book.take_problem().is_some());
    // The source itself is still captured.
    let entry = book
        .recovery_entries()
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Snapshot")
        .unwrap();
    assert_eq!(
        Notebook::preview_source(&root, &entry, &note.id).unwrap(),
        format!("![kept]({relative})\n")
    );
    book.finish_for_test();
}
#[cfg(unix)]
#[test]
fn failed_checkpoint_removes_its_staging_folder_and_commit_still_succeeds() {
    use std::os::unix::fs::PermissionsExt;
    let _root_fixture = isolated("failed-checkpoint");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    book.checkpoint().unwrap();
    book.finish_for_test();
    let bytes = b"new image".to_vec();
    let relative = crate::attachments::reference(&bytes);
    crate::attachments::publish(
        &root.join("notes").join(&note.id),
        &BTreeMap::from([(relative, bytes)]),
    )
    .unwrap();
    // The asset pool cannot accept the new image, as on a full or read-only volume.
    let assets = root.join("recovery/assets");
    fs::set_permissions(&assets, fs::Permissions::from_mode(0o555)).unwrap();
    let mut book = Notebook::open(root.clone()).unwrap();
    assert!(book.checkpoint().is_err());
    assert_eq!(pending_checkpoints(&root), 0);
    let created = book.create(Frame::default());
    fs::set_permissions(&assets, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(created.is_ok(), "{created:?}");
    assert_eq!(pending_checkpoints(&root), 0);
    assert!(book.take_problem().is_some());
    book.finish_for_test();
}
#[test]
fn zoom_restore_round_trips_and_defaults_when_absent() {
    let frame = Frame {
        x: 40.,
        y: 60.,
        width: 300.,
        height: 280.,
    };
    let mut note = NoteRecord::new("zoom".into(), Frame::default());
    assert!(note.zoom_restore.is_none());
    note.zoom_restore = Some(frame);
    let json = serde_json::to_value(&note).unwrap();
    let back: NoteRecord = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back.zoom_restore, Some(frame));
    // Records written before the toggle existed have no such field.
    let mut legacy = json;
    legacy.as_object_mut().unwrap().remove("zoom_restore");
    let back: NoteRecord = serde_json::from_value(legacy).unwrap();
    assert_eq!(back.zoom_restore, None);
}
#[test]
fn unzoom_returns_the_saved_frame_once_and_keeps_a_strip_a_strip() {
    let saved = Frame {
        x: 40.,
        y: 60.,
        width: 300.,
        height: 280.,
    };
    let mut note = NoteRecord::new("zoom".into(), Frame::default());
    assert_eq!(note.unzoom(), None);
    note.zoom_restore = Some(saved);
    note.frame = Frame {
        x: 0.,
        y: 0.,
        width: 1200.,
        height: 900.,
    };
    assert_eq!(note.unzoom(), Some(saved));
    assert_eq!((note.frame, note.expanded_height), (saved, 280.));
    assert_eq!(note.zoom_restore, None);
    // A second use zooms again rather than restoring twice.
    assert_eq!(note.unzoom(), None);
    // Collapsed, the strip height stays and the expanded height comes back with the frame.
    note.collapsed = true;
    note.expanded_height = 900.;
    note.zoom_restore = Some(saved);
    let frame = note.unzoom().unwrap();
    assert_eq!(frame.height, STRIP_HEIGHT);
    assert_eq!((frame.width, note.expanded_height), (300., 280.));
}
fn zoomed_note(collapsed_before: bool) -> (NoteRecord, Frame) {
    let resting = Frame {
        x: 40.,
        y: 60.,
        width: 300.,
        height: 350.,
    };
    let mut note = NoteRecord::new("zoom".into(), resting);
    note.collapsed = collapsed_before;
    if collapsed_before {
        note.frame.height = STRIP_HEIGHT;
    }
    (note, resting)
}
fn zoom_to_screen(note: &mut NoteRecord) {
    // What the window reports once the system zoom has finished.
    note.frame = Frame {
        x: 0.,
        y: 0.,
        width: 1200.,
        height: 900.,
    };
    note.expanded_height = 900.;
}
#[test]
fn zoom_remembers_the_expanded_geometry_and_restores_it_when_still_expanded() {
    let (mut note, resting) = zoomed_note(false);
    assert!(note.begin_zoom());
    assert_eq!(note.zoom_restore, Some(resting));
    zoom_to_screen(&mut note);
    assert_eq!(note.unzoom(), Some(resting));
    assert_eq!((note.frame, note.expanded_height), (resting, 350.));
    assert!(!note.collapsed);
}
#[test]
fn unzooming_a_note_collapsed_since_the_zoom_restores_its_expanded_height() {
    let (mut note, resting) = zoomed_note(false);
    assert!(note.begin_zoom());
    zoom_to_screen(&mut note);
    // Collapse keeps the zoomed width and the strip's top edge.
    note.collapsed = true;
    note.frame.height = STRIP_HEIGHT;
    let frame = note.unzoom().unwrap();
    assert_eq!(
        frame,
        Frame {
            height: STRIP_HEIGHT,
            ..resting
        }
    );
    assert_eq!(note.frame, frame);
    // Expanding afterwards gives the size the note had before the zoom, not a screen-tall one.
    assert_eq!(note.expanded_height, 350.);
}
#[test]
fn a_collapsed_strip_does_not_zoom_so_no_strip_height_is_ever_saved() {
    let (mut note, _) = zoomed_note(true);
    note.expanded_height = 350.;
    assert!(!note.begin_zoom());
    assert_eq!(note.zoom_restore, None);
    // Once expanded it zooms, and the saved frame is the full-height one.
    note.collapsed = false;
    note.frame.height = 350.;
    assert!(note.begin_zoom());
    zoom_to_screen(&mut note);
    let frame = note.unzoom().unwrap();
    assert_eq!((frame.height, note.expanded_height), (350., 350.));
}
#[test]
fn zoom_does_not_start_twice() {
    let (mut note, resting) = zoomed_note(false);
    assert!(note.begin_zoom());
    zoom_to_screen(&mut note);
    assert!(!note.begin_zoom());
    assert_eq!(note.zoom_restore, Some(resting));
}
#[test]
fn a_saved_strip_height_from_an_earlier_build_cannot_shrink_the_note() {
    let (mut note, resting) = zoomed_note(false);
    note.zoom_restore = Some(Frame {
        height: STRIP_HEIGHT,
        ..resting
    });
    zoom_to_screen(&mut note);
    assert_eq!(note.unzoom().unwrap().height, 160.);
    assert_eq!(note.expanded_height, 160.);
}
#[test]
fn restoring_a_collapsed_zoomed_note_keeps_the_saved_expanded_height() {
    let display = Frame {
        x: 0.,
        y: 0.,
        width: 2000.,
        height: 1200.,
    };
    let (mut note, resting) = zoomed_note(false);
    assert!(note.begin_zoom());
    zoom_to_screen(&mut note);
    note.collapsed = true;
    let restored = note.restored(&[display]);
    assert_eq!(restored.frame.height, STRIP_HEIGHT);
    assert_eq!(restored.zoom_restore, Some(resting));
}
#[test]
fn a_zoomed_note_defaults_to_its_pre_zoom_size() {
    let (mut note, _) = zoomed_note(false);
    assert_eq!(note.resting_size(), (300., 350.));
    assert!(note.begin_zoom());
    zoom_to_screen(&mut note);
    assert_eq!(note.resting_size(), (300., 350.));
    // A zoomed note collapsed since still reports the size it will return to.
    note.collapsed = true;
    assert_eq!(note.resting_size(), (300., 350.));
}
#[test]
fn use_as_default_stores_the_pre_zoom_size() {
    let _root_fixture = isolated("zoomed-default");
    let mut book = Notebook::open(_root_fixture.path().to_path_buf()).unwrap();
    let (mut note, _) = zoomed_note(false);
    assert!(note.begin_zoom());
    zoom_to_screen(&mut note);
    let pending = book.request_defaults(&note).unwrap();
    book.complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    let defaults = book.defaults();
    assert_eq!((defaults.width, defaults.height), (300., 350.));
    book.finish_for_test();
}
#[test]
fn arranging_a_note_forgets_its_zoom() {
    let (mut note, _) = zoomed_note(false);
    assert!(note.begin_zoom());
    zoom_to_screen(&mut note);
    let tile = Frame {
        x: 8.,
        y: 8.,
        width: 300.,
        height: STRIP_HEIGHT,
    };
    note.rearrange(tile, 350., true);
    assert_eq!(note.zoom_restore, None);
    assert_eq!((note.frame, note.expanded_height, note.collapsed), (tile, 350., true));
    assert_eq!(note.unzoom(), None);
}
#[test]
fn restoring_a_note_also_keeps_its_zoom_frame_on_a_display() {
    let display = Frame {
        x: 0.,
        y: 0.,
        width: 1000.,
        height: 800.,
    };
    let mut note = NoteRecord::new("zoom".into(), Frame::default());
    note.zoom_restore = Some(Frame {
        x: 5000.,
        y: 5000.,
        width: 300.,
        height: 280.,
    });
    let restored = note.restored(&[display]).zoom_restore.unwrap();
    assert!(restored.x + restored.width <= 1000. && restored.y + restored.height <= 800.);
}
#[test]
fn arrange_policy_persists_with_the_index() {
    use crate::window_commands::Policy;
    let _root_fixture = isolated("arrange-policy");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    assert_eq!(book.arrange_policy(), Policy::Keep);
    book.set_arrange_policy(Policy::Expand).unwrap();
    book.finish_for_test();
    let mut book = Notebook::open(root.clone()).unwrap();
    assert_eq!(book.arrange_policy(), Policy::Expand);
    // A choice made while another change is being written survives that change.
    let pending = book.request_create(Frame::default()).unwrap();
    book.set_arrange_policy(Policy::Collapse).unwrap();
    book.complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    assert_eq!(book.arrange_policy(), Policy::Collapse);
    book.finish_for_test();
    let book = Notebook::open(root.clone()).unwrap();
    assert_eq!(book.arrange_policy(), Policy::Collapse);
    // An index from before the setting existed, or from a newer version, reads as Leave As Is.
    let mut index = serde_json::to_value(book.index.clone()).unwrap();
    book.finish_for_test();
    index.as_object_mut().unwrap().remove("arrange_policy");
    fs::write(root.join("index.json"), serde_json::to_vec(&index).unwrap()).unwrap();
    let book = Notebook::open(root.clone()).unwrap();
    assert_eq!(book.arrange_policy(), Policy::Keep);
    book.finish_for_test();
    index["arrange_policy"] = "sideways".into();
    fs::write(root.join("index.json"), serde_json::to_vec(&index).unwrap()).unwrap();
    let book = Notebook::open(root.clone()).unwrap();
    assert_eq!(book.arrange_policy(), Policy::Keep);
    book.finish_for_test();
}
#[test]
fn a_zoom_and_style_saved_during_a_pending_commit_survive_its_completion() {
    let _root_fixture = isolated("rebase-zoom");
    let root = _root_fixture.path().to_path_buf();
    let mut book = Notebook::open(root.clone()).unwrap();
    let note = book.create(Frame::default()).unwrap();
    // Another note's creation is in flight while this one zooms and changes its text size.
    let pending = book.request_create(Frame::default()).unwrap();
    let mut zoomed = note.clone();
    assert!(zoomed.begin_zoom());
    zoomed.frame = Frame {
        x: 0.,
        y: 0.,
        width: 1200.,
        height: 900.,
    };
    zoomed.expanded_height = 900.;
    zoomed.style.reading_size = 14.;
    book.update(zoomed.clone()).unwrap();
    book.complete(futures::executor::block_on(pending.wait()))
        .unwrap();
    assert_eq!(book.note(&note.id), Some(zoomed.clone()));
    // The rebased state is also what reaches disk.
    book.finish_for_test();
    let book = Notebook::open(root).unwrap();
    assert_eq!(book.note(&note.id), Some(zoomed));
    book.finish_for_test();
}
fn strip_at(x: f32, y: f32) -> Frame {
    Frame {
        x,
        y,
        width: 300.,
        height: STRIP_HEIGHT,
    }
}
#[test]
fn expanding_a_strip_near_the_bottom_slides_the_note_up_onto_the_screen() {
    // A visible area below the menu bar and above the Dock.
    let display = Frame {
        x: 0.,
        y: 25.,
        width: 1000.,
        height: 775.,
    };
    let near_dock = strip_at(40., 760.).expanded_to(350., &[display]);
    assert_eq!((near_dock.x, near_dock.width, near_dock.height), (40., 300., 350.));
    assert_eq!(near_dock.y + near_dock.height, 800.);
    // Where the note fits, the top edge stays put.
    let roomy = strip_at(40., 300.).expanded_to(350., &[display]);
    assert_eq!((roomy.y, roomy.height), (300., 350.));
    // Exactly fitting is not a move.
    let snug = strip_at(40., 450.).expanded_to(350., &[display]);
    assert_eq!(snug.y, 450.);
    // The horizontal position is the user's own, even half off the screen.
    assert_eq!(strip_at(-100., 760.).expanded_to(350., &[display]).x, -100.);
}
#[test]
fn expanding_a_note_taller_than_the_screen_fills_it_from_the_top() {
    let display = Frame {
        x: 0.,
        y: 25.,
        width: 1000.,
        height: 300.,
    };
    let frame = strip_at(40., 200.).expanded_to(700., &[display]);
    assert_eq!((frame.y, frame.height), (25., 300.));
}
#[test]
fn expanding_uses_the_display_the_strip_is_on() {
    let primary = Frame {
        x: 0.,
        y: 25.,
        width: 1000.,
        height: 775.,
    };
    // A shorter external display to the right, whose top is level with the primary's.
    let external = Frame {
        x: 1000.,
        y: 25.,
        width: 800.,
        height: 500.,
    };
    let frame = strip_at(1100., 480.).expanded_to(350., &[primary, external]);
    assert_eq!(frame.y + frame.height, 525.);
    let frame = strip_at(100., 400.).expanded_to(350., &[primary, external]);
    assert_eq!(frame.y, 400.);
    // With no display to ask, the note expands where it is.
    assert_eq!(strip_at(100., 480.).expanded_to(350., &[]).y, 480.);
}
fn laptop() -> Frame {
    Frame {
        x: 0.,
        y: 25.,
        width: 1440.,
        height: 875.,
    }
}
fn note_at(x: f32, y: f32) -> Frame {
    Frame {
        x,
        y,
        width: 360.,
        height: 350.,
    }
}
#[test]
fn a_new_note_steps_down_and_right_from_the_front_one() {
    let next = note_at(100., 100.).cascaded(&[laptop()], &[]);
    assert_eq!((next.x, next.y), (128., 128.));
    assert_eq!((next.width, next.height), (360., 350.));
}
#[test]
fn the_cascade_wraps_to_the_top_left_instead_of_clamping_into_a_corner() {
    // Out of room on the right.
    let next = note_at(1060., 100.).cascaded(&[laptop()], &[]);
    assert_eq!((next.x, next.y), (28., 53.));
    // Out of room at the bottom.
    let next = note_at(100., 530.).cascaded(&[laptop()], &[]);
    assert_eq!((next.x, next.y), (28., 53.));
    // The last position that fits is still used.
    let next = note_at(1052., 497.).cascaded(&[laptop()], &[]);
    assert_eq!((next.x, next.y), (1080., 525.));
}
#[test]
fn a_long_run_of_new_notes_never_leaves_the_screen_or_stacks_in_a_corner() {
    let display = laptop();
    let mut taken: Vec<Frame> = vec![];
    let mut front = note_at(120., 120.);
    for _ in 0..40 {
        front = front.cascaded(&[display], &taken);
        assert!(front.x >= display.x && front.x + front.width <= display.x + display.width);
        assert!(front.y >= display.y && front.y + front.height <= display.y + display.height);
        assert!(
            !taken
                .iter()
                .any(|other| (other.x, other.y) == (front.x, front.y)),
            "two notes opened at {:?}",
            (front.x, front.y)
        );
        taken.push(front);
    }
}
#[test]
fn the_cascade_skips_a_spot_that_a_note_already_holds() {
    let held = [note_at(28., 53.)];
    let next = note_at(1060., 100.).cascaded(&[laptop()], &held);
    assert_eq!((next.x, next.y), (56., 81.));
}
#[test]
fn the_cascade_wraps_on_the_display_the_front_note_is_on() {
    let external = Frame {
        x: 1440.,
        y: 0.,
        width: 1000.,
        height: 600.,
    };
    let next = note_at(2100., 100.).cascaded(&[laptop(), external], &[]);
    assert_eq!((next.x, next.y), (1468., 28.));
}
#[test]
fn a_note_larger_than_the_display_cannot_hang_the_cascade() {
    let small = Frame {
        x: 0.,
        y: 0.,
        width: 300.,
        height: 200.,
    };
    let held = [note_at(28., 28.), note_at(56., 56.)];
    let next = note_at(10., 10.).cascaded(&[small], &held);
    assert!(next.x.is_finite() && next.y.is_finite());
    assert_eq!(note_at(10., 10.).cascaded(&[], &[]).x, 38.);
}
