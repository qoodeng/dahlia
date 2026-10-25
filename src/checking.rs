//! Prose checking uses Markraft's reading/source map and guarded edit funnel.
// Native range conversion and checks adapted from the MIT-licensed Markraft host.
use futures::channel::oneshot;
use gpui::{App, Entity, Task};
use markraft_core::projection::Line;
use markraft_gpui::{ContextRequest, EditorView};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
    time::Duration,
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Checking {
    pub spelling: bool,
    pub grammar: bool,
    pub correction: bool,
    pub quotes: bool,
    pub dashes: bool,
    pub replacements: bool,
    pub links: bool,
    pub data_detectors: bool,
    pub smart_copy_paste: bool,
}
impl Default for Checking {
    fn default() -> Self {
        Self {
            spelling: true,
            grammar: true,
            data_detectors: true,
            correction: false,
            quotes: false,
            dashes: false,
            replacements: false,
            links: false,
            smart_copy_paste: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Spelling,
    Grammar,
    Correction,
    Quotes,
    Dashes,
    Replacements,
    Links,
    DataDetectors,
    SmartCopyPaste,
}
impl Setting {
    pub const ALL: [Self; 9] = [
        Self::Spelling,
        Self::Grammar,
        Self::Correction,
        Self::Quotes,
        Self::Dashes,
        Self::Replacements,
        Self::Links,
        Self::DataDetectors,
        Self::SmartCopyPaste,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Spelling => "Check Spelling While Typing",
            Self::Grammar => "Check Grammar With Spelling",
            Self::Correction => "Correct Spelling Automatically",
            Self::Quotes => "Smart Quotes",
            Self::Dashes => "Smart Dashes",
            Self::Replacements => "Text Replacement",
            Self::Links => "Smart Links",
            Self::DataDetectors => "Data Detectors",
            Self::SmartCopyPaste => "Smart Copy/Paste",
        }
    }
}
impl Checking {
    fn option(&mut self, setting: Setting) -> &mut bool {
        match setting {
            Setting::Spelling => &mut self.spelling,
            Setting::Grammar => &mut self.grammar,
            Setting::Correction => &mut self.correction,
            Setting::Quotes => &mut self.quotes,
            Setting::Dashes => &mut self.dashes,
            Setting::Replacements => &mut self.replacements,
            Setting::Links => &mut self.links,
            Setting::DataDetectors => &mut self.data_detectors,
            Setting::SmartCopyPaste => &mut self.smart_copy_paste,
        }
    }
    pub fn enabled(mut self, setting: Setting) -> bool {
        *self.option(setting)
    }
    pub fn toggle(&mut self, setting: Setting) {
        let option = self.option(setting);
        *option = !*option;
    }
    pub fn native_supported() -> bool {
        cfg!(target_os = "macos")
    }
}

// Only the macOS checker constructs the native-only kinds; other platforms check nothing yet.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Spelling,
    Grammar,
    Quote,
    Dash,
    Replacement,
    Correction,
    Link,
    Date,
    Address,
    Phone,
    Transit,
}
#[derive(Clone, Debug)]
pub struct TextCheck {
    pub kind: Kind,
    /// Unicode scalars in checked reading text; document positions in `results()`.
    pub range: Range<usize>,
    pub replacement: Option<String>,
    pub suggestions: Vec<String>,
    pub detail: Option<String>,
    pub url: Option<String>,
}

/// How a document scan ended. Only a complete scan may report that nothing was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scan {
    Complete,
    /// A newer scan or an edit replaced this one before it finished.
    Superseded,
    /// No native checker is available on this system.
    Unavailable,
}

/// Automatic checking of a note with more reading text than this covers only the lines
/// near the caret and the lines that change; Check Document Now still covers everything.
const AUTOMATIC_LIMIT: usize = 256 * 1024;
/// How many lines on each side of the caret a capped automatic scan covers.
const NEARBY_LINES: usize = 150;
/// A scan returns to the event loop at least this often.
const STEP_BUDGET: Duration = Duration::from_millis(4);

/// The text checker a scan asks. Native on macOS; tests supply their own.
pub(crate) trait Checker {
    /// Check `text` without blocking the caller; resolves with the checks found.
    fn request(&self, text: &str, options: Checking) -> oneshot::Receiver<Vec<TextCheck>>;
    /// Check `text` now, for substitutions that must apply at the caret as it is typed.
    fn check(&self, text: &str, options: Checking) -> Vec<TextCheck>;
    fn ignore(&self, word: &str);
    fn guesses(&self, word: &str) -> Vec<String>;
}

/// One projection line's checks, kept while the line's body is unchanged. Ranges are
/// relative to the line's start, so they survive edits elsewhere that move the line.
#[derive(Clone)]
struct LineChecks {
    line: Line,
    /// `None` until the line has been checked.
    checks: Option<Vec<TextCheck>>,
}
#[derive(Default)]
struct Cache {
    options: Option<Checking>,
    lines: Vec<LineChecks>,
}

