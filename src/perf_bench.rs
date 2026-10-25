//! Performance benchmarks for the A+ gates in the plan. Ignored by default; run them with
//! `cargo test --release -- --ignored --nocapture perf_bench`. Each prints `PERF <name> <ms>`.
//! A bench asserts its gate once the code meets it (in release builds only); a gate the code
//! does not meet yet is marked `// GATE:` with the reason, and may assert a looser ceiling.
//! Every library and note lives in a fresh temporary directory, never the user's data folder.

use crate::notebook::{Frame, Notebook};
use crate::storage::NoteStore;
use crate::transfer::Imported;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const KB: usize = 1024;
const MB: usize = 1024 * KB;
/// About one match per kilobyte of generated Markdown.
const NEEDLE: &str = "needle";

/// Removes its directory on drop, including after a failed assertion.
struct TempRoot {
    _guard: tempfile::TempDir,
    path: PathBuf,
}
impl TempRoot {
    fn new(label: &str) -> Self {
        let guard = tempfile::Builder::new()
            .prefix(&format!("dahlia-perf-{label}-"))
            .tempdir()
            .unwrap();
        // Preserve the original measurement: the library directory does not exist yet.
        let path = guard.path().join("library");
        Self {
            _guard: guard,
            path,
        }
    }
    fn path(&self) -> &Path {
        &self.path
    }
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.
}
fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}
fn measure(runs: usize, mut f: impl FnMut()) -> Duration {
    median(
        (0..runs)
            .map(|_| {
                let start = Instant::now();
                f();
                start.elapsed()
            })
            .collect(),
    )
}
fn report(name: &str, duration: Duration) -> f64 {
    let value = ms(duration);
    println!("PERF {name:<52} {value:>10.3} ms");
    value
}
/// Gates are timing claims about optimized builds; debug builds only print.
fn gate(name: &str, value: f64, limit: f64) {
    if !cfg!(debug_assertions) {
        assert!(
            value < limit,
            "{name}: {value:.3} ms exceeds the {limit} ms gate"
        );
    }
}
fn label(bytes: usize) -> String {
    if bytes >= MB {
        format!("{}MB", bytes / MB)
    } else {
        format!("{}KB", bytes / KB)
    }
}

/// Deterministic prose-heavy Markdown of at least `bytes`, with headings, lists and emphasis.
fn markdown(bytes: usize) -> String {
    const WORDS: &[&str] = &[
        "the", "sticky", "note", "keeps", "a", "quiet", "record", "of", "plans", "and", "small",
        "ideas", "written", "between", "meetings", "with", "notes", "about", "copper", "dahlias",
    ];
    let mut seed = 0x2545_f491_u32;
    let mut word = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        WORDS[seed as usize % WORDS.len()]
    };
    let mut text = String::with_capacity(bytes + KB);
    let mut paragraph = 0;
    while text.len() < bytes {
        match paragraph % 12 {
            0 => text.push_str(&format!("## Section {paragraph}\n\n")),
            5 => {
                for _ in 0..4 {
                    text.push_str(&format!("- {} **{}** {}\n", word(), word(), word()));
                }
                text.push('\n');
            }
            _ => {}
        }
        let start = text.len();
        while text.len() - start < 900 {
            text.push_str(word());
            text.push(' ');
        }
        text.push_str(NEEDLE);
        text.push_str(" ends here.\n\n");
        paragraph += 1;
    }
    text
}

/// One commit imports every note; a tenth carry a 200 KB managed image, as in the measured pass.
fn library(root: &Path, notes: usize) -> Notebook {
    let mut book = Notebook::open(root.to_owned()).unwrap();
    let sources = (0..notes)
        .map(|n| {
            let mut images = BTreeMap::new();
            if n % 10 == 0 {
                let mut bytes = vec![0u8; 200 * KB];
                for (i, byte) in bytes.iter_mut().enumerate() {
                    *byte = (i.wrapping_mul(31) ^ n.wrapping_mul(7919)) as u8;
                }
                images.insert(crate::attachments::reference(&bytes), bytes);
            }
            Imported {
                text: format!("# Note {n}\n\n{}", markdown(KB)),
                title: format!("Note {n}"),
                linked_files: false,
                images,
                disclosures: Vec::new(),
            }
        })
        .collect();
    book.import_sources(sources, Frame::default()).unwrap();
    book
}
/// The metadata change every typing pause commits.
fn typing_pause(book: &mut Notebook) {
    let mut record = book.notes()[0].clone();
    record.modified_at += 1;
    book.update(record).unwrap();
}
/// A window move or resize, which is written promptly rather than at idle.
fn frame_change(book: &mut Notebook) {
    let mut record = book.notes()[0].clone();
    record.frame.x += 1.;
    book.update(record).unwrap();
}

