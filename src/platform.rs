use crate::notebook::NoteRecord;
use gpui::Window;

/// What closing does with the note, said where the choice is made.
const CLOSE_BUTTONS: [&str; 4] = ["Archive", "Cancel", "Export and Archive…", "Delete"];
const CLOSE_DETAIL: &str = "Archive keeps this note in File ▸ Archived Notes. Delete moves it to the Trash, where you can restore it. Export and Archive saves a Markdown file first, then archives the note.";

/// Explicit app choices; native response numbers are confined to this platform boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloseChoice {
    Archive,
    #[default]
    Cancel,
    ExportAndArchive,
    Trash,
}
impl CloseChoice {
    fn from_response(response: Option<usize>) -> Self {
        match response {
            Some(0) => Self::Archive,
            Some(2) => Self::ExportAndArchive,
            Some(3) => Self::Trash,
            _ => Self::Cancel,
        }
    }
}
pub fn close_prompt(
    title: &str,
    window: &mut Window,
    cx: &mut gpui::App,
) -> impl std::future::Future<Output = CloseChoice> + use<> {
    let response = close_response(title, window, cx);
    async move { CloseChoice::from_response(response.await.ok()) }
}

fn close_response(
    title: &str,
    window: &mut Window,
    cx: &mut gpui::App,
) -> futures::channel::oneshot::Receiver<usize> {
    let message = format!("Close “{title}”?");
    #[cfg(not(target_os = "macos"))]
    {
        window.prompt(
            gpui::PromptLevel::Info,
            &message,
            Some(CLOSE_DETAIL),
            &CLOSE_BUTTONS,
            cx,
        )
    }
    #[cfg(target_os = "macos")]
    {
        // Mirror Swift's compact app-modal NSAlert, including Return/Cancel key equivalents.
        // GPUI's generic prompt focuses the final non-cancel button, which would focus Delete.
        let _ = (window, cx);
        let (sender, receiver) = futures::channel::oneshot::channel();
        // SAFETY: `show` takes back ownership of this box exactly once on the main queue.
        unsafe {
            dispatch2::DispatchQueue::main()
                .exec_async_f(Box::into_raw(Box::new((sender, message))).cast(), show);
        }
        return receiver;
    }
    #[cfg(target_os = "macos")]
    extern "C" fn show(pointer: *mut std::ffi::c_void) {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSAlert;
        use objc2_foundation::NSString;
        // SAFETY: `close_prompt` boxed this request and handed it over exactly once.
        let (sender, message) = *unsafe {
            Box::from_raw(pointer.cast::<(futures::channel::oneshot::Sender<usize>, String)>())
        };
        let alert = NSAlert::new(MainThreadMarker::new().expect("main queue"));
        alert.setMessageText(&NSString::from_str(&message));
        alert.setInformativeText(&NSString::from_str(CLOSE_DETAIL));
        for title in CLOSE_BUTTONS {
            let button = alert.addButtonWithTitle(&NSString::from_str(title));
            if title == "Cancel" {
                button.setKeyEquivalent(&NSString::from_str("\u{1b}"));
            }
            if title == "Delete" {
                // Red, and never answered by Return.
                button.setKeyEquivalent(&NSString::from_str(""));
                button.setHasDestructiveAction(true);
            }
        }
        let response = alert.runModal();
        let choice = if (1000..=1003).contains(&response) {
            (response - 1000) as usize
        } else {
            1
        };
        let _ = sender.send(choice);
    }
}

#[cfg_attr(test, allow(dead_code))]
/// An application-modal alert with one OK button, for messages with no note window to
/// show them. Resolves once it is dismissed.
pub fn alert(message: &str) -> futures::channel::oneshot::Receiver<()> {
    let (sender, receiver) = futures::channel::oneshot::channel();
    #[cfg(target_os = "macos")]
    {
        let request = Box::new((sender, message.to_owned()));
        // SAFETY: `show_alert` takes back ownership of this box exactly once on the main queue.
        unsafe {
            dispatch2::DispatchQueue::main()
                .exec_async_f(Box::into_raw(request).cast(), show_alert);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("{message}");
        let _ = sender.send(());
    }
    receiver
}
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
extern "C" fn show_alert(pointer: *mut std::ffi::c_void) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSAlert;
    use objc2_foundation::NSString;
    // SAFETY: `alert` boxed this request and handed it over exactly once.
    let (sender, message) = *unsafe {
        Box::from_raw(pointer.cast::<(futures::channel::oneshot::Sender<()>, String)>())
    };
    let (title, detail) = message.split_once("\n\n").unwrap_or((&message, ""));
    let alert = NSAlert::new(MainThreadMarker::new().expect("main queue"));
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(&NSString::from_str("OK"));
    alert.runModal();
    let _ = sender.send(());
}
pub fn content_opacity(style: &crate::appearance::NoteStyle) -> f32 {
    // AppKit applies opacity to the entire native window. Other platforms use content alpha.
    if !cfg!(target_os = "macos") && style.translucent {
        0.75
    } else {
        1.
    }
}

