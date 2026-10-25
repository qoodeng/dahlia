use objc2::MainThreadOnly;
use objc2_app_kit::{NSApplication, NSWindow, NSWindowStyleMask};

pub fn set_style_mask(window: &NSWindow, mask: NSWindowStyleMask) {
    // Dahlia lists open and closed notes itself; AppKit must not append duplicates.
    window.setExcludedFromWindowsMenu(true);
    NSApplication::sharedApplication(window.mtm()).removeWindowsItem(window);
    if window.styleMask() != mask {
        // AppKit can replace the editor's first responder when changing the frame.
        let responder = window.firstResponder();
        window.setStyleMask(mask);
        if let Some(responder) = responder {
            window.makeFirstResponder(Some(&responder));
        }
    }
}