#[test]
#[ignore]
fn notebook_update_per_typing_pause() {
    for notes in [1, 50, 500] {
        let root = TempRoot::new("pause");
        let mut book = library(root.path(), notes);
        typing_pause(&mut book);
        let pause = measure(15, || typing_pause(&mut book));
        let pause = report(&format!("notebook_update/{notes}_notes"), pause);
        // Typing pause <1 ms of UI-thread work: the write is queued for the metadata thread.
        gate(&format!("notebook_update/{notes}_notes"), pause, 1.);
        let frame = measure(15, || frame_change(&mut book));
        let frame = report(&format!("notebook_frame_change/{notes}_notes"), frame);
        gate(&format!("notebook_frame_change/{notes}_notes"), frame, 1.);
        // Quitting waits for this; typing never does.
        report(
            &format!("notebook_flush/{notes}_notes"),
            measure(1, || book.flush().unwrap()),
        );
    }
}

#[test]
#[ignore]
fn first_commit_includes_checkpoint() {
    for notes in [50, 500] {
        let root = TempRoot::new("first-commit");
        library(root.path(), notes).finish_for_test();
        let first = median(
            (0..3)
                .map(|_| {
                    let start = Instant::now();
                    let mut book = Notebook::open(root.path().to_owned()).unwrap();
                    typing_pause(&mut book);
                    frame_change(&mut book);
                    let elapsed = start.elapsed();
                    // Closing waits for the metadata thread; the next open needs the lock.
                    book.finish_for_test();
                    elapsed
                })
                .collect(),
        );
        let first = report(&format!("first_commit/{notes}_notes"), first);
        // No UI stall >16 ms in a 500-note library: the session's first checkpoint runs on
        // the metadata thread, so the UI waits only for opening and parsing the index.
        gate(&format!("first_commit/{notes}_notes"), first, 16.);
    }
}

#[test]
#[ignore]
fn checkpoint() {
    for notes in [50, 500] {
        let root = TempRoot::new("checkpoint");
        library(root.path(), notes).finish_for_test();
        // A session's first checkpoint reads every source and image once.
        let mut book = Notebook::open(root.path().to_owned()).unwrap();
        let start = Instant::now();
        book.checkpoint().unwrap();
        report(&format!("checkpoint_first/{notes}_notes"), start.elapsed());
        // Later ones clone what has not changed. Both run on the metadata thread; the UI
        // only waits for one when the user asks for a snapshot or a Replace All.
        let checkpoint = measure(5, || {
            book.checkpoint().unwrap();
        });
        let checkpoint = report(&format!("checkpoint/{notes}_notes"), checkpoint);
        gate(&format!("checkpoint/{notes}_notes"), checkpoint, 200.);
    }
}

#[test]
#[ignore]
fn note_store_save() {
    for bytes in [KB, MB, 10 * MB] {
        let root = TempRoot::new("save");
        let (mut store, _) = NoteStore::open(root.path().to_owned()).unwrap();
        let texts = [markdown(bytes), markdown(bytes).replacen(NEEDLE, "pin", 1)];
        let mut turn = 0;
        let save = measure(7, || {
            turn += 1;
            store.save(&texts[turn % 2]).unwrap();
        });
        // No gate: saves run on each note's writer thread, off the UI thread.
        report(&format!("note_store_save/{}", label(bytes)), save);
    }
}

#[test]
#[ignore]
fn title_and_plain_text() {
    let schema = markraft_commonmark::commonmark_schema();
    for bytes in [KB, 100 * KB, MB, 10 * MB] {
        let text = markdown(bytes);
        let source = markraft_commonmark::SourceDocument::parse(&schema, &text).unwrap();
        let doc = source.document();
        let runs = if bytes >= 10 * MB { 3 } else { 9 };
        let title = measure(runs, || {
            std::hint::black_box(crate::title_of(&schema, doc));
        });
        let plain = measure(runs, || {
            std::hint::black_box(markraft_commonmark::to_plain_text(&schema, doc));
        });
        let size = label(bytes);
        let title = report(&format!("title_of/{size}"), title);
        report(&format!("to_plain_text/{size}"), plain);
        // `title_of` runs on every keystroke; it reads only the first block with text.
        gate(&format!("title_of/{size}"), title, 0.1);
    }
}

