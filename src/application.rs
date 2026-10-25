mod appearance;
mod attachments;
mod checking;
mod chrome;
mod copy;
mod finding;
#[cfg(target_os = "macos")]
mod macos_input;
mod markdown;
mod notebook;
mod notify;
mod operations;
mod panel;
#[cfg(test)]
mod perf_bench;
mod platform;
mod recovery;
mod settings;
mod storage;
mod transfer;
mod updates;
mod window_commands;

mod app;
mod keys;
mod menus;
mod note;
#[cfg(not(target_os = "macos"))]
mod linux_menu;
#[cfg(not(target_os = "macos"))]
mod prompt;

use appearance::{Appearance, ColorIndex, Fonts};
use gpui::{prelude::*, *};
use markraft_commonmark::{
    SourceDocument, SourceTrack, commonmark_doc_type_names, commonmark_extensions,
    commonmark_schema,
};
use markraft_core::{Schema, kind::DocTypes};
use markraft_gpui::{EditRejection, EditorEvent, EditorView, Setup};
use notebook::{Frame, NoteRecord, NoteState, Notebook};
use notify::notify;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc, time::Duration};
use storage::{NoteStore, NoteWriter};
type EditorFormatState = (
    markraft_core::MarkSet,
    Option<(markraft_core::NodeTypeId, markraft_core::Attrs)>,
);

use chrome::{Control, HEIGHT};

pub(crate) use app::*;
pub(crate) use keys::*;
pub(crate) use menus::*;
pub(crate) use note::*;

/// Run Dahlia using the native platform event loop.
pub fn run() {
    app::run();
}

/// Preserve keyboard focus during native note-style changes.
#[cfg(target_os = "macos")]
pub use macos_input::set_style_mask as set_note_style_mask;
