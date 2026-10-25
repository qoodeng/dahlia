#![deny(clippy::undocumented_unsafe_blocks)]
// AppKit requires the main thread; the standard Rust test harness uses worker threads.
#[cfg(target_os = "macos")]
use dahlia::set_note_style_mask;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSMenu, NSTextView, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    let main_thread = MainThreadMarker::new().expect("native test must run on the main thread");
    let _app = NSApplication::sharedApplication(main_thread);
    // Exercise the real GPUI delegate selector without terminating or showing an app.
    let delegate_class = objc2::runtime::AnyClass::get(c"GPUIApplicationDelegate")
        .expect("GPUI's native delegate must be linked");
    // SAFETY: GPUI's delegate inherits NSResponder's ordinary retained `new` initializer.
    let delegate: objc2::rc::Retained<objc2::runtime::AnyObject> =
        unsafe { objc2::msg_send![delegate_class, new] };
    use gpui_macos::TerminationReply::{Cancel, Later, Now};
    let answer = std::rc::Rc::new(std::cell::Cell::new(Later));
    let requests = std::rc::Rc::new(std::cell::Cell::new(0));
    gpui_macos::set_termination_guard(main_thread, {
        let answer = answer.clone();
        let requests = requests.clone();
        move || {
            requests.set(requests.get() + 1);
            answer.get()
        }
    });
    // NSTerminateCancel, NSTerminateNow and NSTerminateLater. Later is what keeps a system
    // logout or shutdown alive while Dahlia saves; Cancel would abort it.
    for (guard, expected) in [(Later, 2usize), (Cancel, 0), (Now, 1)] {
        answer.set(guard);
        // SAFETY: the owned GPUI patch registers this exact NSUInteger-returning selector.
        let reply: usize =
            unsafe { objc2::msg_send![&*delegate, applicationShouldTerminate: &*_app] };
        assert_eq!(
            reply, expected,
            "termination must wait for durable acknowledgement"
        );
    }
    assert_eq!(requests.get(), 3);
    // SAFETY: retained, main-thread window with a valid frame; closing must not release it.
    let window = unsafe {
        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(main_thread),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(360., 350.)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView,
            NSBackingStoreType::Buffered,
            false,
        );
        window.setReleasedWhenClosed(false);
        window
    };
    let editor = NSTextView::new(main_thread);
    window.setContentView(Some(&editor));
    assert!(window.makeFirstResponder(Some(&editor)));
    let responder = window
        .firstResponder()
        .expect("editor must own keyboard input");
    assert!(!window.isExcludedFromWindowsMenu());
    let menu = NSMenu::new(main_thread);
    _app.setWindowsMenu(Some(&menu));
    _app.addWindowsItem_title_filename(&window, &NSString::from_str("Native test note"), false);
    assert!(
        menu.numberOfItems() > 0,
        "fixture must contain AppKit's automatic entry"
    );

    // Glass, collapsed Glass, Classic, collapsed Classic, and expanded Classic.
    for mask in [8, 0, 14, 6, 14, 14] {
        set_note_style_mask(&window, NSWindowStyleMask::from_bits_retain(mask));
        assert!(
            window.isExcludedFromWindowsMenu(),
            "Dahlia lists its notes itself; AppKit must not append them a second time"
        );
        assert_eq!(
            menu.numberOfItems(),
            0,
            "the pre-existing automatic entry must be removed"
        );
        let current = window
            .firstResponder()
            .expect("style change lost keyboard input");
        assert!(
            std::ptr::eq(&*current, &*responder),
            "style mask {mask} replaced the editor's keyboard receiver"
        );
    }
    assert!(
        !window.isVisible(),
        "native harness must never show a window"
    );
    println!("PASS: native note-style transitions preserve the editor's keyboard receiver");
    println!("PASS: native termination is deferred, not cancelled, until saving is acknowledged");
    println!("PASS: native note windows stay out of AppKit's automatic Window list");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("Native first-responder regression requires macOS");
}