/// One lifetime per note, including the native tag used by Ignore Word.
type CheckingResults = Rc<RefCell<Option<(ContextRequest, Vec<TextCheck>)>>>;
#[derive(Default)]
pub struct DocumentChecking {
    native: Option<Rc<dyn Checker>>,
    generation: Rc<Cell<u64>>,
    results: CheckingResults,
    cache: Rc<RefCell<Cache>>,
}
impl DocumentChecking {
    pub fn new() -> Self {
        Self::default()
    }
    #[cfg(test)]
    pub(crate) fn with_checker(checker: Rc<dyn Checker>) -> Self {
        Self {
            native: Some(checker),
            ..Self::default()
        }
    }
    fn native(&mut self) -> Option<Rc<dyn Checker>> {
        if self.native.is_none() {
            self.native = NativeDocument::new().map(|native| Rc::new(native) as Rc<dyn Checker>);
        }
        self.native.clone()
    }
    /// Results are source positions, ready for a host's contextual actions.
    pub fn results(&self, editor: &EditorView) -> Vec<TextCheck> {
        self.results
            .borrow()
            .as_ref()
            .filter(|(snapshot, _)| editor.context_document_is_current(snapshot))
            .map(|(_, checks)| checks.clone())
            .unwrap_or_default()
    }
    /// Check the lines that changed since the last scan, asking the checker off the main
    /// thread. A large note's automatic scan covers only the lines near the caret.
    /// Debounce in the host; retain this task until completion. Settings never reload the document.
    pub fn refresh(
        &mut self,
        editor: &Entity<EditorView>,
        options: Checking,
        cx: &mut App,
    ) -> Task<Scan> {
        self.scan(editor, options, false, cx)
    }
    /// `refresh` over every line, whatever the note's size, as Check Document Now asks.
    pub fn check_all(
        &mut self,
        editor: &Entity<EditorView>,
        options: Checking,
        cx: &mut App,
    ) -> Task<Scan> {
        self.scan(editor, options, true, cx)
    }
    fn scan(
        &mut self,
        editor: &Entity<EditorView>,
        options: Checking,
        everything: bool,
        cx: &mut App,
    ) -> Task<Scan> {
        self.generation.set(self.generation.get().wrapping_add(1));
        let serial = self.generation.get();
        let generation = self.generation.clone();
        let results = self.results.clone();
        let cache = self.cache.clone();
        results.borrow_mut().take();
        editor.update(cx, |editor, cx| {
            editor.set_smart_insert_delete(options.smart_copy_paste);
            editor.set_text_diagnostics(Vec::new(), cx);
        });
        let native = self.native();
        let snapshot = editor.read(cx).context_document_snapshot();
        let projection = editor.read(cx).projection();
        let caret = projection
            .pos_to_line_offset(editor.read(cx).head())
            .map_or(0, |(line, _)| line);
        let editor = editor.downgrade();
        cx.spawn(async move |cx| {
            if generation.get() != serial {
                return Scan::Superseded;
            }
            let Some(native) = native else {
                return Scan::Unavailable;
            };
            let current = || generation.get() == serial;
            let mut step = std::time::Instant::now();
            // Lines whose bodies survived the edits since the last scan keep their checks.
            {
                let mut cache = cache.borrow_mut();
                if cache.options != Some(options) {
                    *cache = Cache {
                        options: Some(options),
                        lines: Vec::new(),
                    };
                }
                cache.lines = align(&cache.lines, projection.lines());
            }
            let capped = !everything && projection.plain_text().len() > AUTOMATIC_LIMIT;
            let mut pending: Vec<usize> = cache
                .borrow()
                .lines
                .iter()
                .enumerate()
                .filter(|(index, line)| {
                    line.checks.is_none() && (!capped || index.abs_diff(caret) <= NEARBY_LINES)
                })
                .map(|(index, _)| index)
                .collect();
            // The edited, visible text first.
            pending.sort_by_key(|index| index.abs_diff(caret));
            for index in pending {
                let (Some(line), Some(text)) =
                    (projection.line(index).cloned(), projection.line_text(index))
                else {
                    continue;
                };
                let mut found = Vec::new();
                for chunk in checking_chunks(text, 2048) {
                    if !current() {
                        return Scan::Superseded;
                    }
                    if step.elapsed() >= STEP_BUDGET {
                        #[cfg(test)]
                        record_step(step.elapsed());
                        cx.background_executor()
                            .timer(Duration::from_millis(1))
                            .await;
                        step = std::time::Instant::now();
                        if !current() {
                            return Scan::Superseded;
                        }
                    }
                    let (Some(from), Some(to)) = (
                        line.offset_to_pos(chunk.start),
                        line.offset_to_pos(chunk.end),
                    ) else {
                        continue;
                    };
                    if from == to {
                        continue;
                    }
                    let reading = editor
                        .update(cx, |editor, _| {
                            if !editor.context_document_is_current(&snapshot) {
                                return None;
                            }
                            let request = editor.context_source_range_snapshot(from..to)?;
                            let text = prose_reading(editor, &request)?;
                            Some((request, text))
                        })
                        .ok()
                        .flatten();
                    let Some((request, text)) = reading else {
                        return Scan::Superseded;
                    };
                    if text.trim().is_empty() {
                        continue;
                    }
                    let checked = native.request(
                        &text,
                        Checking {
                            correction: false,
                            quotes: false,
                            dashes: false,
                            replacements: false,
                            links: true,
                            ..options
                        },
                    );
                    // The checker works off the main thread; a scan only waits for it.
                    let mut checks = checked.await.unwrap_or_default();
                    exclude_url_edits(&mut checks, options.links);
                    let mapped = editor
                        .update(cx, |editor, _| map_prose_checks(editor, &request, checks))
                        .ok()
                        .flatten();
                    let Some(mapped) = mapped else {
                        return Scan::Superseded;
                    };
                    found.extend(mapped);
                }
                if !current() {
                    return Scan::Superseded;
                }
                let start = line.from();
                if let Some(entry) = cache.borrow_mut().lines.get_mut(index) {
                    entry.checks = Some(
                        found
                            .into_iter()
                            .map(|mut check| {
                                check.range = check.range.start - start..check.range.end - start;
                                check
                            })
                            .collect(),
                    );
                }
            }
            if !current() {
                return Scan::Superseded;
            }
            let found: Vec<TextCheck> = cache
                .borrow()
                .lines
                .iter()
                .filter_map(|entry| Some((entry.line.from(), entry.checks.as_ref()?)))
                .flat_map(|(start, checks)| {
                    checks.iter().cloned().map(move |mut check| {
                        check.range = check.range.start + start..check.range.end + start;
                        check
                    })
                })
                .collect();
            let current = editor.update(cx, |editor, cx| {
                if !editor.context_document_is_current(&snapshot) {
                    return false;
                }
                editor.set_text_diagnostics(
                    found
                        .iter()
                        .filter(|check| matches!(check.kind, Kind::Spelling | Kind::Grammar))
                        .map(|check| check.range.clone())
                        .collect(),
                    cx,
                );
                *results.borrow_mut() = Some((snapshot, found));
                true
            });
            #[cfg(test)]
            record_step(step.elapsed());
            if current.unwrap_or(false) {
                Scan::Complete
            } else {
                Scan::Superseded
            }
        })
    }
    /// Call ONLY on EditorEvent::Changed { text_input: true, .. }.
    pub fn typed(&mut self, editor: &Entity<EditorView>, options: Checking, cx: &mut App) -> bool {
        if !(options.quotes
            || options.dashes
            || options.replacements
            || options.correction
            || options.links)
        {
            return false;
        }
        let Some((request, boundary)) = typing_context(editor.read(cx)) else {
            return false;
        };
        let Some(text) = prose_reading(editor.read(cx), &request) else {
            return false;
        };
        let Some(native) = self.native() else {
            return false;
        };
        let mut checks = native.check(
            &text,
            Checking {
                spelling: false,
                grammar: false,
                data_detectors: false,
                links: true,
                ..options
            },
        );
        exclude_url_edits(&mut checks, options.links);
        // Cocoa can invent a scheme for bare domain labels. Keep their literal spelling.
        checks.retain(|check| {
            check.kind != Kind::Link
                || text
                    .chars()
                    .skip(check.range.start)
                    .take(check.range.len())
                    .collect::<String>()
                    .split_once(':')
                    .is_some_and(|(scheme, rest)| {
                        !rest.is_empty()
                            && scheme.starts_with(|first: char| first.is_ascii_alphabetic())
                            && scheme
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                    })
        });
        editor.update(cx, |editor, cx| {
            apply_substitution(editor, &request, boundary, checks, cx)
        })
    }
    pub fn ignore(&mut self, word: &str) {
        if let Some(native) = self.native() {
            native.ignore(word);
        }
        // Kept checks may flag the word; the next scan checks every line again.
        self.cache.borrow_mut().lines.clear();
    }
    pub fn guesses(&mut self, word: &str) -> Vec<String> {
        self.native()
            .map(|native| native.guesses(word))
            .unwrap_or_default()
    }
}

