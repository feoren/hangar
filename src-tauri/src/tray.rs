//! Icon in the notification area (contract: docs/wiki/pages/syntheses/agent-workflow-features.md,
//! "Tray").
//!
//! Off by default. While the icon exists, closing the main window only hides it
//! ([`on_window_event`], in the backend so the window cannot end up closed without an icon to
//! bring it back); the tray icon brings it back. A left click and the menu item Open show the
//! window, the menu item Quit emits `tray-quit` so the window saves and quits the usual way. The
//! texts come from the window, which owns all translations.

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter as _, Manager as _, Runtime, Window, WindowEvent};

pub const TRAY_ID: &str = "main";
pub const QUIT_EVENT: &str = "tray-quit";
const MAIN_WINDOW: &str = "main";
const OPEN_ITEM: &str = "open";
const QUIT_ITEM: &str = "quit";
/// Menu texts and tooltip are UI texts; a longer one is a mistake, not a translation.
const MAX_LABEL_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrayLabels {
    pub open: String,
    pub quit: String,
    pub tooltip: String,
}

impl TrayLabels {
    /// One line each, without control characters, at most [`MAX_LABEL_CHARS`]; empty texts fall
    /// back to English so the menu never shows blank items.
    pub fn cleaned(&self) -> Self {
        let clean = |text: &str, fallback: &str| {
            let text: String = text
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .take(MAX_LABEL_CHARS)
                .collect();
            let text = text.trim();
            if text.is_empty() {
                fallback.to_owned()
            } else {
                text.to_owned()
            }
        };
        Self {
            open: clean(&self.open, "Open Hangar"),
            quit: clean(&self.quit, "Quit"),
            tooltip: clean(&self.tooltip, "Hangar"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("Notification area icon failed: {0}")]
    Tauri(#[from] tauri::Error),
}

impl serde::Serialize for TrayError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Shows, unminimizes and focuses the main window.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        // Each step may fail harmlessly (already visible, not minimized).
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Whether closing should only hide the window: the main window, while the icon exists.
pub fn hides_on_close(label: &str, tray_present: bool) -> bool {
    label == MAIN_WINDOW && tray_present
}

/// Close request of a window: hidden instead of closed while the icon exists. The window's own
/// close handler still receives the request afterwards (Tauri emits it to the frontend in any
/// case) and must then neither close nor quit.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let tray_present = window.app_handle().tray_by_id(TRAY_ID).is_some();
        if hides_on_close(window.label(), tray_present) {
            api.prevent_close();
            if let Err(error) = window.hide() {
                eprintln!("hangar: window not hidden: {error}");
            }
        }
    }
}

/// Adds, updates or removes the icon. Updating replaces the menu and the tooltip (another
/// language), so calling it again with the same state is harmless.
pub fn configure<R: Runtime>(
    app: &AppHandle<R>,
    enabled: bool,
    labels: &TrayLabels,
) -> Result<(), TrayError> {
    if !enabled {
        // The icon disappears when the last handle to it is dropped.
        drop(app.remove_tray_by_id(TRAY_ID));
        return Ok(());
    }
    let labels = labels.cleaned();
    let open = MenuItem::with_id(app, OPEN_ITEM, &labels.open, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_ITEM, &labels.quit, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
        tray.set_tooltip(Some(&labels.tooltip))?;
        return Ok(());
    }
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(&labels.tooltip)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN_ITEM => show_main_window(app),
            QUIT_ITEM => {
                let _ = app.emit(QUIT_EVENT, ());
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_one_short_line() {
        let labels = TrayLabels {
            open: "Hangar öffnen".into(),
            quit: "Beenden\r\n".into(),
            tooltip: "x".repeat(200),
        }
        .cleaned();
        assert_eq!(labels.open, "Hangar öffnen");
        assert_eq!(labels.quit, "Beenden");
        assert_eq!(labels.tooltip.chars().count(), MAX_LABEL_CHARS);
        let empty = TrayLabels {
            open: " ".into(),
            quit: String::new(),
            tooltip: "\t".into(),
        }
        .cleaned();
        assert_eq!(
            (
                empty.open.as_str(),
                empty.quit.as_str(),
                empty.tooltip.as_str()
            ),
            ("Open Hangar", "Quit", "Hangar")
        );
    }

    #[test]
    fn only_the_main_window_hides_and_only_with_an_icon() {
        assert!(hides_on_close("main", true));
        assert!(!hides_on_close("main", false));
        assert!(!hides_on_close("other", true));
    }

    #[test]
    fn labels_arrive_as_in_the_contract() {
        let labels: TrayLabels = serde_json::from_value(serde_json::json!({
            "open": "Open", "quit": "Quit", "tooltip": "Hangar"
        }))
        .unwrap();
        assert_eq!(labels.open, "Open");
        assert!(serde_json::from_value::<TrayLabels>(serde_json::json!({ "open": "x" })).is_err());
    }
}