pub fn reduce_transparency() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn reduce_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
/// Whether a window change that asks to animate does: Reduce Motion always wins.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn glides(animate: bool, reduce_motion: bool) -> bool {
    animate && !reduce_motion
}

/// Whether the primary mouse button is down, which is how a window being dragged or resized
/// by hand differs from one the app or the system is moving. Only AppKit is asked: elsewhere
/// the answer is no, so a zoom is kept until Zoom is used again.
pub fn primary_button_down() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSEvent::pressedMouseButtons() & 1 != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// AppKit's standard About panel: the app's name, icon, version and copyright.
pub fn show_about() {
    #[cfg(target_os = "macos")]
    if let Some(marker) = objc2::MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(marker);
        // SAFETY: `None` asks AppKit for its default panel options.
        unsafe {
            app.orderFrontStandardAboutPanelWithOptions(&objc2_foundation::NSDictionary::new())
        };
    }
}

pub fn increase_contrast() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Say `message` to assistive technology: VoiceOver reads it aloud at high priority without
/// moving focus. For a failure or status that appears as text and would otherwise be silent.
/// Does nothing on other platforms.
pub fn announce(message: &str) {
    #[cfg(test)]
    ANNOUNCED.with(|spoken| spoken.borrow_mut().push(message.to_owned()));
    #[cfg(all(target_os = "macos", not(test)))]
    post_announcement(message.to_owned());
    #[cfg(all(not(target_os = "macos"), not(test)))]
    let _ = message;
}

#[cfg(test)]
thread_local! {
    static ANNOUNCED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
/// What `announce` was asked to say since the last call.
#[cfg(test)]
pub fn take_announcements() -> Vec<String> {
    ANNOUNCED.with(|spoken| std::mem::take(&mut *spoken.borrow_mut()))
}

/// The last message `announce` was given for one surface (a note's error, a panel's status
/// line), so a banner that is redrawn many times is spoken once. An empty message clears it,
/// so the same failure appearing again later is spoken again.
#[derive(Default)]
pub struct Announced(String);
impl Announced {
    pub fn say(&mut self, message: &str) {
        if message == self.0 {
            return;
        }
        self.0 = message.to_owned();
        if !message.is_empty() {
            announce(message);
        }
    }
}

#[cfg(all(target_os = "macos", not(test)))]
fn post_announcement(message: String) {
    use objc2::{MainThreadMarker, runtime::AnyObject};
    use objc2_app_kit::{
        NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification,
        NSAccessibilityPostNotificationWithUserInfo, NSAccessibilityPriorityKey,
        NSAccessibilityPriorityLevel, NSApplication,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSString};
    // AppKit's accessibility notifications belong to the main thread.
    let Some(main) = MainThreadMarker::new() else {
        dispatch2::DispatchQueue::main().exec_async(move || post_announcement(message));
        return;
    };
    let app = NSApplication::sharedApplication(main);
    let text = NSString::from_str(&message);
    let priority = NSNumber::new_isize(NSAccessibilityPriorityLevel::High.0);
    // SAFETY: both keys are AppKit constants, valid for the life of the process, and the
    // values are an NSString and an NSNumber, the types the announcement notification takes;
    // the call happens on the main thread with the shared application as its element.
    unsafe {
        let info = NSDictionary::from_slices(
            &[NSAccessibilityAnnouncementKey, NSAccessibilityPriorityKey],
            &[&*text as &AnyObject, &*priority as &AnyObject],
        );
        NSAccessibilityPostNotificationWithUserInfo(
            &app,
            NSAccessibilityAnnouncementRequestedNotification,
            Some(&info),
        );
    }
}

/// The user's accent colour (System Settings › Appearance), in sRGB, for the panels' default
/// button, selection, slider and focus rings. `None` where the system has none, and in tests,
/// which then use the fixed blue.
pub fn accent_color() -> Option<gpui::Rgba> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let color = objc2_app_kit::NSColor::controlAccentColor()
            .colorUsingColorSpace(&objc2_app_kit::NSColorSpace::sRGBColorSpace())?;
        Some(gpui::Rgba {
            r: color.redComponent() as f32,
            g: color.greenComponent() as f32,
            b: color.blueComponent() as f32,
            a: 1.,
        })
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        None
    }
}

/// One item each time a system appearance setting changes: Reduce Transparency, Increase
/// Contrast and the other display options (the workspace's notification), and the accent and
/// highlight colours. macOS posts these for every app at once, whichever window is key.
/// Never yields on other platforms.
pub fn appearance_changes() -> futures::channel::mpsc::UnboundedReceiver<()> {
    let (sender, receiver) = futures::channel::mpsc::unbounded();
    #[cfg(all(target_os = "macos", not(test)))]
    {
        use block2::RcBlock;
        use objc2_app_kit::{
            NSSystemColorsDidChangeNotification, NSWorkspace,
            NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
        };
        use objc2_foundation::NSNotificationCenter;
        let tell = RcBlock::new(move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
            let _ = sender.unbounded_send(());
        });
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let default = NSNotificationCenter::defaultCenter();
        // SAFETY: the names are AppKit constants and no object filter or queue is given; the
        // block only sends on a channel sender, which is `Send`, so it may run on whichever
        // thread posts. The centres retain the observers for the life of the process.
        unsafe {
            workspace.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
                None,
                &tell,
            );
            default.addObserverForName_object_queue_usingBlock(
                Some(NSSystemColorsDidChangeNotification),
                None,
                None,
                &tell,
            );
        }
    }
    #[cfg(any(not(target_os = "macos"), test))]
    drop(sender);
    receiver
}