#[cfg(test)]
thread_local! {
    static LONGEST_STEP: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}
#[cfg(test)]
fn record_step(step: Duration) {
    LONGEST_STEP.with(|longest| longest.set(longest.get().max(step)));
}
/// The longest a scan has run on the main thread without returning to the event loop.
#[cfg(test)]
pub(crate) fn take_longest_step() -> Duration {
    LONGEST_STEP.with(|longest| longest.replace(Duration::ZERO))
}

/// Line checks for `new`, keeping those of the unchanged lines at its start and end.
/// Markraft keeps the body of every line an edit did not reach.
fn align(old: &[LineChecks], new: &[Line]) -> Vec<LineChecks> {
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(old, new)| old.line.same_body(new))
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(old, new)| old.line.same_body(new))
        .count();
    new.iter()
        .enumerate()
        .map(|(index, line)| {
            let kept = if index < prefix {
                old.get(index)
            } else if index >= new.len() - suffix {
                old.get(old.len() - (new.len() - index))
            } else {
                None
            };
            LineChecks {
                line: line.clone(),
                checks: kept.and_then(|kept| kept.checks.clone()),
            }
        })
        .collect()
}

/// Mask code and generated display pieces before asking the OS; scalar offsets stay stable.
fn prose_reading(editor: &EditorView, request: &ContextRequest) -> Option<String> {
    let mapped = editor.context_text_mapping(request)?;
    let types = markraft_core::kind::DocTypes::from_schema_names(
        editor.state().schema(),
        &markraft_commonmark::commonmark_doc_type_names(),
    );
    Some(
        mapped
            .text
            .chars()
            .zip(mapped.source)
            .map(|(character, source)| {
                let prose = source.is_some_and(|range| {
                    editor
                        .state()
                        .doc()
                        .resolve(range.start)
                        .is_ok_and(|resolved| !types.is_verbatim(resolved.parent().type_id()))
                        && editor
                            .state()
                            .doc()
                            .node_at(range.start)
                            .is_some_and(|node| {
                                ![types.code, types.math]
                                    .into_iter()
                                    .flatten()
                                    .any(|ty| node.marks().contains_type(ty))
                            })
                });
                if prose { character } else { ' ' }
            })
            .collect(),
    )
}

