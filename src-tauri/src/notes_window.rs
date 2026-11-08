//! The detached notes window (contract: docs/wiki/pages/syntheses/notes.md).
//!
//! Only the backend creates it, with a fixed label, a fixed app URL and a fixed size, so the
//! main window needs no permission to create arbitrary webviews. The notes window may only
//! listen to events; whatever it has to tell the main window goes through
//! [`NotesMessage`], which the backend validates and delivers to `main` alone.

use serde::{Deserialize, Serialize, Serializer};

pub const LABEL: &str = "notes";
pub const MAIN_LABEL: &str = "main";
/// Relative to the app URL; the frontend renders the notes view for `?window=notes`.
pub const URL: &str = "index.html?window=notes";
pub const WIDTH: f64 = 440.0;
pub const HEIGHT: f64 = 640.0;
pub const MIN_WIDTH: f64 = 320.0;
pub const MIN_HEIGHT: f64 = 360.0;
/// Event the main window receives with a [`NotesMessage`] as payload.
pub const MESSAGE_EVENT: &str = "notes-message";
/// Text sent to a terminal, as for a note: at most 1 MiB.
pub const MAX_TEXT_BYTES: usize = crate::notes::MAX_NOTE_BYTES;

/// What the notes window may ask of the main window. A closed set: anything else is rejected
/// when the arguments are read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NotesMessage {
    /// Paste `text` into the session `sessionId` (or the active one); the main window pastes it
    /// as it pastes from the clipboard (bracketed paste).
    #[serde(rename_all = "camelCase")]
    SendToTerminal {
        text: String,
        session_id: Option<u64>,
    },
    /// Notes of a scope changed in the notes window; the main window reloads its list.
    #[serde(rename_all = "camelCase")]
    NotesChanged { folder_key: String },
    /// Bring the main window to the front. An empty struct rather than a unit variant: serde
    /// enforces `deny_unknown_fields` only on struct variants of an internally tagged enum.
    ShowMain {},
}

#[derive(Debug, thiserror::Error)]
pub enum NotesWindowError {
    #[error("Only the notes window may send notes messages")]
    WrongWindow,
    #[error("Invalid notes message: {0}")]
    Invalid(&'static str),
    #[error("Notes window: {0}")]
    Tauri(#[from] tauri::Error),
}

impl Serialize for NotesWindowError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl NotesMessage {
    /// Text without terminal sequences: tab and line breaks only. An ESC could end bracketed
    /// paste early (`ESC[201~`) and turn the rest into typed commands.
    pub fn validate(&self) -> Result<(), NotesWindowError> {
        match self {
            Self::SendToTerminal { text, .. } => {
                if text.is_empty() {
                    return Err(NotesWindowError::Invalid("empty text"));
                }
                if text.len() > MAX_TEXT_BYTES {
                    return Err(NotesWindowError::Invalid("text too large"));
                }
                if text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
                {
                    return Err(NotesWindowError::Invalid(
                        "text contains control characters",
                    ));
                }
                Ok(())
            }
            Self::NotesChanged { folder_key } => {
                if crate::notes::is_folder_key(folder_key) {
                    Ok(())
                } else {
                    Err(NotesWindowError::Invalid("unknown folder key"))
                }
            }
            Self::ShowMain {} => Ok(()),
        }
    }
}

/// Only the window labelled `notes` may send; the main window has no use for it.
pub fn check_sender(label: &str) -> Result<(), NotesWindowError> {
    if label == LABEL {
        Ok(())
    } else {
        Err(NotesWindowError::WrongWindow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<NotesMessage, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn messages_are_a_closed_validated_set() {
        let send = parse(r#"{"kind":"sendToTerminal","text":"ls -la\n","sessionId":3}"#).unwrap();
        assert_eq!(
            send,
            NotesMessage::SendToTerminal {
                text: "ls -la\n".into(),
                session_id: Some(3)
            }
        );
        assert!(send.validate().is_ok());
        assert!(parse(r#"{"kind":"showMain"}"#).unwrap().validate().is_ok());
        let changed = parse(r#"{"kind":"notesChanged","folderKey":"general"}"#).unwrap();
        assert!(changed.validate().is_ok());
        assert!(parse(&format!(
            r#"{{"kind":"notesChanged","folderKey":"{}"}}"#,
            "a".repeat(32)
        ))
        .unwrap()
        .validate()
        .is_ok());

        for bad in [
            r#"{"kind":"trayQuit"}"#,
            r#"{"kind":"showMain","extra":1}"#,
            r#"{"kind":"sendToTerminal"}"#,
            r#"{"text":"x"}"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        for bad in [
            r#"{"kind":"sendToTerminal","text":""}"#,
            r#"{"kind":"sendToTerminal","text":"a\u001b[201~rm -rf x"}"#,
            r#"{"kind":"sendToTerminal","text":"a\u0003"}"#,
            r#"{"kind":"notesChanged","folderKey":"../x"}"#,
        ] {
            assert!(parse(bad).unwrap().validate().is_err(), "{bad}");
        }
        let big = NotesMessage::SendToTerminal {
            text: "x".repeat(MAX_TEXT_BYTES + 1),
            session_id: None,
        };
        assert!(big.validate().is_err());
        // The main window receives exactly what was validated.
        let json = serde_json::to_value(&send).unwrap();
        assert_eq!(json["kind"], "sendToTerminal");
        assert_eq!(json["sessionId"], 3);
    }

    #[test]
    fn only_the_notes_window_sends() {
        assert!(check_sender("notes").is_ok());
        assert!(matches!(
            check_sender("main"),
            Err(NotesWindowError::WrongWindow)
        ));
    }
}
