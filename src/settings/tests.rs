use super::{GlassControl, SliderValue, appearance, show};
use crate::{AppState, Notebook, appearance::Fonts};
use gpui::{AnyWindowHandle, TestAppContext, WindowHandle};
use std::{cell::RefCell, rc::Rc, time::Duration};

fn setup(cx: &mut TestAppContext) -> (tempfile::TempDir, Rc<RefCell<Notebook>>) {
    // The real metadata worker completes outside GPUI's deterministic executor.
    cx.executor().allow_parking();
    let fixture = tempfile::tempdir().unwrap();
    let mut notebook = Notebook::open(fixture.path().to_owned()).unwrap();
    let mut problems = notebook.take_problems().unwrap();
    let library = Rc::new(RefCell::new(notebook));
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_global(AppState::new(library.clone(), Fonts::available(&[])));
        // Use the same native-worker reporting path as application startup.
        cx.spawn(async move |cx| {
            while let Some(problem) = futures::StreamExt::next(&mut problems).await {
                cx.update(|cx| crate::notify::notify(cx, problem));
            }
        })
        .detach();
    });
    (fixture, library)
}
fn open(cx: &mut TestAppContext) -> WindowHandle<GlassControl> {
    cx.update(show);
    let handle = cx.update(|cx| cx.global::<AppState>().transparency_window.unwrap());
    AnyWindowHandle::from(handle)
        .update(cx, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    handle
}
fn percent(handle: WindowHandle<GlassControl>, cx: &mut TestAppContext) -> f32 {
    handle
        .update(cx, |control, _, cx| {
            let SliderValue::Single(value) = control.slider.read(cx).value() else {
                panic!("transparency must have one value");
            };
            value
        })
        .unwrap()
}
fn persist(cx: &mut TestAppContext) {
    cx.executor().advance_clock(Duration::from_millis(500));
    crate::settle_io(cx);
}

#[gpui::test]
fn settings_focus_reuse_and_all_close_routes_work_in_test_windows(cx: &mut TestAppContext) {
    let (_fixture, _library) = setup(cx);
    let first = open(cx);
    assert!(
        first
            .update(cx, |control, window, _| control.focus.is_focused(window))
            .unwrap()
    );
    assert_eq!(
        percent(first, cx),
        appearance::transparency(appearance::DEFAULT_TINT)
    );
    assert_eq!(open(cx), first);
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    for key in ["escape", "enter"] {
        let handle = open(cx);
        cx.simulate_keystrokes(handle.into(), key);
        assert!(handle.update(cx, |_, _, _| ()).is_err());
        // show() must recover from the stale handle left by a closed panel.
        let replacement = open(cx);
        assert_ne!(replacement, handle);
        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    }
    let handle = open(cx);
    handle
        .update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(crate::CloseNote), cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert!(handle.update(cx, |_, _, _| ()).is_err());
}

#[gpui::test]
fn settings_keyboard_changes_the_actual_slider_and_clamps_both_ends(cx: &mut TestAppContext) {
    let (_fixture, library) = setup(cx);
    let handle = open(cx);
    for (keys, expected) in [
        ("home left down", 0.),
        ("right up", 2.),
        ("end right up", 100.),
        ("left down", 98.),
        ("z", 98.),
    ] {
        cx.simulate_keystrokes(handle.into(), keys);
        assert!((percent(handle, cx) - expected).abs() < 0.0001, "{keys}");
        let expected_tint = appearance::tint_from_transparency(expected);
        assert!((library.borrow().glass_tint() - expected_tint).abs() < f32::EPSILON);
    }
    persist(cx);
    assert_eq!(
        library.borrow().glass_tint(),
        appearance::tint_from_transparency(98.)
    );
}

#[gpui::test]
fn settings_slider_and_programmatic_sync_preserve_the_latest_durable_value(
    cx: &mut TestAppContext,
) {
    let (fixture, library) = setup(cx);
    let handle = open(cx);
    for value in [13., 67., 37.] {
        handle
            .update(cx, |control, window, cx| {
                control
                    .slider
                    .update(cx, |slider, cx| slider.set_value(value, window, cx));
            })
            .unwrap();
    }
    cx.run_until_parked();
    assert!(
        (library.borrow().glass_tint() - appearance::tint_from_transparency(37.)).abs()
            < f32::EPSILON
    );
    persist(cx);
    let disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.path().join("index.json")).unwrap()).unwrap();
    assert!(
        (disk["glass_tint"].as_f64().unwrap() - f64::from(library.borrow().glass_tint())).abs()
            < 0.000001
    );
    for tint in [0., 0.12, appearance::MAX_TINT, f32::NAN, f32::INFINITY] {
        cx.update(|cx| crate::set_glass_tint(tint, cx));
        cx.run_until_parked();
        assert!((percent(handle, cx) - appearance::transparency(tint)).abs() < 0.0001);
        assert!(
            (library.borrow().glass_tint() - appearance::clamp_tint(tint)).abs() < f32::EPSILON
        );
    }
    persist(cx);
    let disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.path().join("index.json")).unwrap()).unwrap();
    assert!(
        (disk["glass_tint"].as_f64().unwrap() - f64::from(appearance::DEFAULT_TINT)).abs()
            < 0.000001
    );
}

#[gpui::test]
fn settings_write_conflict_keeps_external_metadata_in_recovery_and_saves(cx: &mut TestAppContext) {
    let (fixture, library) = setup(cx);
    let handle = open(cx);
    crate::notify::take_alerts();
    let external = b"metadata replaced by another app";
    std::fs::write(fixture.path().join("index.json"), external).unwrap();
    handle
        .update(cx, |control, window, cx| {
            control
                .slider
                .update(cx, |slider, cx| slider.set_value(40., window, cx));
        })
        .unwrap();
    persist(cx);
    let disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.path().join("index.json")).unwrap()).unwrap();
    assert!(
        (disk["glass_tint"].as_f64().unwrap() - f64::from(appearance::tint_from_transparency(40.)))
            .abs()
            < 0.000001
    );
    let kept = std::fs::read_dir(fixture.path().join("recovery/preserved"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| std::fs::read(entry.path()).is_ok_and(|bytes| bytes == external));
    assert!(kept, "the replaced index must be preserved");
    assert!(
        (library.borrow().glass_tint() - appearance::tint_from_transparency(40.)).abs()
            < f32::EPSILON
    );
    let alerts = crate::notify::take_alerts();
    assert!(
        alerts
            .iter()
            .any(|message| message.contains("Another app changed Dahlia’s list of notes")),
        "{alerts:?}"
    );
}
