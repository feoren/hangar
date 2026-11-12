//! The detached side panel window (ADR 0007, contract: docs/wiki/pages/syntheses/notes.md).
//!
//! Only the backend creates it, with a fixed label and a fixed app URL, so the main window needs
//! no permission to create arbitrary webviews. The panel window may only listen to events;
//! whatever it has to tell the main window goes through [`PanelMessage`], which the backend
//! validates and delivers to `main` alone. The main window stays the single source of truth: it
//! pushes the active session's context to the panel, the panel never reads sessions itself.

use serde::{Deserialize, Serialize, Serializer};

pub const LABEL: &str = "panel";
pub const MAIN_LABEL: &str = "main";
/// Relative to the app URL; the frontend renders the panel view for `?window=panel`.
pub const URL: &str = "index.html?window=panel";
pub const WIDTH: f64 = 460.0;
pub const HEIGHT: f64 = 680.0;
pub const MIN_WIDTH: f64 = 320.0;
pub const MIN_HEIGHT: f64 = 360.0;
/// Larger remembered sizes are taken for garbage, not for a huge screen.
pub const MAX_SIDE: f64 = 16_384.0;
/// Event the main window receives with a [`PanelMessage`] as payload.
pub const MESSAGE_EVENT: &str = "panel-message";
/// Text sent to a terminal, as for a note: at most 1 MiB.
pub const MAX_TEXT_BYTES: usize = crate::notes::MAX_NOTE_BYTES;
/// A remembered position counts as visible when this part of the title bar, measured from the
/// window's top left corner, lies on a monitor: enough to grab and move it.
const GRIP_X: f64 = 80.0;
const GRIP_Y: f64 = 16.0;

/// What the panel window may ask of the main window. A closed set: anything else is rejected
/// when the arguments are read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PanelMessage {
    /// Paste `text` into the session `sessionId` (or the active one); the main window pastes it
    /// as it pastes from the clipboard (bracketed paste).
    #[serde(rename_all = "camelCase")]
    SendToTerminal {
        text: String,
        session_id: Option<u64>,
    },
    /// Notes of a scope changed in the panel window; the main window reloads its list.
    #[serde(rename_all = "camelCase")]
    NotesChanged { folder_key: String },
    /// Bring the main window to the front. An empty struct rather than a unit variant: serde
    /// enforces `deny_unknown_fields` only on struct variants of an internally tagged enum.
    ShowMain {},
    /// Make that session the active one (a claims warning names another session); the backend
    /// also brings the main window to the front. The main window ignores unknown ids.
    #[serde(rename_all = "camelCase")]
    ShowSession { session_id: u64 },
    /// The panel window has loaded and listens: the main window sends its context now.
    Ready {},
}

/// Where the panel window was last, in logical pixels, as the frontend remembered it.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A monitor's work area in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// How the window is placed: the remembered size (clamped), and the remembered position only
/// while it is still on a monitor (a monitor that was unplugged would hide the window).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub width: f64,
    pub height: f64,
    pub position: Option<(f64, f64)>,
}

