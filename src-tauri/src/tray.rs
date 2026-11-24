//! Icon in the notification area (contract: docs/wiki/pages/syntheses/agent-workflow-features.md,
//! "Tray").
//!
//! Off by default. While the icon exists, closing the main window only hides it
//! ([`on_window_event`], in the backend so the window cannot end up closed without an icon to
//! bring it back); the tray icon brings it back. A left click and the menu item Open show the
//! window, the menu item Quit emits `tray-quit` so the window saves and quits the usual way. The
//! texts come from the window, which owns all translations. While a shared project is connected
//! to its team server, the icon carries a small green dot on its lower right corner
//! ([`set_connected`]).

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use tauri::image::Image;
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
/// The dot's green (the app's `--ok`) and its ring, which keeps it apart on light and dark
/// taskbars alike.
const DOT_GREEN: [u8; 3] = [0x3f, 0xb9, 0x50];
const DOT_RING: [u8; 3] = [0x0e, 0x11, 0x16];

/// Whether a team is connected; read when the icon is (re)built, so turning the icon on while
/// connected shows the dot at once.
static CONNECTED: AtomicBool = AtomicBool::new(false);

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
                // Only the main window quits the app; no other window needs to hear it.
                let _ = app.emit_to(MAIN_WINDOW, QUIT_EVENT, ());
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
    if let Some(icon) = tray_icon(app, CONNECTED.load(Ordering::Relaxed)) {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// Shows or hides the green dot on the icon. Called with every team snapshot; only a change
/// redraws, and the icon itself is changed on the main thread, where the platform wants it.
pub fn set_connected<R: Runtime>(app: &AppHandle<R>, connected: bool) {
    if CONNECTED.swap(connected, Ordering::Relaxed) == connected {
        return;
    }
    let handle = app.clone();
    let result = app.run_on_main_thread(move || {
        // The flag may have changed again meanwhile; the icon follows its latest value.
        let connected = CONNECTED.load(Ordering::Relaxed);
        if let (Some(tray), Some(icon)) =
            (handle.tray_by_id(TRAY_ID), tray_icon(&handle, connected))
        {
            if let Err(error) = tray.set_icon(Some(icon)) {
                eprintln!("hangar: tray icon not updated: {error}");
            }
        }
    });
    if let Err(error) = result {
        eprintln!("hangar: tray icon not updated: {error}");
    }
}

/// The app icon, with the green dot while `connected`.
fn tray_icon<R: Runtime>(app: &AppHandle<R>, connected: bool) -> Option<Image<'static>> {
    let icon = app.default_window_icon()?;
    let (width, height) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    if connected {
        draw_dot(&mut rgba, width, height);
    }
    Some(Image::new_owned(rgba, width, height))
}

/// Paints the status dot into an RGBA image: a green disc with a dark ring on the lower right
/// corner, about two fifths of the edge across, its edge smoothed over one pixel.
fn draw_dot(rgba: &mut [u8], width: u32, height: u32) {
    let edge = f64::from(width.min(height));
    let outer = edge * 0.21;
    let ring = (edge * 0.045).max(1.0);
    let cx = f64::from(width) - outer;
    let cy = f64::from(height) - outer;
    for y in 0..height {
        for x in 0..width {
            let distance = (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy);
            let cover = (outer - distance + 0.5).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let inner = (outer - ring - distance + 0.5).clamp(0.0, 1.0);
            let colour: [f64; 3] = std::array::from_fn(|i| {
                f64::from(DOT_GREEN[i]) * inner + f64::from(DOT_RING[i]) * (1.0 - inner)
            });
            let index = ((y * width + x) * 4) as usize;
            let Some(pixel) = rgba.get_mut(index..index + 4) else {
                return;
            };
            let below = f64::from(pixel[3]) / 255.0;
            let alpha = cover + below * (1.0 - cover);
            for (channel, value) in pixel.iter_mut().take(3).zip(colour) {
                let mixed = value * cover + f64::from(*channel) * below * (1.0 - cover);
                *channel = (mixed / alpha.max(f64::EPSILON)).round().clamp(0.0, 255.0) as u8;
            }
            pixel[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
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
    fn the_dot_sits_green_on_the_lower_right_corner_only() {
        let (width, height) = (32u32, 32u32);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        draw_dot(&mut rgba, width, height);
        let pixel = |x: u32, y: u32| {
            let i = ((y * width + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        // Its centre is green and opaque, the ring around it dark.
        let centre = pixel(25, 25);
        assert_eq!(centre, [DOT_GREEN[0], DOT_GREEN[1], DOT_GREEN[2], 255]);
        // The rest of the icon stays untouched.
        assert_eq!(pixel(2, 2), [0, 0, 0, 0]);
        assert_eq!(pixel(16, 8), [0, 0, 0, 0]);
        assert_eq!(pixel(2, 30), [0, 0, 0, 0]);
    }

    #[test]
    fn the_dot_covers_an_opaque_icon_without_seams() {
        let (width, height) = (16u32, 16u32);
        let mut rgba = [255u8, 255, 255, 255].repeat((width * height) as usize);
        draw_dot(&mut rgba, width, height);
        assert!(rgba.chunks(4).all(|pixel| pixel[3] == 255));
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
