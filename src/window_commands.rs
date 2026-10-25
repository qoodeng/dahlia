#[cfg(test)]
use crate::appearance::ColorIndex;
use crate::{
    AppState,
    notebook::{Frame, NoteRecord},
};
use gpui::{Action, App, Focusable};
use serde::{Deserialize, Serialize};

const ARRANGE_FAILED: &str = "Couldn’t arrange the notes.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Color,
    Content,
    Date,
    Location,
}
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub struct Arrange {
    pub order: Order,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    Collapse,
    Expand,
    /// Also what a policy this version does not know reads as.
    #[default]
    #[serde(other)]
    Keep,
}
#[derive(Action, Clone, PartialEq)]
#[action(no_json)]
pub struct SetPolicy {
    pub policy: Policy,
}
#[derive(Clone, Debug)]
pub struct Geometry {
    id: String,
    frame: Frame,
    expanded_height: f32,
    collapsed: bool,
}
impl From<&NoteRecord> for Geometry {
    fn from(note: &NoteRecord) -> Self {
        Self {
            id: note.id.clone(),
            frame: note.frame,
            expanded_height: note.expanded_height,
            collapsed: note.collapsed,
        }
    }
}
fn capture(cx: &mut App) -> Vec<(NoteRecord, String, u128)> {
    let handles: Vec<_> = cx.global::<AppState>().windows.values().copied().collect();
    handles
        .into_iter()
        .filter_map(|handle| {
            handle
                .update(cx, |note, window, cx| {
                    note.capture(window);
                    let editor = note.editor.read(cx);
                    let text = editor
                        .context_text(&editor.context_document_snapshot())
                        .map(|text| text.text)
                        .unwrap_or_else(|| editor.text().to_owned());
                    let date = if note.record.modified_at != 0 {
                        note.record.modified_at as u128
                    } else {
                        std::fs::metadata(note.asset_directory.join("note.md"))
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(u128::MAX, |t| t.as_millis())
                    };
                    (note.record.clone(), text, date)
                })
                .ok()
        })
        .collect()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulkOperation {
    Collapse,
    Expand,
    Zoom,
    Minimize,
}
pub fn all(operation: BulkOperation, cx: &mut App) {
    let handles: Vec<_> = cx.global::<AppState>().windows.values().copied().collect();
    for handle in handles {
        let _ = handle.update(cx, |note, window, cx| match operation {
            BulkOperation::Collapse if !note.record.collapsed => note.collapse(window, cx),
            BulkOperation::Expand if note.record.collapsed => note.collapse(window, cx),
            BulkOperation::Zoom => note.toggle_zoom(window, cx),
            BulkOperation::Minimize => window.minimize_window(),
            _ => {}
        });
    }
}
pub fn arrange(order: Order, cx: &mut App) {
    let notes = capture(cx);
    let previous: Vec<_> = notes
        .iter()
        .map(|(note, _, _)| Geometry::from(note))
        .collect();
    let plan = layout(
        notes,
        order,
        cx.global::<AppState>().notebook.borrow().arrange_policy(),
        &crate::displays(cx),
    );
    apply(
        plan,
        move |cx| {
            let state = cx.global_mut::<AppState>();
            state.arrange_undo.push(previous);
            if state.arrange_undo.len() > 20 {
                state.arrange_undo.remove(0);
            }
            state.arrange_redo.clear();
        },
        cx,
    );
}

pub fn undo(redo: bool, cx: &mut App) {
    let plan = if redo {
        cx.global::<AppState>().arrange_redo.last()
    } else {
        cx.global::<AppState>().arrange_undo.last()
    }
    .cloned();
    let Some(plan) = plan else {
        return;
    };
    let current = capture(cx)
        .iter()
        .map(|(note, _, _)| Geometry::from(note))
        .collect();
    apply(
        plan,
        move |cx| {
            let state = cx.global_mut::<AppState>();
            if redo {
                state.arrange_redo.pop();
                state.arrange_undo.push(current);
            } else {
                state.arrange_undo.pop();
                state.arrange_redo.push(current);
            }
        },
        cx,
    );
}

fn apply(plan: Vec<Geometry>, on_commit: impl FnOnce(&mut App) + 'static, cx: &mut App) {
    if cx.global::<AppState>().quitting {
        return;
    }
    let state = cx.global::<AppState>();
    let notebook = state.notebook.clone();
    let mut changes = Vec::new();
    for geometry in plan {
        if !state.windows.contains_key(&geometry.id) {
            continue;
        }
        let Some(mut note) = notebook.borrow().note(&geometry.id) else {
            continue;
        };
        note.rearrange(
            geometry.frame,
            geometry.expanded_height,
            geometry.collapsed,
        );
        changes.push(note.restored(&crate::displays(cx)));
    }
    let pending = notebook.borrow_mut().request_update_many(changes.clone());
    let pending = match pending {
        Ok(pending) => pending,
        Err(error) => {
            crate::notify::notify(cx, crate::copy::failed(ARRANGE_FAILED, &error));
            return;
        }
    };
    cx.spawn(async move |cx| {
        let completed = pending.wait().await;
        cx.update(|cx| {
            if let Err(error) = notebook.borrow_mut().complete(completed) {
                crate::notify::notify(cx, crate::copy::failed(ARRANGE_FAILED, &error));
                return;
            }

            for record in changes {
                let Some(handle) = cx.global::<AppState>().windows.get(&record.id).copied() else {
                    continue;
                };
                let _ = handle.update(cx, |note, window, cx| {
                    if record.collapsed && note.editor.read(cx).is_composing() {
                        note.editor
                            .update(cx, |editor, cx| editor.cancel_composition(cx));
                    }
                    // Change geometry only; newer text, colors and checking settings survive history.
                    note.record.rearrange(record.frame, record.expanded_height, record.collapsed);
                    note.settle_bounds();
                    crate::platform::configure(
                        window,
                        record.collapsed,
                        record.floating,
                        record.style.frosted(),
                        None,
                    );
                    crate::platform::place(window, record.frame, false);
                    if record.collapsed {
                        window.focus(&note.shell_focus, cx);
                    } else {
                        window.focus(&note.editor.read(cx).focus_handle(cx), cx);
                    }
                    cx.notify();
                });
            }
            on_commit(cx);
            crate::update_menus(cx);
        });
    })
    .detach();
}

fn layout(
    mut notes: Vec<(NoteRecord, String, u128)>,
    order: Order,
    policy: Policy,
    screens: &[Frame],
) -> Vec<Geometry> {
    notes.sort_by(|(a, ac, ad), (b, bc, bd)| {
        let content = || ac.to_lowercase().cmp(&bc.to_lowercase());
        (match order {
            Order::Color => a.style.color.cmp(&b.style.color).then_with(content),
            Order::Content => content(),
            Order::Date => ad.cmp(bd),
            Order::Location => a
                .frame
                .x
                .total_cmp(&b.frame.x)
                .then(a.frame.y.total_cmp(&b.frame.y)),
        })
        .then(a.id.cmp(&b.id))
    });
    let areas: Vec<_> = screens
        .iter()
        .filter(|s| s.width > 16. && s.height > 16.)
        .map(|s| Frame {
            x: s.x + 8.,
            y: s.y + 8.,
            width: s.width - 16.,
            height: s.height - 16.,
        })
        .collect();
    if areas.is_empty() {
        return vec![];
    }
    let mut screen = 0;
    let mut x = areas[0].x;
    let mut y = areas[0].y;
    let mut column: f32 = 0.;
    let mut overflow = 0;
    notes
        .into_iter()
        .map(|(note, _, _)| {
            let mut geometry = Geometry::from(&note);
            geometry.collapsed = match policy {
                Policy::Collapse => true,
                Policy::Expand => false,
                Policy::Keep => note.collapsed,
            };
            let height = if geometry.collapsed {
                crate::notebook::STRIP_HEIGHT
            } else {
                note.expanded_height
            };
            let mut area = areas[screen.min(areas.len() - 1)];
            let mut width = note.frame.width.min(area.width);
            let mut height = height.min(area.height);
            if screen < areas.len() {
                if y + height > area.y + area.height && column > 0. {
                    x += column + 8.;
                    y = area.y;
                    column = 0.;
                }
                if x + width > area.x + area.width {
                    screen += 1;
                    if screen < areas.len() {
                        area = areas[screen];
                        x = area.x;
                        y = area.y;
                        column = 0.;
                        width = width.min(area.width);
                        height = height.min(area.height);
                    }
                }
            }
            if screen >= areas.len() {
                area = areas[overflow % areas.len()];
                let offset = (overflow / areas.len() + 1) as f32 * 20.;
                width = note.frame.width.min(area.width);
                height = height.min(area.height);
                x = area.x + offset.min(area.width - width);
                y = area.y + offset.min(area.height - height);
                overflow += 1;
            }
            geometry.frame = Frame {
                x,
                y,
                width,
                height,
            }
            .restored(&[area], geometry.collapsed);
            if screen < areas.len() {
                y += height + 8.;
                column = column.max(width);
            }
            geometry
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrangement_orders_and_bounds_overflow_without_changing_preferred_height() {
        let _root_fixture = tempfile::tempdir().unwrap();
        let root = _root_fixture.path().to_path_buf();
        let mut library = crate::notebook::Notebook::open(root.clone()).unwrap();
        let mut notes = Vec::new();
        for n in 0..18 {
            let mut note = library.create(Frame::default()).unwrap();
            note.style.color = ColorIndex::ALL[n % ColorIndex::ALL.len()];
            notes.push((note, format!("{n:02}"), n as u128));
        }
        let screen = Frame {
            x: 0.,
            y: 0.,
            width: 720.,
            height: 500.,
        };
        let plan = layout(notes, Order::Color, Policy::Collapse, &[screen]);
        assert_eq!(plan.len(), 18);
        for geometry in plan {
            assert!(geometry.collapsed);
            assert_eq!(geometry.expanded_height, 350.);
            assert_eq!(geometry.frame.height, 12.);
            assert!(geometry.frame.x >= 0. && geometry.frame.x + geometry.frame.width <= 720.);
            assert!(geometry.frame.y >= 0. && geometry.frame.y + geometry.frame.height <= 500.);
        }
        drop(library);
    }
}