/// The user's text highlight colour (System Settings › Appearance), in sRGB.
pub fn highlight_color() -> Option<gpui::Rgba> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let color = objc2_app_kit::NSColor::selectedTextBackgroundColor()
            .colorUsingColorSpace(&objc2_app_kit::NSColorSpace::sRGBColorSpace())?;
        Some(gpui::Rgba {
            r: color.redComponent() as f32,
            g: color.greenComponent() as f32,
            b: color.blueComponent() as f32,
            a: 1.,
        })
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        None
    }
}

/// Glass and Sea Glass are always the full material: the transparency setting only changes
/// how much paper the note paints over it (see `appearance::glass_paper_alpha` and
/// `sea_paper_alpha`), so the desktop is never seen unblurred. Reduce Transparency removes
/// the material and the paper turns opaque.
pub fn apply_style(window: &mut Window, note: &NoteRecord) {
    let glass = note.style.frosted();
    let blur = glass && !reduce_transparency();
    window.set_background_appearance(if blur {
        gpui::WindowBackgroundAppearance::Blurred
    } else if glass {
        // Clear outside the rounded corners; the paper itself is opaque.
        gpui::WindowBackgroundAppearance::Transparent
    } else {
        gpui::WindowBackgroundAppearance::Opaque
    });
    #[cfg(target_os = "macos")]
    native_update(
        window,
        note.collapsed,
        note.floating,
        glass,
        None,
        Some((
            glass,
            blur,
            note.style.translucent,
            note.style.paper(),
            note.style.corner_radius(),
            note.style.sea_glass(),
        )),
    );
}

/// Where the notes live. `DAHLIA_DATA_DIR` (or the name it had before the rename,
/// `ORTENSIA_RUST_DATA_DIR`) overrides it for fixtures; otherwise it is the platform's
/// application-data folder, `~/Library/Application Support/Dahlia` on the Mac, moved there
/// once from the folder the app used before it was named Dahlia.
pub fn data_directory() -> std::io::Result<std::path::PathBuf> {
    location(|name| std::env::var_os(name), dirs::data_local_dir())
}
pub const DATA_DIR_VAR: &str = "DAHLIA_DATA_DIR";
pub const LEGACY_DATA_DIR_VAR: &str = "ORTENSIA_RUST_DATA_DIR";
const DATA_FOLDER: &str = "Dahlia";
const LEGACY_DATA_FOLDER: &str = "ortensia-rust";
fn location(
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
    base: Option<std::path::PathBuf>,
) -> std::io::Result<std::path::PathBuf> {
    if let Some(path) = var(DATA_DIR_VAR).or_else(|| var(LEGACY_DATA_DIR_VAR)) {
        return Ok(path.into());
    }
    let base = base.ok_or_else(|| {
        std::io::Error::other("Dahlia can’t find a folder for its notes on this computer.")
    })?;
    migrate(&base.join(LEGACY_DATA_FOLDER), &base.join(DATA_FOLDER))
}

/// Move the notes from `old` to `new` the first time Dahlia runs after the rename, and
/// return `new`.
///
/// Nothing is ever deleted. When `new` already exists it wins and `old` is left alone; when
/// `old` is missing there is nothing to move. Otherwise the folder is renamed within its
/// parent, which is atomic, after checking that no older copy of the app has the library
/// open. If the rename cannot be done (another volume behind a link, say), the library is
/// copied into a staging folder, every file is compared byte for byte, and only then does
/// the staging folder become `new`; `old` stays where it was.
pub fn migrate(
    old: &std::path::Path,
    new: &std::path::Path,
) -> std::io::Result<std::path::PathBuf> {
    migrate_by(old, new, |from, to| std::fs::rename(from, to))
}
fn migrate_by(
    old: &std::path::Path,
    new: &std::path::Path,
    rename: impl Fn(&std::path::Path, &std::path::Path) -> std::io::Result<()>,
) -> std::io::Result<std::path::PathBuf> {
    use std::{fs, io};
    if new.symlink_metadata().is_ok() || old.symlink_metadata().is_err() {
        return Ok(new.to_owned());
    }
    // An older build still running holds this lock; moving its library under it would
    // split the notes between two folders.
    let lock = old.join("library.lock");
    let _held_lock = if lock.exists() {
        let held = fs::OpenOptions::new().read(true).write(true).open(&lock)?;
        held.try_lock()
            .map_err(|_| io::Error::from(io::ErrorKind::WouldBlock))?;
        Some(held)
    } else {
        None
    };
    if rename(old, new).is_ok() {
        log::warn!("Moved notes from {} to {}", old.display(), new.display());
        return Ok(new.to_owned());
    }
    let staging = new.with_file_name(format!("{DATA_FOLDER}.moving-{}", uuid::Uuid::new_v4()));
    let copied = copy_tree(old, &staging).and_then(|()| same_tree(old, &staging));
    match copied {
        Ok(true) => {
            fs::rename(&staging, new)?;
            log::warn!("Copied notes from {} to {}", old.display(), new.display());
            Ok(new.to_owned())
        }
        Ok(false) | Err(_) => {
            // The copy is ours to remove; the original was never touched.
            let _ = fs::remove_dir_all(&staging);
            copied.and(Err(io::Error::other(
                "Dahlia couldn’t move your notes to their new folder. Nothing was changed.",
            )))
        }
    }
}
fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
            std::fs::File::open(&target)?.sync_all()?;
        }
    }
    Ok(())
}
/// Whether two folders hold the same files with the same bytes.
fn same_tree(a: &std::path::Path, b: &std::path::Path) -> std::io::Result<bool> {
    let names = |dir: &std::path::Path| -> std::io::Result<std::collections::BTreeSet<_>> {
        std::fs::read_dir(dir)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect()
    };
    let names_a = names(a)?;
    if names_a != names(b)? {
        return Ok(false);
    }
    for name in names_a {
        let (left, right) = (a.join(&name), b.join(&name));
        let kind = left.symlink_metadata()?.file_type();
        let same = if kind.is_dir() {
            same_tree(&left, &right)?
        } else if kind.is_symlink() {
            std::fs::read_link(&left)? == std::fs::read_link(&right)?
        } else {
            std::fs::read(&left)? == std::fs::read(&right)?
        };
        if !same {
            return Ok(false);
        }
    }
    Ok(true)
}