fn map_prose_checks(
    editor: &EditorView,
    request: &ContextRequest,
    checks: Vec<TextCheck>,
) -> Option<Vec<TextCheck>> {
    if !editor.context_is_current(request) || editor.is_composing() {
        return None;
    }
    Some(
        checks
            .into_iter()
            .filter_map(|mut check| {
                let range = editor.context_text_range(request, check.range.clone())?;
                if !editor.context_is_prose(&range) {
                    return None;
                }
                check.range = range.text_range();
                Some(check)
            })
            .collect(),
    )
}
#[derive(Clone, Copy)]
struct TypingBoundary {
    caret: usize,
    completed_end: Option<usize>,
}
impl TypingBoundary {
    fn accepts(self, kind: Kind, end: usize) -> bool {
        match kind {
            Kind::Quote | Kind::Dash => end == self.caret || Some(end) == self.completed_end,
            Kind::Replacement | Kind::Correction | Kind::Link => Some(end) == self.completed_end,
            _ => false,
        }
    }
}
fn apply_substitution(
    editor: &mut EditorView,
    paragraph: &ContextRequest,
    boundary: TypingBoundary,
    checks: Vec<TextCheck>,
    cx: &mut gpui::Context<EditorView>,
) -> bool {
    for check in checks.into_iter().rev() {
        let Some(request) = editor.context_text_range(paragraph, check.range.clone()) else {
            continue;
        };
        if !boundary.accepts(check.kind, request.text_range().end)
            || !editor.context_is_prose(&request)
        {
            continue;
        }
        if let Some(replacement) = check.replacement {
            if editor.apply_context_text_checks(
                paragraph,
                vec![(check.range, replacement)],
                Vec::new(),
                cx,
            ) {
                return true;
            }
        } else if let Some(url) = check.url
            && editor.set_context_link(&request, &url, cx)
        {
            return true;
        }
    }
    false
}
fn typing_context(editor: &EditorView) -> Option<(ContextRequest, TypingBoundary)> {
    editor.context_paragraph_snapshot_at_caret()?;
    let projection = editor.projection();
    let caret = editor.head();
    let (index, offset) = projection.pos_to_line_offset(caret)?;
    let (line_index, completed_end) = if offset == 0 && index > 0 {
        (index - 1, Some(projection.line(index - 1)?.to()))
    } else {
        let completed = projection
            .line_text(index)?
            .chars()
            .nth(offset.checked_sub(1)?)
            .filter(|c| !c.is_alphanumeric() && !matches!(c, '_' | '\'' | '’'))
            .and_then(|_| projection.line(index)?.offset_to_pos(offset - 1));
        (index, completed)
    };
    let line = projection.line(line_index)?;
    let end = if line_index == index {
        offset
    } else {
        line.len()
    };
    let from = line.offset_to_pos(end.saturating_sub(1024))?;
    let to = line.offset_to_pos((end + 2).min(line.len()))?;
    Some((
        editor.context_source_range_snapshot(from..to)?,
        TypingBoundary {
            caret,
            completed_end,
        },
    ))
}
fn exclude_url_edits(checks: &mut Vec<TextCheck>, include_links: bool) {
    let links: Vec<_> = checks
        .iter()
        .filter(|check| check.kind == Kind::Link)
        .map(|check| check.range.clone())
        .collect();
    checks.retain(|check| {
        if check.kind == Kind::Link {
            include_links
        } else {
            !links
                .iter()
                .any(|link| check.range.start < link.end && link.start < check.range.end)
        }
    });
}
fn checking_chunks(text: &str, limit: usize) -> Vec<Range<usize>> {
    let (mut chunks, mut start, mut end) = (Vec::new(), 0, 0);
    for word in text.split_word_bounds() {
        let length = word.chars().count();
        if end - start + length > limit {
            if start < end {
                chunks.push(start..end);
            }
            start = end;
        }
        end += length;
        // An overlong token cannot be checked in fragments without changing its meaning.
        if length > limit {
            start = end;
        }
    }
    if start < end {
        chunks.push(start..end);
    }
    chunks
}

