//! Shared wording for messages people read: storage errors in plain words, file names in
//! curly quotes, counts with the right plural. The raw error still goes to the log.
use std::{io, path::Path};

/// What a storage error means to the person using Dahlia. Messages Dahlia wrote itself
/// pass through; operating-system errors become a sentence, never "(os error 35)".
pub fn plain(error: &io::Error) -> String {
    if error.raw_os_error().is_none() && error.get_ref().is_some() {
        return error.to_string();
    }
    let sentence = match error.kind() {
        io::ErrorKind::PermissionDenied => {
            "Dahlia doesn’t have permission to use this location. Check its permissions, then try again."
        }
        io::ErrorKind::NotFound => "The file is missing.",
        io::ErrorKind::StorageFull => "The disk is full. Free up space, then try again.",
        io::ErrorKind::ReadOnlyFilesystem => "The disk is read-only. Use a location you can write to.",
        io::ErrorKind::AlreadyExists => "A file with this name already exists. Choose a different name.",
        // `try_lock` reports a held library lock this way.
        io::ErrorKind::WouldBlock => {
            "Your notes are open in another copy of Dahlia. Quit that copy, then try again."
        }
        _ if error.raw_os_error().is_some() => "Dahlia couldn’t read or write a file.",
        _ => return error.to_string(),
    };
    log::warn!("{error}");
    sentence.to_owned()
}

/// A sentence about a failure followed by its plain reason, as one message.
pub fn failed(what: &str, error: &io::Error) -> String {
    format!("{what} {}", plain(error))
}

/// A file named the way Finder names it in messages: its name only, in curly quotes.
pub fn file(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy());
    format!("“{name}”")
}

/// "1 note", "3 notes".
pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Markraft's own wording for image states, in Dahlia's voice: "Can’t", and no third-party
/// option names.
pub fn editor_messages() -> markraft_gpui::EditorMessages {
    use markraft_gpui::EditorMessage;
    markraft_gpui::EditorMessages::new(|message, args| {
        let text = match message {
            EditorMessage::ImageRemoteFailed => "Can’t load remote image",
            EditorMessage::ImageUnreadable => "Can’t read image",
            EditorMessage::ImageUnsupportedRoot => "Can’t use this image’s location",
            message => message.english(),
        };
        args.iter().fold(text.to_owned(), |text, (key, value)| {
            text.replace(&format!("%{{{key}}}"), value)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_errors_read_as_sentences_and_own_messages_pass_through() {
        let lock = io::Error::from(io::ErrorKind::WouldBlock);
        assert_eq!(lock.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(
            plain(&lock),
            "Your notes are open in another copy of Dahlia. Quit that copy, then try again."
        );
        let unknown = io::Error::from_raw_os_error(5);
        assert!(!plain(&unknown).contains("os error"));
        let busy = io::Error::new(
            io::ErrorKind::WouldBlock,
            "Another library change is still saving. Try again shortly.",
        );
        assert_eq!(
            plain(&busy),
            "Another library change is still saving. Try again shortly."
        );
        let own = io::Error::other("This snapshot is incomplete.");
        assert_eq!(plain(&own), "This snapshot is incomplete.");
        assert_eq!(
            failed(
                "Couldn’t save this note.",
                &io::Error::from(io::ErrorKind::StorageFull)
            ),
            "Couldn’t save this note. The disk is full. Free up space, then try again."
        );
    }

    #[test]
    fn every_storage_failure_says_what_to_do_next() {
        for (kind, expected) in [
            (
                io::ErrorKind::PermissionDenied,
                "Dahlia doesn’t have permission to use this location. Check its permissions, then try again.",
            ),
            (io::ErrorKind::StorageFull, "The disk is full. Free up space, then try again."),
            (
                io::ErrorKind::ReadOnlyFilesystem,
                "The disk is read-only. Use a location you can write to.",
            ),
            (
                io::ErrorKind::AlreadyExists,
                "A file with this name already exists. Choose a different name.",
            ),
        ] {
            assert_eq!(plain(&io::Error::from(kind)), expected);
        }
        assert_eq!(
            plain(&io::Error::from_raw_os_error(5)),
            "Dahlia couldn’t read or write a file."
        );
    }

    #[test]
    fn files_are_named_without_their_folders_and_counts_agree() {
        assert_eq!(
            file(Path::new("/Users/me/Notes/Week 42.md")),
            "“Week 42.md”"
        );
        assert_eq!(count(1, "note", "notes"), "1 note");
        assert_eq!(count(0, "note", "notes"), "0 notes");
        assert_eq!(count(12, "match", "matches"), "12 matches");
    }

    #[test]
    fn editor_messages_keep_markraft_arguments() {
        use markraft_gpui::EditorMessage;
        let messages = editor_messages();
        assert_eq!(
            messages.text(EditorMessage::ImageRemoteFailed),
            "Can’t load remote image"
        );
        assert_eq!(
            messages.text(EditorMessage::ImageUnsupportedRoot),
            "Can’t use this image’s location"
        );
        assert_eq!(
            messages.format(EditorMessage::InlineImage, &[("name", "chart.png")]),
            "Inline image: chart.png"
        );
    }
}
