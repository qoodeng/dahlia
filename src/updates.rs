//! Automatic updates through Sparkle 2.
//!
//! Only packaged builds have Sparkle: `scripts/package-mac.py` embeds
//! `Contents/Frameworks/Sparkle.framework` and adds the `SU*` keys to Info.plist. Dahlia loads
//! the framework at run time rather than linking it, so `cargo build`, `cargo test` and a
//! development build need neither Sparkle nor its SDK, and simply have no updater.
//!
//! Sparkle installs an update by calling `[NSApp terminate:]`, which reaches
//! `applicationShouldTerminate:` and so the termination guard in `app.rs`: notes are saved
//! before it answers, and the app only relaunches after that reply.
use crate::*;
use std::path::{Path, PathBuf};

/// The menu label; AppKit convention is a trailing ellipsis because a window follows.
pub(crate) const CHECK_LABEL: &str = "Check for Updates…";
/// Where the framework sits inside an app bundle.
const FRAMEWORK: &str = "Contents/Frameworks/Sparkle.framework";
/// Sparkle's ready-made controller: owns the updater and shows its standard windows.
#[cfg(target_os = "macos")]
const CONTROLLER_CLASS: &str = "SPUStandardUpdaterController";
/// The Info.plist key packaging sets; without it Sparkle has nowhere to look.
#[cfg(target_os = "macos")]
const FEED_KEY: &str = "SUFeedURL";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn framework_path(bundle: &Path) -> PathBuf {
    bundle.join(FRAMEWORK)
}

/// Whether `bundle` is an app bundle with Sparkle inside. A bare `cargo run` binary's "bundle"
/// is its directory, which is neither.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn bundles_sparkle(bundle: &Path) -> bool {
    bundle.extension().is_some_and(|extension| extension == "app")
        && framework_path(bundle).join("Sparkle").is_file()
}

/// Sparkle starts only in a bundle that has both the framework and a feed to read, so a
/// hand-made development bundle that borrowed the framework never shows a feed error.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn should_start(bundle: &Path, has_feed: bool) -> bool {
    has_feed && bundles_sparkle(bundle)
}

/// How the app menu offers the check: not at all where there is no Sparkle to ask (`supported`
/// false), disabled while it is not loaded (a development build), otherwise live.
pub(crate) fn menu_item(supported: bool, available: bool) -> Option<MenuItem> {
    supported.then(|| menu_action(CHECK_LABEL, CheckForUpdates, available))
}

#[cfg(target_os = "macos")]
thread_local! {
    /// Kept for the life of the app: dropping the controller stops the updater.
    static CONTROLLER: std::cell::RefCell<Option<objc2::rc::Retained<objc2::runtime::AnyObject>>> =
        const { std::cell::RefCell::new(None) };
}

/// Whether Sparkle loaded and is running, so "Check for Updates…" can do something.
pub(crate) fn available() -> bool {
    #[cfg(target_os = "macos")]
    {
        CONTROLLER.with(|controller| controller.borrow().is_some())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Start the updater if this is a packaged build. Call once, on the main thread, at launch.
pub(crate) fn start() {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSBundle, NSString};
        if available() || objc2::MainThreadMarker::new().is_none() {
            return;
        }
        let main = NSBundle::mainBundle();
        let path = PathBuf::from(main.bundlePath().to_string());
        let has_feed = main
            .objectForInfoDictionaryKey(&NSString::from_str(FEED_KEY))
            .is_some();
        if !should_start(&path, has_feed) {
            return;
        }
        match load_controller(&framework_path(&path)) {
            Ok(controller) => CONTROLLER.with(|slot| *slot.borrow_mut() = Some(controller)),
            Err(error) => log::warn!("Automatic updates are unavailable: {error}"),
        }
    }
}

#[cfg(target_os = "macos")]
fn load_controller(
    framework: &Path,
) -> Result<objc2::rc::Retained<objc2::runtime::AnyObject>, String> {
    use objc2::{msg_send, rc::Allocated, runtime::AnyObject};
    use objc2_foundation::{NSBundle, NSClassFromString, NSString};
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(&framework.to_string_lossy()))
        .ok_or("Sparkle.framework could not be opened")?;
    // SAFETY: this is the framework packaging signed inside Dahlia's own bundle; loading it
    // runs its initializers and nothing else.
    unsafe { bundle.loadAndReturnError() }
        .map_err(|error| error.localizedDescription().to_string())?;
    let class = NSClassFromString(&NSString::from_str(CONTROLLER_CLASS))
        .ok_or("Sparkle.framework has no SPUStandardUpdaterController")?;
    // SAFETY: `+alloc` returns an uninitialised instance, which the next call consumes.
    let allocated: Allocated<AnyObject> = unsafe { msg_send![class, alloc] };
    // SAFETY: `-initWithStartingUpdater:updaterDelegate:userDriverDelegate:` takes a BOOL and
    // two optional objects (both nil here) and returns the initialised, owned controller.
    let controller: Option<objc2::rc::Retained<AnyObject>> = unsafe {
        msg_send![
            allocated,
            initWithStartingUpdater: true,
            updaterDelegate: None::<&AnyObject>,
            userDriverDelegate: None::<&AnyObject>
        ]
    };
    controller.ok_or_else(|| "Sparkle could not create its updater".to_owned())
}