#[cfg(not(target_os = "macos"))]
struct NativeDocument;
#[cfg(not(target_os = "macos"))]
impl NativeDocument {
    fn new() -> Option<Self> {
        None
    }
}
#[cfg(not(target_os = "macos"))]
impl Checker for NativeDocument {
    fn request(&self, _: &str, _: Checking) -> oneshot::Receiver<Vec<TextCheck>> {
        oneshot::channel().1
    }
    fn check(&self, _: &str, _: Checking) -> Vec<TextCheck> {
        Vec::new()
    }
    fn ignore(&self, _: &str) {}
    fn guesses(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}
#[cfg(target_os = "macos")]
use native::NativeDocument;

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use block2::RcBlock;
    use objc2::{MainThreadMarker, class, msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::{
        NSArray, NSDataDetector, NSGrammarCorrections, NSGrammarRange, NSGrammarUserDescription,
        NSMatchingOptions, NSRange, NSString, NSTextCheckingResult, NSTextCheckingType,
    };
    use std::ptr::{self, NonNull};
    const DATA_TYPES: u64 = NSTextCheckingType::Date.0
        | NSTextCheckingType::Address.0
        | NSTextCheckingType::PhoneNumber.0
        | NSTextCheckingType::TransitInformation.0;

    pub(super) struct NativeDocument {
        checker: Retained<AnyObject>,
        detector: Retained<NSDataDetector>,
        tag: isize,
    }
    impl NativeDocument {
        pub(super) fn new() -> Option<Self> {
            MainThreadMarker::new()?;
            // SAFETY: main-thread marker checked above; NSSpellChecker returns its shared checker.
            let checker: Option<Retained<AnyObject>> =
                unsafe { msg_send![class!(NSSpellChecker), sharedSpellChecker] };
            let detector = NSDataDetector::dataDetectorWithTypes_error(DATA_TYPES).ok()?;
            // SAFETY: AppKit class selector returns an integer tag, called on the main thread.
            let tag = unsafe { msg_send![class!(NSSpellChecker), uniqueSpellDocumentTag] };
            Some(Self {
                checker: checker?,
                detector,
                tag,
            })
        }
        fn mask(options: Checking) -> u64 {
            [
                (options.spelling, NSTextCheckingType::Spelling),
                (options.grammar, NSTextCheckingType::Grammar),
                (options.quotes, NSTextCheckingType::Quote),
                (options.dashes, NSTextCheckingType::Dash),
                (options.replacements, NSTextCheckingType::Replacement),
                (options.correction, NSTextCheckingType::Correction),
                (options.links, NSTextCheckingType::Link),
            ]
            .into_iter()
            .filter(|(enabled, _)| *enabled)
            .fold(0, |mask, (_, kind)| mask | kind.0)
        }
        fn detected(&self, text: &str, options: Checking) -> Vec<Retained<NSTextCheckingResult>> {
            if !options.data_detectors {
                return Vec::new();
            }
            let string = NSString::from_str(text);
            self.detector
                .matchesInString_options_range(
                    &string,
                    NSMatchingOptions::empty(),
                    NSRange::new(0, text.encode_utf16().count()),
                )
                .iter()
                .collect()
        }
    }
    impl Checker for NativeDocument {
        /// The spell server answers on its own queue, so the main thread never waits on it.
        fn request(&self, text: &str, options: Checking) -> oneshot::Receiver<Vec<TextCheck>> {
            let (reply, receiver) = oneshot::channel();
            if MainThreadMarker::new().is_none() || text.is_empty() {
                let _ = reply.send(Vec::new());
                return receiver;
            }
            let detected = self.detected(text, options);
            let mask = Self::mask(options);
            if mask == 0 {
                let _ = reply.send(convert(text, detected));
                return receiver;
            }
            let reply = std::sync::Mutex::new(Some(reply));
            let owned = text.to_owned();
            // Data detector results are immutable and safe to read on the server's queue.
            let detected = std::sync::Mutex::new(Some(detected));
            let handler = RcBlock::new(
                move |_: isize,
                      checked: NonNull<NSArray<NSTextCheckingResult>>,
                      _: *mut AnyObject,
                      _: isize| {
                    // SAFETY: AppKit passes a valid array that lives for the duration of the call.
                    let checked = unsafe { checked.as_ref() };
                    let mut results: Vec<_> = checked.iter().collect();
                    results.extend(
                        detected
                            .lock()
                            .ok()
                            .and_then(|mut d| d.take())
                            .unwrap_or_default(),
                    );
                    if let Some(reply) = reply.lock().ok().and_then(|mut reply| reply.take()) {
                        let _ = reply.send(convert(&owned, results));
                    }
                },
            );
            let string = NSString::from_str(text);
            let range = NSRange::new(0, text.encode_utf16().count());
            // SAFETY: the arguments match the selector; AppKit copies the block and calls it
            // once, on a queue of its own.
            let _: isize = unsafe {
                msg_send![&*self.checker, requestCheckingOfString: &*string, range: range, types: mask,
                    options: ptr::null::<AnyObject>(), inSpellDocumentWithTag: self.tag,
                    completionHandler: &*handler]
            };
            receiver
        }
        fn check(&self, text: &str, options: Checking) -> Vec<TextCheck> {
            if MainThreadMarker::new().is_none() || text.is_empty() {
                return Vec::new();
            }
            let string = NSString::from_str(text);
            let native_range = NSRange::new(0, text.encode_utf16().count());
            let mask = Self::mask(options);
            let mut results = Vec::new();
            if mask != 0 {
                // SAFETY: retained checker/string, bounded UTF-16 range and nullable out parameters
                // match this AppKit selector; the main-thread check above guards the call.
                let checked: Retained<NSArray<NSTextCheckingResult>> = unsafe {
                    msg_send![&*self.checker, checkString: &*string, range: native_range, types: mask,
                        options: ptr::null::<AnyObject>(), inSpellDocumentWithTag: self.tag,
                        orthography: ptr::null_mut::<*mut AnyObject>(), wordCount: ptr::null_mut::<isize>()]
                };
                results.extend(checked.iter());
            }
            results.extend(self.detected(text, options));
            convert(text, results)
        }
        fn ignore(&self, word: &str) {
            if MainThreadMarker::new().is_some() && !word.is_empty() {
                // SAFETY: retained checker and live NSString, valid document tag, main-thread call.
                unsafe {
                    let _: () = msg_send![&*self.checker, ignoreWord: &*NSString::from_str(word), inSpellDocumentWithTag: self.tag];
                }
            }
        }
        fn guesses(&self, word: &str) -> Vec<String> {
            if MainThreadMarker::new().is_none() || word.is_empty() {
                return Vec::new();
            }
            // SAFETY: live NSString, valid UTF-16 range and document tag; main-thread check above.
            let guesses: Option<Retained<NSArray<NSString>>> = unsafe {
                msg_send![&*self.checker,
                guessesForWordRange: NSRange::new(0, word.encode_utf16().count()),
                inString: &*NSString::from_str(word), language: ptr::null::<NSString>(), inSpellDocumentWithTag: self.tag]
            };
            guesses
                .map(|items| items.iter().map(|item| item.to_string()).collect())
                .unwrap_or_default()
        }
    }
    /// Native results as checks over `text`, on whichever thread received them.
    fn convert(text: &str, results: Vec<Retained<NSTextCheckingResult>>) -> Vec<TextCheck> {
        let mut checks = Vec::new();
        for result in results {
            let kind = match result.resultType() {
                NSTextCheckingType::Spelling => Kind::Spelling,
                NSTextCheckingType::Grammar => Kind::Grammar,
                NSTextCheckingType::Quote => Kind::Quote,
                NSTextCheckingType::Dash => Kind::Dash,
                NSTextCheckingType::Replacement => Kind::Replacement,
                NSTextCheckingType::Correction => Kind::Correction,
                NSTextCheckingType::Link => Kind::Link,
                NSTextCheckingType::Date => Kind::Date,
                NSTextCheckingType::Address => Kind::Address,
                NSTextCheckingType::PhoneNumber => Kind::Phone,
                NSTextCheckingType::TransitInformation => Kind::Transit,
                _ => continue,
            };
            let native_range = result.range();
            let Some(range) =
                utf16_to_scalar_range(text, native_range.location, native_range.length)
            else {
                continue;
            };
            if kind == Kind::Grammar {
                if let Some(details) = result.grammarDetails() {
                    for detail in details.iter() {
                        // SAFETY: AppKit's grammar details use NSGrammarRange for an NSValue range.
                        let local: NSRange = unsafe { detail.objectForKey(NSGrammarRange) }
                            .map(|value| {
                                // SAFETY: NSGrammarRange's documented NSValue provides rangeValue.
                                unsafe { msg_send![&*value, rangeValue] }
                            })
                            .unwrap_or(NSRange::new(0, native_range.length));
                        let Some(end) = local.location.checked_add(local.length) else {
                            continue;
                        };
                        if end > native_range.length {
                            continue;
                        }
                        let Some(start) = native_range.location.checked_add(local.location) else {
                            continue;
                        };
                        let Some(range) = utf16_to_scalar_range(text, start, local.length) else {
                            continue;
                        };
                        // SAFETY: this AppKit grammar key contains an optional NSArray<NSString>.
                        let suggestions: Option<Retained<NSArray<NSString>>> =
                            unsafe { msg_send![&*detail, objectForKey: NSGrammarCorrections] };
                        // SAFETY: this AppKit grammar key contains an optional NSString.
                        let description: Option<Retained<NSString>> =
                            unsafe { msg_send![&*detail, objectForKey: NSGrammarUserDescription] };
                        checks.push(TextCheck {
                            kind,
                            range,
                            replacement: None,
                            url: None,
                            suggestions: suggestions
                                .map(|items| items.iter().map(|item| item.to_string()).collect())
                                .unwrap_or_default(),
                            detail: description.map(|value| value.to_string()),
                        });
                    }
                }
                continue;
            }
            let replacement = matches!(
                kind,
                Kind::Quote | Kind::Dash | Kind::Replacement | Kind::Correction
            )
            .then(|| result.replacementString().map(|value| value.to_string()))
            .flatten();
            let url = (kind == Kind::Link)
                .then(|| {
                    result
                        .URL()
                        .and_then(|url| url.absoluteString())
                        .map(|value| value.to_string())
                })
                .flatten();
            checks.push(TextCheck {
                kind,
                range,
                replacement,
                suggestions: Vec::new(),
                detail: None,
                url,
            });
        }
        checks.sort_by_key(|check| (check.range.start, check.range.end));
        checks
    }

    impl Drop for NativeDocument {
        fn drop(&mut self) {
            if MainThreadMarker::new().is_some() {
                // SAFETY: checker remains retained during Drop; tag belongs to this document.
                unsafe {
                    let _: () = msg_send![&*self.checker, closeSpellDocumentWithTag: self.tag];
                }
            }
        }
    }
}

/// Cocoa counts UTF-16 units; Markraft maps reading text by Unicode scalars.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn utf16_to_scalar_range(text: &str, location: usize, length: usize) -> Option<Range<usize>> {
    let end = location.checked_add(length)?;
    let (mut from, mut to, mut utf16) = ((location == 0).then_some(0), (end == 0).then_some(0), 0);
    for (index, character) in text.chars().enumerate() {
        utf16 += character.len_utf16();
        if utf16 == location {
            from = Some(index + 1);
        }
        if utf16 == end {
            to = Some(index + 1);
        }
    }
    Some(from?..to?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use gpui::{TestAppContext, prelude::*};
    use markraft_commonmark::{
        SourceDocument, commonmark_doc_type_names, commonmark_extensions, commonmark_schema,
    };
    use markraft_core::{
        Selection, TransactionSpec, commands, composition, history, kind::DocTypes,
    };
    use markraft_gpui::Setup;
    use std::sync::Arc;

    fn editor(text: &str, cx: &mut TestAppContext) -> Entity<EditorView> {
        let schema = commonmark_schema();
        let document = SourceDocument::parse(&schema, text).unwrap();
        cx.new(|cx| {
            EditorView::new(
                Setup::new(schema.clone())
                    .types(DocTypes::from_schema_names(
                        &schema,
                        &commonmark_doc_type_names(),
                    ))
                    .kind(Arc::new(crate::markdown::Markdown {
                        schema: schema.clone(),
                    }))
                    .extensions(commonmark_extensions(&schema))
                    .doc(document.document().clone()),
                cx,
            )
        })
    }
    fn check(kind: Kind, range: Range<usize>) -> TextCheck {
        TextCheck {
            kind,
            range,
            replacement: None,
            suggestions: Vec::new(),
            detail: None,
            url: None,
        }
    }
    #[test]
    fn native_utf16_ranges_are_unicode_scalars_and_reject_surrogate_splits() {
        let text = "a😀中文e\u{301}";
        assert_eq!(utf16_to_scalar_range(text, 1, 2), Some(1..2));
        assert_eq!(utf16_to_scalar_range(text, 3, 2), Some(2..4));
        assert_eq!(utf16_to_scalar_range(text, 5, 2), Some(4..6));
        assert_eq!(utf16_to_scalar_range(text, 7, 0), Some(6..6));
        assert_eq!(utf16_to_scalar_range(text, 2, 1), None);
        assert_eq!(utf16_to_scalar_range(text, 1, 1), None);
        assert_eq!(utf16_to_scalar_range(text, usize::MAX, 1), None);
    }
    #[test]
    fn chunks_keep_complete_unicode_words_and_skip_overlong_tokens() {
        let text = "😀 hello 中文 e\u{301} last";
        let chunks = checking_chunks(text, 7);
        assert!(chunks.iter().all(|range| range.len() <= 7));
        let rebuilt = chunks
            .into_iter()
            .map(|range| {
                text.chars()
                    .skip(range.start)
                    .take(range.len())
                    .collect::<String>()
            })
            .collect::<String>();
        assert_eq!(rebuilt, text);
        assert_eq!(
            checking_chunks("ok exceptionallylong end", 6),
            vec![0..3, 20..24]
        );
    }
    #[test]
    fn typing_boundaries_and_urls_cannot_trigger_unrelated_edits() {
        let boundary = TypingBoundary {
            caret: 12,
            completed_end: Some(11),
        };
        assert!(boundary.accepts(Kind::Quote, 12));
        assert!(boundary.accepts(Kind::Replacement, 11));
        assert!(!boundary.accepts(Kind::Correction, 10));
        assert!(!boundary.accepts(Kind::Replacement, 12));
        let mut checks = vec![
            check(Kind::Spelling, 0..4),
            check(Kind::Link, 5..25),
            check(Kind::Replacement, 8..12),
        ];
        exclude_url_edits(&mut checks, false);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].range, 0..4);
    }
    /// Flags every "wrng" as a misspelling and counts the texts it was asked about.
    pub(crate) struct Fake {
        pub(crate) requests: Rc<Cell<usize>>,
    }
    impl Checker for Fake {
        fn request(&self, text: &str, _: Checking) -> oneshot::Receiver<Vec<TextCheck>> {
            self.requests.set(self.requests.get() + 1);
            let chars: Vec<char> = text.chars().collect();
            let found = (0..chars.len().saturating_sub(3))
                .filter(|&at| chars[at..at + 4].iter().collect::<String>() == "wrng")
                .map(|at| check(Kind::Spelling, at..at + 4))
                .collect();
            let (reply, receiver) = oneshot::channel();
            let _ = reply.send(found);
            receiver
        }
        fn check(&self, _: &str, _: Checking) -> Vec<TextCheck> {
            Vec::new()
        }
        fn ignore(&self, _: &str) {}
        fn guesses(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }
    pub(crate) fn finish(task: Task<Scan>, cx: &mut TestAppContext) -> Scan {
        let outcome = Rc::new(Cell::new(None));
        let slot = outcome.clone();
        cx.update(|cx| cx.spawn(async move |_| slot.set(Some(task.await))).detach());
        for _ in 0..10_000 {
            cx.run_until_parked();
            if let Some(scan) = outcome.get() {
                return scan;
            }
            cx.executor().advance_clock(Duration::from_millis(5));
        }
        panic!("the scan never finished");
    }
    #[gpui::test]
    fn scans_ask_only_about_changed_lines_and_kept_checks_follow_their_lines(
        cx: &mut TestAppContext,
    ) {
        let editor = editor("wrng one\n\nsecond line\n\nthird wrng\n", cx);
        let requests = Rc::new(Cell::new(0));
        let mut checker = DocumentChecking::with_checker(Rc::new(Fake {
            requests: requests.clone(),
        }));
        let scan = cx.update(|cx| checker.refresh(&editor, Checking::default(), cx));
        assert_eq!(finish(scan, cx), Scan::Complete);
        assert_eq!(requests.get(), 3);
        // Typing in the middle line changes only that line.
        editor.update(cx, |editor, cx| {
            let second = editor.projection().line(1).unwrap().from();
            editor.dispatch(
                [TransactionSpec::new().selection(Selection::cursor(second))],
                cx,
            );
            assert!(editor.dispatch([commands::insert_text("x")(editor.state()).unwrap()], cx));
        });
        let scan = cx.update(|cx| checker.refresh(&editor, Checking::default(), cx));
        assert_eq!(finish(scan, cx), Scan::Complete);
        assert_eq!(requests.get(), 4);
        editor.update(cx, |editor, _| {
            let found = checker.results(editor);
            assert_eq!(found.len(), 2);
            for check in found {
                let request = editor.context_source_range_snapshot(check.range).unwrap();
                assert_eq!(editor.context_text(&request).unwrap().text, "wrng");
            }
        });
        // Ignore Word may change any line's checks, so every line is asked about again.
        checker.ignore("wrng");
        let scan = cx.update(|cx| checker.check_all(&editor, Checking::default(), cx));
        assert_eq!(finish(scan, cx), Scan::Complete);
        assert_eq!(requests.get(), 7);
    }
    #[gpui::test]
    fn large_notes_check_near_the_caret_until_check_document_asks_for_everything(
        cx: &mut TestAppContext,
    ) {
        let paragraph = "plain words ".repeat(90);
        let lines = AUTOMATIC_LIMIT / paragraph.len() + NEARBY_LINES;
        let text = vec![paragraph.as_str(); lines].join("\n\n");
        let editor = editor(&text, cx);
        let requests = Rc::new(Cell::new(0));
        let mut checker = DocumentChecking::with_checker(Rc::new(Fake {
            requests: requests.clone(),
        }));
        let scan = cx.update(|cx| checker.refresh(&editor, Checking::default(), cx));
        assert_eq!(finish(scan, cx), Scan::Complete);
        assert_eq!(requests.get(), NEARBY_LINES + 1);
        let scan = cx.update(|cx| checker.check_all(&editor, Checking::default(), cx));
        assert_eq!(finish(scan, cx), Scan::Complete);
        assert_eq!(requests.get(), lines);
    }
    #[gpui::test]
    fn a_superseded_scan_never_reports_completion(cx: &mut TestAppContext) {
        let editor = editor("Helo wrld", cx);
        let mut checker = DocumentChecking::new();
        let first = cx.update(|cx| checker.refresh(&editor, Checking::default(), cx));
        let second = cx.update(|cx| checker.refresh(&editor, Checking::default(), cx));
        drop(second);
        cx.run_until_parked();
        assert_eq!(
            futures::FutureExt::now_or_never(first),
            Some(Scan::Superseded)
        );
    }
    #[gpui::test]
    fn displayed_prose_mapping_excludes_syntax_code_and_active_ime(cx: &mut TestAppContext) {
        let editor = editor(
            "**wrng** 😀 `codde`\n\n```rust\nfn misspelled() {}\n```",
            cx,
        );
        editor.update(cx, |editor, cx| {
            let request = editor.context_document_snapshot();
            let reading = editor.context_text(&request).unwrap().text;
            assert!(reading.starts_with("wrng 😀 codde"));
            assert!(!reading.contains("**"));
            let masked = prose_reading(editor, &request).unwrap();
            assert!(masked.starts_with("wrng 😀      "));
            assert!(!masked.contains("codde"));
            assert!(!masked.contains("misspelled"));
            assert_eq!(masked.chars().count(), reading.chars().count());
            let found = map_prose_checks(
                editor,
                &request,
                vec![check(Kind::Spelling, 0..4), check(Kind::Spelling, 7..12)],
            )
            .unwrap();
            assert_eq!(found.len(), 1);
            let selected = editor
                .context_source_range_snapshot(found[0].range.clone())
                .unwrap();
            assert_eq!(editor.context_text(&selected).unwrap().text, "wrng");
            let composition = composition::update_composition(editor.state(), "候補", 2).unwrap();
            assert!(editor.dispatch([composition], cx));
            assert!(editor.is_composing());
            assert!(prose_reading(editor, &request).is_none());
            assert!(
                map_prose_checks(editor, &request, vec![check(Kind::Spelling, 0..4)]).is_none()
            );
        });
    }
    #[gpui::test]
    fn substitution_preserves_delimiter_caret_and_has_one_undo(cx: &mut TestAppContext) {
        let editor = editor("omw", cx);
        editor.update(cx, |editor, cx| {
            let request = editor.context_document_snapshot();
            let mapped = editor.context_text_mapping(&request).unwrap();
            let end = mapped.source.last().unwrap().as_ref().unwrap().end;
            editor.dispatch(
                [TransactionSpec::new().selection(Selection::text(end, end))],
                cx,
            );
            assert_eq!(editor.head(), end);
            assert!(editor.dispatch([commands::insert_text(" ")(editor.state()).unwrap()], cx));
            let (request, boundary) = typing_context(editor).unwrap();
            let mut replacement = check(Kind::Replacement, 0..3);
            replacement.replacement = Some("on my way".into());
            assert!(apply_substitution(
                editor,
                &request,
                boundary,
                vec![replacement],
                cx
            ));
            assert_eq!(
                editor
                    .context_text(&editor.context_document_snapshot())
                    .unwrap()
                    .text,
                "on my way "
            );
            assert!(editor.dispatch(
                [commands::insert_text("there")(editor.state()).unwrap()],
                cx
            ));
            assert_eq!(
                editor
                    .context_text(&editor.context_document_snapshot())
                    .unwrap()
                    .text,
                "on my way there"
            );
            assert!(editor.dispatch([history::undo(editor.state()).unwrap()], cx));
            assert!(editor.dispatch([history::undo(editor.state()).unwrap()], cx));
            assert_eq!(
                editor
                    .context_text(&editor.context_document_snapshot())
                    .unwrap()
                    .text,
                "omw "
            );
        });
    }
}