/// A note editor with the real Changed handler; tests run off the main thread, so native
/// spell checking is unavailable and excluded here.
fn note(text: &str, root: &TempRoot, cx: &mut gpui::TestAppContext) -> gpui::Entity<crate::Note> {
    use gpui::AppContext;
    let (store, _) = NoteStore::open(root.path().to_owned()).unwrap();
    cx.new(|cx| crate::Note::new(store, text.to_owned(), cx))
}

#[gpui::test]
#[ignore]
fn keystroke(cx: &mut gpui::TestAppContext) {
    for bytes in [KB, 100 * KB, MB, 10 * MB] {
        let root = TempRoot::new("keystroke");
        let note = note(&markdown(bytes), &root, cx);
        let runs = if bytes >= 10 * MB { 5 } else { 15 };
        let keystroke = measure(runs, || {
            note.update(cx, |note, cx| {
                note.editor.update(cx, |editor, cx| {
                    let edit = markraft_core::commands::insert_text("x")(editor.state()).unwrap();
                    assert!(editor.dispatch([edit], cx));
                })
            });
        });
        let size = label(bytes);
        // Insert dispatch plus the Changed handler. Layout and painting need a real window and
        // are not measured here, so this gate is necessary but not sufficient.
        let keystroke = report(&format!("keystroke/{size}"), keystroke);
        // Keystroke <4 ms up to 1 MB and <16 ms at 10 MB.
        gate(
            &format!("keystroke/{size}"),
            keystroke,
            if bytes <= MB { 4. } else { 16. },
        );
        drop(note);
        cx.run_until_parked();
    }
}

#[gpui::test]
#[ignore]
fn find(cx: &mut gpui::TestAppContext) {
    let root = TempRoot::new("find");
    let note = note(&markdown(MB), &root, cx);
    // Alternate queries so each search does real work instead of reusing the last result.
    let queries = ["needl", NEEDLE];
    let mut turn = 0;
    let (mut ui, mut background) = (Vec::new(), Vec::new());
    let mut total = 0;
    for _ in 0..9 {
        turn += 1;
        let query = queries[turn % 2].to_owned();
        // A keystroke in the Find field only restarts a 150 ms timer. After the pause the
        // main thread snapshots the note, the search runs off it, and the hits are shown.
        let start = Instant::now();
        let (state, types) = note.read_with(cx, |note, cx| {
            let editor = note.editor.read(cx);
            (editor.state_without_find(), editor.doc_types())
        });
        let snapshot = start.elapsed();
        let start = Instant::now();
        let hits = markraft_gpui::find_hits_in(&state, &types, &query);
        background.push(start.elapsed());
        let start = Instant::now();
        note.update(cx, |note, cx| {
            note.editor.update(cx, |editor, cx| {
                editor.set_find_results(query, hits, cx);
                total = editor.find_status().total;
            })
        });
        ui.push(snapshot + start.elapsed());
    }
    assert!(total >= 1000, "expected about 1000 matches, found {total}");
    report("find/1MB/search_off_main_thread", median(background));
    let ui = report("find/1MB/main_thread", median(ui));
    // Find <16 ms per keystroke at 1 MB: the main thread only snapshots and shows hits.
    gate("find/1MB/main_thread", ui, 16.);
    let typing = measure(15, || {
        note.update(cx, |note, cx| {
            note.editor.update(cx, |editor, cx| {
                let edit = markraft_core::commands::insert_text("x")(editor.state()).unwrap();
                assert!(editor.dispatch([edit], cx));
            })
        });
    });
    let typing = report("find/1MB/typing_with_find_open", typing);
    gate("find/1MB/typing_with_find_open", typing, 4.);
    // The synchronous search the field used to run on every keystroke, for comparison.
    let sync = measure(5, || {
        turn += 1;
        note.update(cx, |note, cx| {
            note.editor.update(cx, |editor, cx| {
                editor.set_find_query(queries[turn % 2].into(), cx)
            })
        });
    });
    report("find/1MB/synchronous_search", sync);
}

