//! Size, place and maximized state of the main window across restarts (ADR 0007, section "Main
//! window state").
//!
//! The window comes from `tauri.conf.json` hidden (`"visible": false`); this module puts it where
//! it was and then shows it, so it does not jump on screen. The state lives in a file in the local
//! app data folder, not in `localStorage`, which the panel and file viewer windows share with the
//! main window (see `process-launch-hardening` in the wiki). The file is written atomically and
//! validated on reading; a damaged file gives the default size, centered.
//!
//! What is stored: whether the window was maximized, and its last normal (not maximized) size and
//! place in logical pixels, so "Restore" after a maximized start brings back a sensible window.
//! A minimized window is never stored as such: its place is the one before minimizing.
//!
//! Saved after a move or resize (once the window has been quiet for [`SAVE_DELAY`]), on a close
//! request and when the app exits. The state is read from the window only after the delay: on
//! Windows the move event of maximizing arrives before the window counts as maximized, so a
//! reading at that event would store the maximized rectangle as the normal one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{
    AppHandle, Manager as _, PhysicalPosition, PhysicalSize, RunEvent, Runtime, WebviewWindow,
    WindowEvent,
};

use crate::panel_window::{self, Area, Bounds, Limits};
use crate::persist::{read_limited, write_atomic};

pub const FILE_NAME: &str = "main-window.json";
/// The sizes of the main window in `tauri.conf.json` (a test keeps them equal).
pub const LIMITS: Limits = Limits {
    width: 1280.0,
    height: 800.0,
    min_width: 720.0,
    min_height: 420.0,
};
/// Quiet time after the last move or resize before the state is read and written: a drag writes
/// the file a few times, not on every pixel.
pub const SAVE_DELAY: Duration = Duration::from_millis(500);
/// Positions beyond this are garbage, not a wall of monitors.
const MAX_COORD: f64 = 1_000_000.0;
/// The JSON of one state is below 200 bytes.
const MAX_BYTES: usize = 4 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum MainWindowError {
    #[error("File {path} cannot be written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Window state cannot be encoded: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("Main window: {0}")]
    Tauri(#[from] tauri::Error),
}

/// What the file holds.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowState {
    /// Last normal size and place in logical pixels; `None` until the window was seen unmaximized.
    pub normal: Option<Bounds>,
    pub maximized: bool,
}

/// Whether `state` may be used: normal bounds plausible ([`panel_window::plausible`]) and not
/// absurdly far away.
pub fn is_valid(state: &WindowState) -> bool {
    state.normal.is_none_or(|b| {
        panel_window::plausible(&b) && b.x.abs() <= MAX_COORD && b.y.abs() <= MAX_COORD
    })
}

/// The stored state; a damaged or implausible file counts as nothing stored.
pub fn parse(bytes: &[u8]) -> WindowState {
    serde_json::from_slice::<WindowState>(bytes)
        .ok()
        .filter(is_valid)
        .unwrap_or_default()
}

/// One reading of the window, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub minimized: bool,
    pub maximized: bool,
    pub bounds: Bounds,
}

/// The state after a reading: a minimized window changes nothing, a maximized one keeps the last
/// normal bounds, a normal one replaces them (unless the reading is garbage).
pub fn observe(previous: WindowState, sample: Sample) -> WindowState {
    if sample.minimized {
        return previous;
    }
    if sample.maximized {
        return WindowState {
            normal: previous.normal,
            maximized: true,
        };
    }
    let reading = WindowState {
        normal: Some(sample.bounds),
        maximized: false,
    };
    if is_valid(&reading) {
        reading
    } else {
        WindowState {
            normal: previous.normal,
            maximized: false,
        }
    }
}

/// A monitor in logical pixels with its scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub area: Area,
    pub scale: f64,
}

/// Where the window goes at the start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Logical size.
    pub width: f64,
    pub height: f64,
    /// The remembered place and the scale of its monitor; `None`: centered at the default size.
    pub spot: Option<Spot>,
    pub maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