pub fn placement(bounds: Option<Bounds>, monitors: &[Area]) -> Placement {
    let fallback = Placement {
        width: WIDTH,
        height: HEIGHT,
        position: None,
    };
    let Some(b) = bounds else {
        return fallback;
    };
    let finite = [b.x, b.y, b.width, b.height].iter().all(|v| v.is_finite());
    if !finite || b.width <= 0.0 || b.height <= 0.0 || b.width > MAX_SIDE || b.height > MAX_SIDE {
        return fallback;
    }
    let (grip_x, grip_y) = (b.x + GRIP_X.min(b.width), b.y + GRIP_Y);
    let visible = monitors.iter().any(|m| {
        grip_x >= m.x && grip_x < m.x + m.width && grip_y >= m.y && grip_y < m.y + m.height
    });
    Placement {
        width: b.width.max(MIN_WIDTH),
        height: b.height.max(MIN_HEIGHT),
        position: visible.then_some((b.x, b.y)),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PanelWindowError {
    #[error("Only the panel window may send panel messages")]
    WrongWindow,
    #[error("Invalid panel message: {0}")]
    Invalid(&'static str),
    #[error("Panel window: {0}")]
    Tauri(#[from] tauri::Error),
}

impl Serialize for PanelWindowError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl PanelMessage {
    /// Text without terminal sequences: tab and line breaks only. An ESC could end bracketed
    /// paste early (`ESC[201~`) and turn the rest into typed commands.
    pub fn validate(&self) -> Result<(), PanelWindowError> {
        match self {
            Self::SendToTerminal { text, .. } => {
                if text.is_empty() {
                    return Err(PanelWindowError::Invalid("empty text"));
                }
                if text.len() > MAX_TEXT_BYTES {
                    return Err(PanelWindowError::Invalid("text too large"));
                }
                if text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
                {
                    return Err(PanelWindowError::Invalid(
                        "text contains control characters",
                    ));
                }
                Ok(())
            }
            Self::NotesChanged { folder_key } => {
                if crate::notes::is_folder_key(folder_key) {
                    Ok(())
                } else {
                    Err(PanelWindowError::Invalid("unknown folder key"))
                }
            }
            Self::ShowMain {} | Self::ShowSession { .. } | Self::Ready {} => Ok(()),
        }
    }

    /// Messages after which the main window comes to the front.
    pub fn shows_main(&self) -> bool {
        matches!(self, Self::ShowMain {} | Self::ShowSession { .. })
    }
}

/// Only the window labelled `panel` may send; the main window has no use for it.
pub fn check_sender(label: &str) -> Result<(), PanelWindowError> {
    if label == LABEL {
        Ok(())
    } else {
        Err(PanelWindowError::WrongWindow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<PanelMessage, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn messages_are_a_closed_validated_set() {
        let send = parse(r#"{"kind":"sendToTerminal","text":"ls -la\n","sessionId":3}"#).unwrap();
        assert_eq!(
            send,
            PanelMessage::SendToTerminal {
                text: "ls -la\n".into(),
                session_id: Some(3)
            }
        );
        assert!(send.validate().is_ok());
        assert!(parse(r#"{"kind":"showMain"}"#).unwrap().validate().is_ok());
        assert!(parse(r#"{"kind":"ready"}"#).unwrap().validate().is_ok());
        let show = parse(r#"{"kind":"showSession","sessionId":7}"#).unwrap();
        assert_eq!(show, PanelMessage::ShowSession { session_id: 7 });
        assert!(show.validate().is_ok());
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
            r#"{"kind":"ready","sessionId":1}"#,
            r#"{"kind":"showSession"}"#,
            r#"{"kind":"showSession","sessionId":-1}"#,
            r#"{"kind":"showSession","sessionId":"1"}"#,
            r#"{"kind":"openFileViewer","sessionId":1,"path":"a"}"#,
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
        let big = PanelMessage::SendToTerminal {
            text: "x".repeat(MAX_TEXT_BYTES + 1),
            session_id: None,
        };
        assert!(big.validate().is_err());
        // The main window receives exactly what was validated.
        let json = serde_json::to_value(&send).unwrap();
        assert_eq!(json["kind"], "sendToTerminal");
        assert_eq!(json["sessionId"], 3);
        let json = serde_json::to_value(&show).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "showSession", "sessionId": 7})
        );
    }

    #[test]
    fn showing_a_session_brings_main_to_the_front() {
        assert!(PanelMessage::ShowMain {}.shows_main());
        assert!(PanelMessage::ShowSession { session_id: 1 }.shows_main());
        assert!(!PanelMessage::Ready {}.shows_main());
        assert!(!PanelMessage::NotesChanged {
            folder_key: "general".into()
        }
        .shows_main());
    }

    #[test]
    fn only_the_panel_window_sends() {
        assert!(check_sender("panel").is_ok());
        for other in ["main", "notes", "file-viewer-1", "Panel", ""] {
            assert!(
                matches!(check_sender(other), Err(PanelWindowError::WrongWindow)),
                "{other}"
            );
        }
    }

    const SCREEN: Area = Area {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };
    const LEFT: Area = Area {
        x: -1280.0,
        y: 0.0,
        width: 1280.0,
        height: 1024.0,
    };

    fn bounds(x: f64, y: f64, width: f64, height: f64) -> Option<Bounds> {
        Some(Bounds {
            x,
            y,
            width,
            height,
        })
    }

    #[test]
    fn remembered_bounds_are_kept_while_on_a_monitor() {
        let placed = placement(bounds(1400.0, 80.0, 500.0, 900.0), &[SCREEN]);
        assert_eq!(
            placed,
            Placement {
                width: 500.0,
                height: 900.0,
                position: Some((1400.0, 80.0))
            }
        );
        // A second monitor left of the primary one has negative coordinates.
        let placed = placement(bounds(-900.0, 40.0, 460.0, 680.0), &[SCREEN, LEFT]);
        assert_eq!(placed.position, Some((-900.0, 40.0)));
    }

    #[test]
    fn a_window_off_every_monitor_opens_at_the_default_place() {
        // The left monitor was unplugged: keep the size, let the system place it.
        let placed = placement(bounds(-900.0, 40.0, 460.0, 680.0), &[SCREEN]);
        assert_eq!(placed.position, None);
        assert_eq!((placed.width, placed.height), (460.0, 680.0));
        // Only the right edge still on screen is not enough to grab it.
        assert_eq!(
            placement(bounds(-440.0, 40.0, 460.0, 680.0), &[SCREEN]).position,
            None
        );
        // No monitor known at all.
        assert_eq!(
            placement(bounds(10.0, 10.0, 460.0, 680.0), &[]).position,
            None
        );
    }

    #[test]
    fn garbage_bounds_fall_back_to_the_default_size() {
        let fallback = Placement {
            width: WIDTH,
            height: HEIGHT,
            position: None,
        };
        assert_eq!(placement(None, &[SCREEN]), fallback);
        for bad in [
            bounds(f64::NAN, 0.0, 400.0, 400.0),
            bounds(0.0, 0.0, f64::INFINITY, 400.0),
            bounds(0.0, 0.0, 0.0, 400.0),
            bounds(0.0, 0.0, 400.0, -1.0),
            bounds(0.0, 0.0, 100_000.0, 400.0),
        ] {
            assert_eq!(placement(bad, &[SCREEN]), fallback, "{bad:?}");
        }
        // Too small grows to the minimum.
        let placed = placement(bounds(10.0, 10.0, 100.0, 100.0), &[SCREEN]);
        assert_eq!((placed.width, placed.height), (MIN_WIDTH, MIN_HEIGHT));
    }

    #[test]
    fn bounds_reject_unknown_fields() {
        let ok: Bounds =
            serde_json::from_str(r#"{"x":1,"y":2,"width":300.5,"height":400}"#).unwrap();
        assert_eq!(ok.width, 300.5);
        assert!(serde_json::from_str::<Bounds>(r#"{"x":1,"y":2,"width":3}"#).is_err());
        assert!(
            serde_json::from_str::<Bounds>(r#"{"x":1,"y":2,"width":3,"height":4,"z":5}"#).is_err()
        );
    }
}