#[gpui::test]
#[ignore]
fn replace_all(cx: &mut gpui::TestAppContext) {
    let root = TempRoot::new("replace-all");
    let note = note(&markdown(MB), &root, cx);
    note.update(cx, |note, cx| {
        note.editor
            .update(cx, |editor, cx| editor.set_find_query(NEEDLE.into(), cx))
    });
    // As Replace All does: snapshot on the main thread, then find, plan and verify off it.
    let start = Instant::now();
    let (state, types, source, preparation) = note.read_with(cx, |note, cx| {
        let editor = note.editor.read(cx);
        (
            editor.state_without_find(),
            editor.doc_types(),
            note.source.clone(),
            editor.prepare_edit(),
        )
    });
    let snapshot = report("replace_all/1MB/snapshot", start.elapsed());
    let start = Instant::now();
    let hits = markraft_gpui::find_hits_in(&state, &types, NEEDLE);
    let gather = report("replace_all/1MB/gather", start.elapsed());
    let total = hits.len();
    assert!(total >= 1000, "expected about 1000 matches, found {total}");
    let start = Instant::now();
    let spec = crate::finding::plan_replacements(&state, NEEDLE, hits, "pin")
        .unwrap()
        .unwrap();
    let plan = report("replace_all/1MB/plan", start.elapsed());
    let start = Instant::now();
    let prepared = preparation.prepare(spec).unwrap();
    report("replace_all/1MB/verify_transactions", start.elapsed());
    let render_start = Instant::now();
    source
        .prepare_transactions(state.schema(), prepared.transactions())
        .unwrap();
    report("replace_all/1MB/verify_source", render_start.elapsed());
    let verify = report("replace_all/1MB/verify", start.elapsed());
    let start = Instant::now();
    let applied = note.update(cx, |note, cx| {
        note.editor
            .update(cx, |editor, cx| editor.dispatch_prepared(prepared, cx))
    });
    let apply = report("replace_all/1MB/apply", start.elapsed());
    assert!(applied);
    let text = note.read_with(cx, |note, cx| note.source_text(cx).unwrap());
    assert!(!text.contains(NEEDLE), "every match is replaced");
    assert_eq!(text.matches("pin ends here").count(), total);
    let total = snapshot + gather + plan + verify + apply;
    report(
        "replace_all/1MB/total",
        Duration::from_secs_f64(total / 1000.),
    );
    gate("replace_all/1MB/total", total, 200.);
    gate("replace_all/1MB/apply", apply, 16.);
}

#[gpui::test]
#[ignore]
fn update_menus(cx: &mut gpui::TestAppContext) {
    let root = TempRoot::new("menus");
    let book = std::rc::Rc::new(std::cell::RefCell::new(library(root.path(), 500)));
    cx.update(|cx| {
        cx.set_global(crate::AppState::new(
            book.clone(),
            crate::appearance::Fonts::available(&[]),
        ))
    });
    let start = Instant::now();
    cx.update(crate::update_menus);
    report("update_menus/500_notes/rebuild", start.elapsed());
    // After every autosave: nothing the menus show has changed, so nothing is rebuilt.
    let unchanged = measure(15, || cx.update(crate::update_menus));
    let unchanged = report("update_menus/500_notes/unchanged", unchanged);
    gate("update_menus/500_notes/unchanged", unchanged, 1.);
}

#[gpui::test]
#[ignore]
fn spell_check(cx: &mut gpui::TestAppContext) {
    use crate::checking::{
        Checking, DocumentChecking, take_longest_step, tests::Fake, tests::finish,
    };
    use std::{cell::Cell, rc::Rc};
    let root = TempRoot::new("spell");
    let note = note(&markdown(MB), &root, cx);
    let editor = note.read_with(cx, |note, _| note.editor.clone());
    // The native checker needs the main thread, which tests do not run on; this one answers
    // at once, so every measured millisecond is the scan's own main-thread work.
    let requests = Rc::new(Cell::new(0));
    let mut checker = DocumentChecking::with_checker(Rc::new(Fake {
        requests: requests.clone(),
    }));
    take_longest_step();
    let start = Instant::now();
    let scan = cx.update(|cx| checker.refresh(&editor, Checking::default(), cx));
    finish(scan, cx);
    report("spell_check/1MB/automatic_scan", start.elapsed());
    let start = Instant::now();
    let scan = cx.update(|cx| checker.check_all(&editor, Checking::default(), cx));
    finish(scan, cx);
    report("spell_check/1MB/check_document", start.elapsed());
    let asked = requests.get();
    // A typing pause rechecks the changed line only.
    let incremental = measure(5, || {
        editor.update(cx, |editor, cx| {
            let edit = markraft_core::commands::insert_text("x")(editor.state()).unwrap();
            assert!(editor.dispatch([edit], cx));
        });
        let scan = cx.update(|cx| checker.refresh(&editor, Checking::default(), cx));
        finish(scan, cx);
    });
    assert_eq!(requests.get() - asked, 5, "one changed line per pause");
    let incremental = report("spell_check/1MB/typing_pause", incremental);
    gate("spell_check/1MB/typing_pause", incremental, 16.);
    // Spell check never blocks the main thread: it returns to the event loop within a frame.
    let longest = report(
        "spell_check/1MB/longest_main_thread_step",
        take_longest_step(),
    );
    gate("spell_check/1MB/longest_main_thread_step", longest, 16.);
}