/// The start plan: the remembered bounds while their title bar lies on a monitor
/// ([`panel_window::placement_within`]), at most as large as that monitor; otherwise the default
/// size, centered. Maximized again when it was.
pub fn plan(state: WindowState, screens: &[Screen]) -> Plan {
    let areas: Vec<Area> = screens.iter().map(|s| s.area).collect();
    let placed = panel_window::placement_within(state.normal, &areas, LIMITS);
    let screen = state
        .normal
        .and_then(|b| panel_window::monitor_of(&b, &areas))
        .and_then(|i| screens.get(i));
    match (placed.position, screen) {
        (Some((x, y)), Some(screen)) => Plan {
            width: placed.width.min(screen.area.width.max(LIMITS.min_width)),
            height: placed.height.min(screen.area.height.max(LIMITS.min_height)),
            spot: Some(Spot {
                x,
                y,
                scale: screen.scale,
            }),
            maximized: state.maximized,
        },
        _ => Plan {
            width: LIMITS.width,
            height: LIMITS.height,
            spot: None,
            maximized: state.maximized,
        },
    }
}

/// The state in memory and its file. Shared with the delayed save thread.
#[derive(Debug)]
pub struct MainWindowStore {
    /// `None`: no data folder known; the state then lives for this run only.
    path: Option<PathBuf>,
    state: Mutex<WindowState>,
    /// What the file holds; also serializes the writes.
    written: Mutex<WindowState>,
    save_pending: AtomicBool,
    restored: AtomicBool,
}

impl MainWindowStore {
    /// Reads the file in `dir`; nothing stored or a damaged file gives the defaults.
    pub fn open(dir: Option<&Path>) -> Self {
        let path = dir.map(|dir| dir.join(FILE_NAME));
        let state = path
            .as_deref()
            .and_then(|path| read_limited(path, MAX_BYTES).ok().flatten())
            .map(|bytes| parse(&bytes))
            .unwrap_or_default();
        Self {
            path,
            state: Mutex::new(state),
            written: Mutex::new(state),
            save_pending: AtomicBool::new(false),
            restored: AtomicBool::new(false),
        }
    }

    pub fn state(&self) -> WindowState {
        *crate::lock(&self.state)
    }

    pub fn observe(&self, sample: Sample) {
        let mut state = crate::lock(&self.state);
        *state = observe(*state, sample);
    }

    /// Writes the state when it differs from what the file holds.
    pub fn save(&self) -> Result<(), MainWindowError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut written = crate::lock(&self.written);
        let state = self.state();
        if *written == state {
            return Ok(());
        }
        let json = serde_json::to_vec(&state)?;
        write_atomic(path, &json).map_err(|source| MainWindowError::Io {
            path: path.clone(),
            source,
        })?;
        *written = state;
        Ok(())
    }
}

/// Registers the store, restores the main window before it is shown and saves its state. A
/// plugin, so the run loop and the window events of `lib.rs` stay as they are.
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("main-window")
        .setup(|app, _api| {
            let dir = app.path().app_local_data_dir();
            if let Err(error) = &dir {
                eprintln!("hangar: main window state not remembered: {error}");
            }
            app.manage(Arc::new(MainWindowStore::open(dir.ok().as_deref())));
            Ok(())
        })
        .on_window_ready(|window| {
            if window.label() == panel_window::MAIN_LABEL {
                restore_once(window.app_handle());
            }
        })
        .on_event(|app, event| match event {
            // A fallback: the window must become visible even if its ready hook never ran.
            RunEvent::Ready => {
                restore_once(app);
                ensure_visible(app);
            }
            RunEvent::WindowEvent { label, event, .. } if label == panel_window::MAIN_LABEL => {
                match event {
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) => schedule_save(app),
                    WindowEvent::CloseRequested { .. } => save_now(app),
                    _ => {}
                }
            }
            RunEvent::ExitRequested { .. } | RunEvent::Exit => save_now(app),
            _ => {}
        })
        .build()
}

fn store<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<MainWindowStore>> {
    app.try_state::<Arc<MainWindowStore>>()
        .map(|store| store.inner().clone())
}