/// Check for updates now and show Sparkle's window, whatever the result.
pub(crate) fn check() {
    #[cfg(target_os = "macos")]
    CONTROLLER.with(|controller| {
        if let Some(controller) = &*controller.borrow() {
            use objc2::{msg_send, runtime::AnyObject};
            // SAFETY: `-checkForUpdates:` takes the sender (nil is allowed) and returns nothing;
            // the controller is alive, and actions run on the main thread that AppKit needs.
            unsafe { msg_send![controller, checkForUpdates: None::<&AnyObject>] }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        CHECK_LABEL, CheckForUpdates, MenuItem, available, bundles_sparkle, check, framework_path,
        menu_item, should_start, start,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    fn bundle(root: &Path, name: &str, framework: bool) -> PathBuf {
        let app = root.join(name);
        if framework {
            let sparkle = framework_path(&app);
            fs::create_dir_all(sparkle.join("Versions/B")).unwrap();
            fs::write(sparkle.join("Versions/B/Sparkle"), b"sparkle").unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink("Versions/B/Sparkle", sparkle.join("Sparkle")).unwrap();
        } else {
            fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        }
        app
    }

    #[test]
    fn the_framework_is_looked_for_where_packaging_puts_it() {
        assert_eq!(
            framework_path(Path::new("/Applications/Dahlia.app")),
            Path::new("/Applications/Dahlia.app/Contents/Frameworks/Sparkle.framework")
        );
    }

    #[test]
    fn only_an_app_bundle_with_the_framework_has_sparkle() {
        let root = tempfile::tempdir().unwrap();
        let packaged = bundle(root.path(), "Dahlia.app", true);
        assert!(bundles_sparkle(&packaged));
        assert!(!bundles_sparkle(&bundle(root.path(), "Bare.app", false)));
        // A development binary: its "bundle" is a plain directory.
        let target = bundle(root.path(), "debug", false);
        assert!(!bundles_sparkle(&target));
        // The framework alone does not make a folder an app.
        assert!(!bundles_sparkle(&bundle(root.path(), "NotAnApp", true)));
        assert!(!bundles_sparkle(&root.path().join("missing.app")));
    }

    #[test]
    fn an_empty_framework_folder_is_not_sparkle() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("Dahlia.app");
        fs::create_dir_all(framework_path(&app)).unwrap();
        assert!(!bundles_sparkle(&app));
    }

    #[test]
    fn sparkle_starts_only_with_both_the_framework_and_a_feed() {
        let root = tempfile::tempdir().unwrap();
        let packaged = bundle(root.path(), "Dahlia.app", true);
        let bare = bundle(root.path(), "Bare.app", false);
        assert!(should_start(&packaged, true));
        assert!(!should_start(&packaged, false));
        assert!(!should_start(&bare, true));
        assert!(!should_start(&bare, false));
    }

    #[test]
    fn nothing_runs_without_the_framework() {
        // Unit tests run unbundled, as a development build does.
        start();
        assert!(!available());
        check();
    }

    #[test]
    fn the_menu_item_is_absent_without_sparkle_support_and_disabled_until_it_loads() {
        let disabled = |item: &MenuItem| match item {
            MenuItem::Action {
                name,
                action,
                disabled,
                ..
            } => (
                name.to_string(),
                *disabled,
                action.as_any().downcast_ref::<CheckForUpdates>().is_some(),
            ),
            _ => panic!("not an action"),
        };
        assert!(menu_item(false, false).is_none());
        assert!(menu_item(false, true).is_none());
        let live = menu_item(true, true).unwrap();
        assert_eq!(disabled(&live), (CHECK_LABEL.to_owned(), false, true));
        let unloaded = menu_item(true, false).unwrap();
        let (name, is_disabled, runs_check) = disabled(&unloaded);
        assert_eq!(name, CHECK_LABEL);
        assert!(is_disabled);
        assert!(!runs_check, "a disabled item must not carry the live action");
    }
}