/// Resident memory of this process, in megabytes.
fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Cannot measure process RSS: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .unwrap()
        / 1024.
}

#[gpui::test]
#[ignore]
fn open_large_note(cx: &mut gpui::TestAppContext) {
    use crate::{AppState, appearance::Fonts};
    let root = TempRoot::new("open-large");
    let book = std::rc::Rc::new(std::cell::RefCell::new(
        Notebook::open(root.path().to_owned()).unwrap(),
    ));
    cx.update(|cx| cx.set_global(AppState::new(book.clone(), Fonts::available(&[]))));
    let record = book.borrow_mut().create(Frame::default()).unwrap();
    let text = markdown(10 * MB);
    let (mut store, _) = book.borrow().open_store(&record.id).unwrap();
    store.save(&text).unwrap();
    drop(store);
    let start = Instant::now();
    cx.update(|cx| crate::open_note(&record.id, cx)).unwrap();
    let shown = report("open_note/10MB/window_shown", start.elapsed());
    // A 10 MB note shows its window in <500 ms: it opens on an empty stand-in.
    gate("open_note/10MB/window_shown", shown, 500.);
    let start = Instant::now();
    let schema = markraft_commonmark::commonmark_schema();
    let parsed = crate::parse_note(&schema, &text).unwrap();
    let prepared = crate::editor_setup(&schema, &parsed).prepare();
    report("open_note/10MB/prepare_off_main_thread", start.elapsed());
    let window = cx
        .update(|cx| cx.global::<AppState>().windows.get(&record.id).copied())
        .unwrap();
    let start = Instant::now();
    window
        .update(cx, |note, window, cx| {
            note.finish_opening_prepared(parsed, prepared, window, cx)
        })
        .unwrap();
    // Parsing, projection and immutable analysis are prepared before this UI step.
    let installed = report("open_note/10MB/show_content", start.elapsed());
    gate("open_note/10MB/show_content", installed, 16.);
    cx.run_until_parked();
}

/// Not a gate: what one note costs in memory, by stage, to see whose memory it is. Resident
/// memory is only meaningful in a fresh process, so this runs `memory_probe` in a child.
#[test]
#[ignore]
fn memory_per_note() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "perf_bench::memory_probe",
            "--nocapture",
        ])
        .env("DAHLIA_MEMORY_PROBE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Memory probe failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<_> = printed
        .lines()
        .filter(|line| line.contains("MEM "))
        .collect();
    assert_eq!(lines.len(), 3, "{printed}");
    for line in lines {
        println!("{}", &line[line.find("MEM ").unwrap()..]);
    }
}
#[gpui::test]
#[ignore]
fn memory_probe(cx: &mut gpui::TestAppContext) {
    if std::env::var_os("DAHLIA_MEMORY_PROBE").is_none() {
        return;
    }
    let text = markdown(10 * MB);
    let schema = markraft_commonmark::commonmark_schema();
    let base = rss_mb();
    let parsed = markraft_commonmark::SourceDocument::parse(&schema, &text).unwrap();
    let after_parse = rss_mb();
    let projection = markraft_core::projection::Projection::of(parsed.document(), &schema);
    let after_projection = rss_mb();
    let root = TempRoot::new("memory");
    let source = parsed.clone();
    let window = cx.add_window(|_, cx| {
        let (store, _) = NoteStore::open(root.path().to_owned()).unwrap();
        let record = crate::NoteRecord {
            id: "memory".into(),
            title: String::new(),
            open: true,
            frame: Frame::default(),
            expanded_height: 350.,
            collapsed: false,
            floating: false,
            style: crate::appearance::NoteStyle::legacy(),
            state: crate::NoteState::Active,
            state_changed_at: 0,
            source_base: None,
            modified_at: 0,
            zoom_restore: None,
        };
        crate::Note::new_record(store, source, record, None, cx)
    });
    cx.run_until_parked();
    let after_note = rss_mb();
    println!(
        "MEM memory/10MB/markraft_document {:>8.0} MB",
        after_parse - base
    );
    println!(
        "MEM memory/10MB/markraft_projection {:>6.0} MB",
        after_projection - after_parse
    );
    println!(
        "MEM memory/10MB/note_on_that_document {:>4.0} MB",
        after_note - after_projection
    );
    std::hint::black_box((&parsed, &projection, &window));
}