/// Places and then shows the main window, once. The window is shown whatever fails before.
fn restore_once<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(panel_window::MAIN_LABEL) else {
        return;
    };
    let store = store(app);
    if let Some(store) = &store {
        if store.restored.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Err(error) = apply(&window, store.state()) {
            eprintln!("hangar: main window not restored: {error}");
        }
    }
    if let Err(error) = window.show() {
        eprintln!("hangar: main window not shown: {error}");
    }
}

/// Last line of defense at startup: nothing before may leave the main window hidden, not even a
/// panic in the restore.
fn ensure_visible<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(panel_window::MAIN_LABEL) {
        if matches!(window.is_visible(), Ok(false)) {
            let _ = window.show();
        }
    }
}

/// Position first, then size: moving onto a monitor with another scale factor may resize the
/// window, and the size is meant in that monitor's pixels.
fn apply<R: Runtime>(window: &WebviewWindow<R>, state: WindowState) -> Result<(), MainWindowError> {
    let plan = plan(state, &screens(window));
    match plan.spot {
        Some(spot) => {
            window.set_position(PhysicalPosition::new(
                (spot.x * spot.scale).round(),
                (spot.y * spot.scale).round(),
            ))?;
            window.set_size(PhysicalSize::new(
                (plan.width * spot.scale).round(),
                (plan.height * spot.scale).round(),
            ))?;
        }
        None => {
            window.set_size(tauri::LogicalSize::new(plan.width, plan.height))?;
            window.center()?;
        }
    }
    // While hidden this only marks the window; showing it then opens it maximized without first
    // drawing it at its normal size (tao 0.37, `WindowFlags::apply_diff` on Windows).
    if plan.maximized {
        window.maximize()?;
    }
    Ok(())
}

fn screens<R: Runtime>(window: &WebviewWindow<R>) -> Vec<Screen> {
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| {
            let scale = monitor.scale_factor().max(0.1);
            let position = monitor.position();
            let size = monitor.size();
            Screen {
                area: Area {
                    x: f64::from(position.x) / scale,
                    y: f64::from(position.y) / scale,
                    width: f64::from(size.width) / scale,
                    height: f64::from(size.height) / scale,
                },
                scale,
            }
        })
        .collect()
}

/// `last` gives the place where the window cannot tell its own (Wayland), so the size still
/// counts.
fn sample<R: Runtime>(window: &WebviewWindow<R>, last: Option<Bounds>) -> Option<Sample> {
    let scale = window.scale_factor().ok()?.max(0.1);
    let size = window.inner_size().ok()?;
    let (x, y) = match window.outer_position() {
        Ok(position) => (f64::from(position.x) / scale, f64::from(position.y) / scale),
        Err(_) => last.map_or((0.0, 0.0), |b| (b.x, b.y)),
    };
    Some(Sample {
        minimized: window.is_minimized().ok()?,
        maximized: window.is_maximized().ok()?,
        bounds: Bounds {
            x,
            y,
            width: f64::from(size.width) / scale,
            height: f64::from(size.height) / scale,
        },
    })
}

/// Reads the window now and writes the file. On the main thread at close and exit.
fn save_now<R: Runtime>(app: &AppHandle<R>) {
    let Some(store) = store(app) else {
        return;
    };
    // Before the first restore the window still has the config's size, not the user's.
    if !store.restored.load(Ordering::SeqCst) {
        return;
    }
    let last = store.state().normal;
    if let Some(sample) = app
        .get_webview_window(panel_window::MAIN_LABEL)
        .and_then(|window| sample(&window, last))
    {
        store.observe(sample);
    }
    if let Err(error) = store.save() {
        eprintln!("hangar: main window state not saved: {error}");
    }
}