/// What double-clicking a note's strip does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoubleClickAction {
    Collapse,
    Zoom,
    Minimize,
    Nothing,
}

/// Follow the user's "Double-click a window's title bar to" preference; Stickies' own
/// collapse remains the fallback when the preference is unset or cannot be read.
pub fn double_click_action() -> DoubleClickAction {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSString, NSUserDefaults};
        let defaults = NSUserDefaults::standardUserDefaults();
        let value = defaults.stringForKey(&NSString::from_str("AppleActionOnDoubleClick"));
        double_click_for(value.as_deref().map(|value| value.to_string()).as_deref())
    }
    #[cfg(not(target_os = "macos"))]
    {
        DoubleClickAction::Collapse
    }
}
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn double_click_for(preference: Option<&str>) -> DoubleClickAction {
    match preference {
        Some("Maximize" | "Zoom") => DoubleClickAction::Zoom,
        Some("Minimize") => DoubleClickAction::Minimize,
        Some("None") => DoubleClickAction::Nothing,
        _ => DoubleClickAction::Collapse,
    }
}

pub fn can_toggle_float() -> bool {
    cfg!(target_os = "macos")
}
/// Whether the platform can hide the app and list Services (AppKit only).
pub fn has_app_hiding() -> bool {
    cfg!(target_os = "macos")
}
/// The window system Dahlia runs on: `macOS`, or GPUI's chosen Linux backend (`Wayland`,
/// `X11` or `Headless`), decided the same way GPUI decides it.
pub fn window_system() -> &'static str {
    #[cfg(target_os = "macos")]
    return "macOS";
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    return gpui::guess_compositor();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
    return "Other";
}
/// Whether Window ▸ Arrange By can move note windows on this window system.
///
/// Only AppKit lets Dahlia set a window's position. GPUI exposes no window move on Linux:
/// on X11 the window manager also ignores the requested origin of a new window, and core
/// Wayland gives clients no way to position their own windows at all. Both were checked by
/// running Arrange and reading back the window positions, so the Linux menus leave Arrange
/// out rather than offer a command that only resizes.
pub fn can_arrange() -> bool {
    can_arrange_on(window_system())
}
fn can_arrange_on(system: &str) -> bool {
    system == "macOS"
}
/// What the Recovery window's reveal button says.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else {
    "Show in File Manager"
};
/// An AppKit rectangle: bottom-left origin, global across screens.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
/// A screen's visible area in the coordinates note frames use: global, top-left origin at
/// the primary screen's top-left (see `note_frame`). GPUI's `visible_bounds` is local to
/// each screen, which would put every display at the primary's origin.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn display_frame(visible: ScreenRect, primary: ScreenRect) -> crate::notebook::Frame {
    let top = primary.y + primary.height;
    crate::notebook::Frame {
        x: visible.x as f32,
        y: (top - visible.y - visible.height) as f32,
        width: visible.width as f32,
        height: visible.height as f32,
    }
}
/// Every screen's visible area, primary first; `None` where screens cannot be queried.
pub fn display_frames() -> Option<Vec<crate::notebook::Frame>> {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSScreen;
        let rect = |rect: objc2_foundation::NSRect| ScreenRect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        };
        let screens = NSScreen::screens(MainThreadMarker::new()?);
        // AppKit lists the primary (menu bar) screen first.
        let primary = rect(screens.firstObject()?.frame());
        Some(
            screens
                .iter()
                .map(|screen| display_frame(rect(screen.visibleFrame()), primary))
                .collect(),
        )
    }
    #[cfg(not(target_os = "macos"))]
    None
}
pub fn note_frame(window: &Window) -> crate::notebook::Frame {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSScreen, NSView};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::AppKit(handle) = handle.as_raw()
        {
            // SAFETY: GPUI owns this live view on the main thread.
            // SAFETY: GPUI's AppKit handle is a live NSView borrowed on its UI thread.
            let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            if let Some(native) = view.window()
                && let Some(primary) =
                    NSScreen::screens(MainThreadMarker::new().expect("main thread")).firstObject()
            {
                let frame = native.frame();
                let top = primary.frame().origin.y + primary.frame().size.height;
                return crate::notebook::Frame {
                    x: frame.origin.x as f32,
                    y: (top - frame.origin.y - frame.size.height) as f32,
                    width: frame.size.width as f32,
                    height: frame.size.height as f32,
                };
            }
        }
    }
    let bounds = window.bounds();
    crate::notebook::Frame {
        x: bounds.origin.x.into(),
        y: bounds.origin.y.into(),
        width: bounds.size.width.into(),
        height: bounds.size.height.into(),
    }
}
/// Set the window's frame. `animate` glides there like AppKit's own zoom, unless the user
/// asked for Reduce Motion; elsewhere the window simply resizes.
pub fn place(window: &mut Window, frame: crate::notebook::Frame, animate: bool) {
    #[cfg(not(target_os = "macos"))]
    let _ = animate;
    #[cfg(not(target_os = "macos"))]
    window.resize(gpui::size(gpui::px(frame.width), gpui::px(frame.height)));
    #[cfg(target_os = "macos")]
    {
        use objc2::{MainThreadMarker, rc::Retained};
        use objc2_app_kit::{NSScreen, NSView, NSWindow};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::AppKit(handle) = handle.as_raw()
        {
            // SAFETY: GPUI's AppKit handle is a live NSView borrowed on its UI thread.
            let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            if let Some(native) = view.window() {
                let pointer = Box::into_raw(Box::new((native, frame, animate))).cast();
                // SAFETY: apply takes this boxed retained window/frame exactly once on main.
                unsafe {
                    dispatch2::DispatchQueue::main().exec_async_f(pointer, apply);
                }
            }
        }
        extern "C" fn apply(pointer: *mut std::ffi::c_void) {
            // SAFETY: place transferred this exact Box to this one main-queue callback.
            let (native, frame, animate) = unsafe {
                *Box::from_raw(
                    pointer.cast::<(Retained<NSWindow>, crate::notebook::Frame, bool)>(),
                )
            };
            let marker = MainThreadMarker::new().expect("main queue");
            if let Some(primary) = NSScreen::screens(marker).firstObject() {
                let top = primary.frame().origin.y + primary.frame().size.height;
                let rect = objc2_foundation::NSRect::new(
                    objc2_foundation::NSPoint::new(
                        frame.x as f64,
                        top - frame.y as f64 - frame.height as f64,
                    ),
                    objc2_foundation::NSSize::new(frame.width as f64, frame.height as f64),
                );
                native.setFrame_display_animate(rect, true, glides(animate, reduce_motion()));
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn configure(
    _window: &Window,
    _collapsed: bool,
    _floating: bool,
    _glass: bool,
    _size: Option<(f32, f32)>,
) {
}

#[cfg(target_os = "macos")]
pub fn configure(
    window: &Window,
    collapsed: bool,
    floating: bool,
    glass: bool,
    size: Option<(f32, f32)>,
) {
    native_update(window, collapsed, floating, glass, size, None);
}

/// GPUI renders sRGB values into a `CAMetalLayer` with no colour space, which Core Animation
/// sends to the display unmatched, so on a Display P3 screen every colour came out more
/// saturated than the same sRGB value in Stickies, whose Yellow strip `#FEEA3D` is captured
/// as Display P3 `#FBEB60` (`evidence/stickies-collapsed.png`). Tag GPUI's layers as sRGB
/// so Core Animation colour-matches them like any other sRGB content.
#[cfg(target_os = "macos")]
fn match_srgb(view: &objc2_app_kit::NSView) {
    use objc2_core_graphics::{CGColorSpace, kCGColorSpaceSRGB};
    use objc2_quartz_core::CAMetalLayer;
    if let Some(layer) = view.layer()
        && let Some(metal) = layer.downcast_ref::<CAMetalLayer>()
        && metal.colorspace().is_none()
    {
        // SAFETY: Apple's exported colour-space name constant is a permanent CFString.
        let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
        metal.setColorspace(srgb.as_deref());
    }
    for child in view.subviews() {
        match_srgb(&child);
    }
}

/// NSFloatingWindowLevel, where Float on Top notes sit; a normal window is level 0.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const FLOATING_LEVEL: isize = 3;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn note_level(floating: bool) -> isize {
    if floating { FLOATING_LEVEL } else { 0 }
}
/// The level for Find, Settings and Recovery: above Float on Top notes while any floats, so
/// a panel that serves the notes is never hidden under one, and normal otherwise, so it does
/// not float over other apps for no reason.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn panel_level(notes_float: bool) -> isize {
    if notes_float { FLOATING_LEVEL + 1 } else { 0 }
}
/// Put a panel window at the level that keeps it above any floating note.
pub fn set_panel_level(window: &Window, notes_float: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSView;
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::AppKit(handle) = handle.as_raw()
        {
            // SAFETY: GPUI's AppKit handle is a live NSView borrowed on its UI thread.
            let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            if let Some(native) = view.window() {
                native.setLevel(panel_level(notes_float));
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, notes_float);
}

#[cfg(target_os = "macos")]
fn native_update(
    window: &Window,
    collapsed: bool,
    floating: bool,
    glass: bool,
    size: Option<(f32, f32)>,
    surface: Option<(bool, bool, bool, u32, f32, bool)>,
) {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSAppearanceCustomization, NSView, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    struct Update {
        window: Retained<NSWindow>,
        collapsed: bool,
        floating: bool,
        glass: bool,
        size: Option<(f32, f32)>,
        surface: Option<(bool, bool, bool, u32, f32, bool)>,
    }
    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::AppKit(handle) = handle.as_raw()
    {
        // SAFETY: GPUI gives a live NSView on the main thread; its window is retained until the queue runs.
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        if let Some(native) = view.window() {
            let update = Box::new(Update {
                window: native,
                collapsed,
                floating,
                glass,
                size,
                surface,
            });
            // SAFETY: apply owns this boxed Update exactly once; it retains the native window.
            unsafe {
                dispatch2::DispatchQueue::main().exec_async_f(Box::into_raw(update).cast(), apply);
            }
        }
    }
    extern "C" fn apply(pointer: *mut std::ffi::c_void) {
        // SAFETY: this boxed update is passed exactly once through the main queue.
        let update = unsafe { Box::from_raw(pointer.cast::<Update>()) };
        let native = &update.window;
        let old = native.frame();
        // Installed Stickies 10.3 archives style mask 14 in SNDocument.nib and
        // removes Resizable (8) while collapsed. Reuse AppKit's frame renderer,
        // including its backing-scale and active/inactive behavior.
        let mut mask = if update.glass {
            NSWindowStyleMask::empty()
        } else {
            NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable
        };
        if !update.collapsed {
            mask |= NSWindowStyleMask::Resizable;
        }
        crate::macos_input::set_style_mask(native, mask);
        native.setMinSize(NSSize::new(230., if update.collapsed { 12. } else { 160. }));
        native.setMaxSize(NSSize::new(
            crate::notebook::MAX_NOTE_SIZE as f64,
            if update.collapsed {
                12.
            } else {
                crate::notebook::MAX_NOTE_SIZE as f64
            },
        ));
        native.setLevel(note_level(update.floating));
        if let Some((glass, blur, translucent, body, radius, sea)) = update.surface {
            let background = if glass {
                objc2_app_kit::NSColor::clearColor()
            } else {
                objc2_app_kit::NSColor::colorWithSRGBRed_green_blue_alpha(
                    ((body >> 16) & 255) as f64 / 255.,
                    ((body >> 8) & 255) as f64 / 255.,
                    (body & 255) as f64 / 255.,
                    1.,
                )
            };
            // Stickies setSpineColor:stickyColor: also sets NSWindow.backgroundColor.
            native.setBackgroundColor(Some(&background));
            native.setAlphaValue(if translucent { 0.75 } else { 1. });
            // SAFETY: Apple's exported appearance-name constant is a permanent NSString.
            if let Some(aqua) = objc2_app_kit::NSAppearance::appearanceNamed(unsafe {
                objc2_app_kit::NSAppearanceNameAqua
            }) {
                native.setAppearance(Some(&aqua));
            }
            if let Some(content) = native.contentView() {
                match_srgb(&content);
                content.setWantsLayer(true);
                if let Some(layer) = content.layer() {
                    layer.setCornerRadius(if glass { f64::from(radius) } else { 0. });
                    layer.setMasksToBounds(true);
                }
                // AppKit draws the shadow from the window's alpha, so recompute it once the
                // corners change. A Glass note needs it to lift off a pale desktop.
                native.setHasShadow(true);
                native.invalidateShadow();
                for child in content.subviews() {
                    if let Some(material) =
                        child.downcast_ref::<objc2_app_kit::NSVisualEffectView>()
                    {
                        use objc2_app_kit::{
                            NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
                        };
                        // A light material, made for dark text in the light appearance
                        // notes keep, at full strength whenever it shows. Sea Glass takes
                        // the softer under-window material beneath its pastel.
                        material.setMaterial(if sea {
                            NSVisualEffectMaterial::UnderWindowBackground
                        } else {
                            NSVisualEffectMaterial::Popover
                        });
                        material.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                        material.setState(NSVisualEffectState::Active);
                        material.setMaskImage(None);
                        material.setHidden(!blur);
                    }
                }
            }
        }
        if let Some((width, height)) = update.size {
            let frame = NSRect::new(
                NSPoint::new(old.origin.x, old.origin.y + old.size.height - height as f64),
                NSSize::new(width as f64, height as f64),
            );
            native.setFrame_display(frame, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Announced, ScreenRect, display_frame, glides, take_announcements};

    #[test]
    fn a_requested_animation_yields_to_reduce_motion() {
        assert!(glides(true, false));
        assert!(!glides(true, true));
        assert!(!glides(false, false));
        assert!(!glides(false, true));
    }

    #[test]
    fn panels_sit_above_floating_notes_only_while_notes_float() {
        use super::{FLOATING_LEVEL, note_level, panel_level};
        assert_eq!(note_level(true), FLOATING_LEVEL);
        assert_eq!(note_level(false), 0);
        assert!(panel_level(true) > note_level(true));
        assert_eq!(panel_level(false), note_level(false));
    }

    #[test]
    fn a_message_is_announced_once_until_it_clears_or_changes() {
        take_announcements();
        let mut announced = Announced::default();
        for message in ["", "Couldn’t restore.", "Couldn’t restore.", "", "Couldn’t restore.", "Done."] {
            announced.say(message);
        }
        assert_eq!(
            take_announcements(),
            ["Couldn’t restore.", "Couldn’t restore.", "Done."]
        );
    }

    mod data_folder {
        use super::super::{copy_tree, location, migrate, migrate_by, same_tree};
        use std::{fs, path::PathBuf};
        fn scratch(label: &str) -> tempfile::TempDir {
            tempfile::Builder::new()
                .prefix(&format!("dahlia-move-{label}-"))
                .tempdir()
                .unwrap()
        }
        fn library(root: &std::path::Path) {
            fs::create_dir_all(root.join("notes/a")).unwrap();
            fs::write(root.join("index.json"), b"{\"version\":1}").unwrap();
            fs::write(root.join("notes/a/note.md"), "# Weekend plan\n").unwrap();
            fs::write(root.join("library.lock"), b"").unwrap();
        }

        #[test]
        fn notes_move_once_to_the_dahlia_folder_with_every_byte() {
            let _base_fixture = scratch("once");
            let base = _base_fixture.path().to_path_buf();
            let (old, new) = (base.join("ortensia-rust"), base.join("Dahlia"));
            library(&old);
            let _reference_fixture = scratch("reference");
            let reference = _reference_fixture.path().to_path_buf();
            copy_tree(&old, &reference.join("copy")).unwrap();
            assert_eq!(location(|_| None, Some(base.clone())).unwrap(), new);
            assert!(same_tree(&reference.join("copy"), &new).unwrap());
            assert!(!old.exists(), "moved, not duplicated");
            // A second launch finds the new folder and changes nothing.
            fs::write(new.join("notes/a/note.md"), "edited").unwrap();
            assert_eq!(location(|_| None, Some(base.clone())).unwrap(), new);
            assert_eq!(
                fs::read_to_string(new.join("notes/a/note.md")).unwrap(),
                "edited"
            );
        }

        #[test]
        fn an_existing_dahlia_folder_wins_and_the_old_one_is_left_alone() {
            let _base_fixture = scratch("both");
            let base = _base_fixture.path().to_path_buf();
            let (old, new) = (base.join("ortensia-rust"), base.join("Dahlia"));
            library(&old);
            fs::create_dir_all(&new).unwrap();
            assert_eq!(migrate(&old, &new).unwrap(), new);
            assert!(old.join("notes/a/note.md").exists());
            assert!(!new.join("index.json").exists());
        }

        #[test]
        fn a_library_open_in_an_older_build_is_not_moved() {
            let _base_fixture = scratch("locked");
            let base = _base_fixture.path().to_path_buf();
            let (old, new) = (base.join("ortensia-rust"), base.join("Dahlia"));
            library(&old);
            let held = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(old.join("library.lock"))
                .unwrap();
            held.lock().unwrap();
            let error = migrate(&old, &new).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
            assert_eq!(
                crate::copy::plain(&error),
                "Your notes are open in another copy of Dahlia. Quit that copy, then try again."
            );
            assert!(old.join("notes/a/note.md").exists() && !new.exists());
            drop(held);
            assert_eq!(migrate(&old, &new).unwrap(), new);
        }

        #[test]
        fn migration_holds_the_legacy_lock_until_the_move_finishes() {
            let base = tempfile::tempdir().unwrap();
            let (old, new) = (
                base.path().join("ortensia-rust"),
                base.path().join("Dahlia"),
            );
            library(&old);
            migrate_by(&old, &new, |from, to| {
                let competitor = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(from.join("library.lock"))?;
                assert!(matches!(
                    competitor.try_lock(),
                    Err(std::fs::TryLockError::WouldBlock)
                ));
                fs::rename(from, to)
            })
            .unwrap();
            assert!(new.join("notes/a/note.md").exists());
        }

        #[test]
        fn a_fresh_install_and_the_fixture_variables_never_move_anything() {
            let _base_fixture = scratch("fresh");
            let base = _base_fixture.path().to_path_buf();
            assert_eq!(
                location(|_| None, Some(base.clone())).unwrap(),
                base.join("Dahlia")
            );
            assert!(!base.join("Dahlia").exists(), "Notebook::open creates it");
            library(&base.join("ortensia-rust"));
            let var =
                |name: &str| (name == "ORTENSIA_RUST_DATA_DIR").then(|| "/fixture/old".into());
            assert_eq!(
                location(var, Some(base.clone())).unwrap(),
                PathBuf::from("/fixture/old")
            );
            let both = |name: &str| match name {
                "DAHLIA_DATA_DIR" => Some("/fixture/new".into()),
                _ => Some("/fixture/old".into()),
            };
            assert_eq!(
                location(both, Some(base.clone())).unwrap(),
                PathBuf::from("/fixture/new")
            );
            assert!(base.join("ortensia-rust/index.json").exists(), "untouched");
        }

        #[test]
        fn when_the_folder_cannot_be_renamed_a_verified_copy_takes_its_place() {
            let _base_fixture = scratch("copy");
            let base = _base_fixture.path().to_path_buf();
            let (old, new) = (base.join("ortensia-rust"), base.join("Dahlia"));
            library(&old);
            let refuse = |_: &std::path::Path, _: &std::path::Path| {
                Err(std::io::Error::from(std::io::ErrorKind::CrossesDevices))
            };
            assert_eq!(migrate_by(&old, &new, refuse).unwrap(), new);
            assert!(same_tree(&old, &new).unwrap());
            assert!(old.join("notes/a/note.md").exists(), "the original is kept");
            let leftovers = fs::read_dir(&base)
                .unwrap()
                .filter(|entry| {
                    entry
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .contains(".moving-")
                })
                .count();
            assert_eq!(leftovers, 0, "no staging folder is left behind");
        }

        #[test]
        fn a_copy_that_differs_is_discarded_and_reported() {
            let _base_fixture = scratch("verify");
            let base = _base_fixture.path().to_path_buf();
            let (a, b) = (base.join("a"), base.join("b"));
            library(&a);
            copy_tree(&a, &b).unwrap();
            assert!(same_tree(&a, &b).unwrap());
            fs::write(b.join("notes/a/note.md"), "changed").unwrap();
            assert!(!same_tree(&a, &b).unwrap());
        }
    }
    fn display_frame_of(screen: ScreenRect, visible: ScreenRect, primary: ScreenRect) -> Frame {
        let _ = screen;
        display_frame(visible, primary)
    }
    use crate::notebook::Frame;
    #[test]
    fn display_frames_are_global_top_left_like_note_frames() {
        let primary = ScreenRect {
            x: 0.,
            y: 0.,
            width: 1440.,
            height: 900.,
        };
        // The menu bar takes the primary's top 25 points.
        let visible = ScreenRect {
            height: 875.,
            ..primary
        };
        assert_eq!(
            display_frame_of(primary, visible, primary),
            Frame {
                x: 0.,
                y: 25.,
                width: 1440.,
                height: 875.
            }
        );
        // A taller external display to the left whose bottom sits 200 points lower.
        let left = ScreenRect {
            x: -1920.,
            y: -200.,
            width: 1920.,
            height: 1080.,
        };
        assert_eq!(
            display_frame_of(left, left, primary),
            Frame {
                x: -1920.,
                y: 20.,
                width: 1920.,
                height: 1080.
            }
        );
        // An external display above the primary.
        let above = ScreenRect {
            x: 200.,
            y: 900.,
            width: 1280.,
            height: 800.,
        };
        assert_eq!(
            display_frame_of(above, above, primary),
            Frame {
                x: 200.,
                y: -800.,
                width: 1280.,
                height: 800.
            }
        );
    }
}

#[cfg(test)]
mod double_click_tests {
    use super::{DoubleClickAction::*, double_click_for};
    #[test]
    fn the_system_preference_picks_the_strip_double_click() {
        assert_eq!(double_click_for(Some("Maximize")), Zoom);
        assert_eq!(double_click_for(Some("Zoom")), Zoom);
        assert_eq!(double_click_for(Some("Minimize")), Minimize);
        assert_eq!(double_click_for(Some("None")), Nothing);
        // Unset or unrecognised keeps Stickies' collapse.
        assert_eq!(double_click_for(None), Collapse);
        assert_eq!(double_click_for(Some("Something New")), Collapse);
    }
}
#[cfg(test)]
mod arrange_tests {
    #[test]
    fn arrange_is_offered_only_where_windows_can_move() {
        assert!(super::can_arrange_on("macOS"));
        for system in ["X11", "Wayland", "Headless", "Other"] {
            assert!(!super::can_arrange_on(system), "{system}");
        }
        assert_eq!(super::can_arrange(), cfg!(target_os = "macos"));
    }
}

#[cfg(test)]
mod close_choice_tests {
    use super::CloseChoice;
    #[test]
    fn native_close_responses_are_explicit_and_unknown_or_dropped_prompts_cancel() {
        assert_eq!(CloseChoice::from_response(Some(0)), CloseChoice::Archive);
        assert_eq!(
            CloseChoice::from_response(Some(2)),
            CloseChoice::ExportAndArchive
        );
        assert_eq!(CloseChoice::from_response(Some(3)), CloseChoice::Trash);
        for response in [None, Some(1), Some(4), Some(usize::MAX)] {
            assert_eq!(CloseChoice::from_response(response), CloseChoice::Cancel);
        }
    }
}