/// One delayed save per quiet phase: further events while it waits fold into it.
fn schedule_save<R: Runtime>(app: &AppHandle<R>) {
    let Some(store) = store(app) else {
        return;
    };
    if !store.restored.load(Ordering::SeqCst) || store.save_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("main-window-save".to_owned())
        .spawn(move || {
            std::thread::sleep(SAVE_DELAY);
            store.save_pending.store(false, Ordering::SeqCst);
            save_now(&app);
        });
    if let Err(error) = spawned {
        eprintln!("hangar: main window state not saved: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

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

    fn screen(area: Area, scale: f64) -> Screen {
        Screen { area, scale }
    }

    fn bounds(x: f64, y: f64, width: f64, height: f64) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    fn sample(minimized: bool, maximized: bool, b: Bounds) -> Sample {
        Sample {
            minimized,
            maximized,
            bounds: b,
        }
    }

    #[test]
    fn a_maximized_window_comes_back_maximized_after_a_restart() {
        let dir = TempDir::new("main-window-restart");
        let store = MainWindowStore::open(Some(dir.path()));
        assert_eq!(store.state(), WindowState::default());
        store.observe(sample(false, false, bounds(100.0, 50.0, 1500.0, 900.0)));
        store.observe(sample(false, true, bounds(-8.0, -8.0, 1936.0, 1056.0)));
        store.save().unwrap();

        let next = MainWindowStore::open(Some(dir.path()));
        let state = next.state();
        assert!(state.maximized);
        // The normal bounds from before maximizing, for "Restore".
        assert_eq!(state.normal, Some(bounds(100.0, 50.0, 1500.0, 900.0)));
        let plan = plan(state, &[screen(SCREEN, 1.0)]);
        assert!(plan.maximized);
        assert_eq!((plan.width, plan.height), (1500.0, 900.0));
        assert_eq!(
            plan.spot,
            Some(Spot {
                x: 100.0,
                y: 50.0,
                scale: 1.0
            })
        );
    }

    #[test]
    fn size_and_place_of_a_normal_window_survive_a_restart() {
        let dir = TempDir::new("main-window-normal");
        let store = MainWindowStore::open(Some(dir.path()));
        store.observe(sample(false, false, bounds(-1000.0, 20.0, 900.0, 700.0)));
        store.save().unwrap();
        let state = MainWindowStore::open(Some(dir.path())).state();
        assert_eq!(
            state,
            WindowState {
                normal: Some(bounds(-1000.0, 20.0, 900.0, 700.0)),
                maximized: false
            }
        );
        let plan = plan(state, &[screen(SCREEN, 1.0), screen(LEFT, 1.25)]);
        assert_eq!(
            plan.spot,
            Some(Spot {
                x: -1000.0,
                y: 20.0,
                scale: 1.25
            })
        );
        assert!(!plan.maximized);
    }

    #[test]
    fn a_minimized_window_is_never_stored_as_such() {
        let before = WindowState {
            normal: Some(bounds(10.0, 10.0, 1000.0, 700.0)),
            maximized: true,
        };
        // Windows reports a minimized window far off screen.
        let after = observe(
            before,
            sample(true, false, bounds(-32000.0, -32000.0, 160.0, 28.0)),
        );
        assert_eq!(after, before);
    }

    #[test]
    fn a_garbage_reading_keeps_the_last_normal_bounds() {
        let before = WindowState {
            normal: Some(bounds(10.0, 10.0, 1000.0, 700.0)),
            maximized: true,
        };
        for bad in [
            bounds(f64::NAN, 0.0, 800.0, 600.0),
            bounds(0.0, 0.0, 0.0, 600.0),
            bounds(5e7, 0.0, 800.0, 600.0),
        ] {
            let after = observe(before, sample(false, false, bad));
            assert_eq!(after.normal, before.normal, "{bad:?}");
            assert!(!after.maximized);
        }
    }

    #[test]
    fn a_damaged_file_gives_the_default() {
        assert_eq!(
            parse(br#"{"normal":{"x":1,"y":2,"width":800,"height":600},"maximized":true}"#),
            WindowState {
                normal: Some(bounds(1.0, 2.0, 800.0, 600.0)),
                maximized: true
            }
        );
        assert_eq!(
            parse(br#"{"normal":null,"maximized":true}"#),
            WindowState {
                normal: None,
                maximized: true
            }
        );
        for bad in [
            &b""[..],
            b"not json",
            br#"{"maximized":"yes"}"#,
            br#"{"normal":null,"maximized":false,"minimized":true}"#,
            br#"{"normal":{"x":1,"y":2,"width":800},"maximized":false}"#,
            br#"{"normal":{"x":1,"y":2,"width":-5,"height":600},"maximized":true}"#,
            br#"{"normal":{"x":1,"y":2,"width":100000,"height":600},"maximized":true}"#,
            br#"{"normal":{"x":1e9,"y":2,"width":800,"height":600},"maximized":true}"#,
        ] {
            assert_eq!(
                parse(bad),
                WindowState::default(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }

        let dir = TempDir::new("main-window-damaged");
        std::fs::write(dir.path().join(FILE_NAME), b"{ broken").unwrap();
        assert_eq!(
            MainWindowStore::open(Some(dir.path())).state(),
            WindowState::default()
        );
        // Without a data folder: nothing read, nothing written.
        assert_eq!(MainWindowStore::open(None).state(), WindowState::default());
        assert!(MainWindowStore::open(None).save().is_ok());
    }

    #[test]
    fn a_window_off_every_monitor_opens_centered_at_the_default_size() {
        let state = WindowState {
            normal: Some(bounds(-1000.0, 20.0, 900.0, 700.0)),
            maximized: true,
        };
        // The left monitor was unplugged.
        let plan = plan(state, &[screen(SCREEN, 1.0)]);
        assert_eq!(
            plan,
            Plan {
                width: LIMITS.width,
                height: LIMITS.height,
                spot: None,
                maximized: true
            }
        );
        // No monitor known at all, and nothing stored.
        assert_eq!(plan_default(&[]), plan_default(&[screen(SCREEN, 1.0)]));
        assert_eq!(plan_default(&[]).spot, None);
    }

    fn plan_default(screens: &[Screen]) -> Plan {
        plan(WindowState::default(), screens)
    }

    #[test]
    fn the_size_fits_the_monitor_and_the_minimum() {
        // Remembered on a larger monitor that now runs at a lower resolution.
        let plan_big = plan(
            WindowState {
                normal: Some(bounds(0.0, 0.0, 3000.0, 2000.0)),
                maximized: false,
            },
            &[screen(SCREEN, 1.0)],
        );
        assert_eq!((plan_big.width, plan_big.height), (1920.0, 1040.0));
        let plan_small = plan(
            WindowState {
                normal: Some(bounds(10.0, 10.0, 100.0, 100.0)),
                maximized: false,
            },
            &[screen(SCREEN, 1.0)],
        );
        assert_eq!(
            (plan_small.width, plan_small.height),
            (LIMITS.min_width, LIMITS.min_height)
        );
    }

    #[test]
    fn saving_twice_writes_once_and_without_a_folder_nothing() {
        let dir = TempDir::new("main-window-unchanged");
        let store = MainWindowStore::open(Some(dir.path()));
        // Nothing changed: no file yet.
        store.save().unwrap();
        assert!(!dir.path().join(FILE_NAME).exists());
        store.observe(sample(false, false, bounds(1.0, 2.0, 800.0, 600.0)));
        store.save().unwrap();
        assert!(dir.path().join(FILE_NAME).exists());
    }

    /// The window starts hidden so the restore does not show it jumping, and the defaults here are
    /// the config's.
    #[test]
    fn the_config_window_starts_hidden_with_these_sizes() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let windows = conf["app"]["windows"].as_array().unwrap();
        let main = windows
            .iter()
            .find(|w| w["label"] == panel_window::MAIN_LABEL)
            .unwrap();
        assert_eq!(main["visible"], serde_json::json!(false));
        assert_eq!(main["width"].as_f64(), Some(LIMITS.width));
        assert_eq!(main["height"].as_f64(), Some(LIMITS.height));
        assert_eq!(main["minWidth"].as_f64(), Some(LIMITS.min_width));
        assert_eq!(main["minHeight"].as_f64(), Some(LIMITS.min_height));
    }
}
