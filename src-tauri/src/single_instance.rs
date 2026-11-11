//! One Hangar per user session (behavior and platform notes: docs/wiki/pages/components/
//! single-instance.md).
//!
//! A second start of a release build never opens a second app: the plugin hands its command line
//! to the running Hangar and exits before any window, session or team connection exists, and the
//! running Hangar brings its main window to the front (shown from the tray, unminimized,
//! focused). The updater's restart is unaffected: Tauri releases the plugin's lock in
//! `cleanup_before_exit`, before it starts the new process.

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Runtime};

use crate::tray;

/// Whether this build allows only one instance. Debug builds (`pnpm tauri dev`, `tauri build
/// --debug`) are exempt on purpose: they must run next to the installed Hangar and next to each
/// other so a change can be looked at without quitting the app in daily use. They neither take
/// the lock nor look for it, so an installed Hangar started afterwards still starts.
pub const ENFORCED: bool = enforced(cfg!(debug_assertions));

pub const fn enforced(debug_build: bool) -> bool {
    !debug_build
}

/// The plugin, to be registered before every other plugin: plugins initialize in the order they
/// were added, and a second instance must exit before the others start their work.
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_single_instance::init(on_second_instance)
}

/// Hangar has no command line contract (no file associations, no deep links), so the forwarded
/// arguments and working folder are deliberately ignored rather than interpreted.
fn on_second_instance<R: Runtime>(app: &AppHandle<R>, _args: Vec<String>, _cwd: String) {
    tray::show_main_window(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_release_builds_are_single_instance() {
        assert!(enforced(false));
        assert!(!enforced(true));
        assert_eq!(ENFORCED, !cfg!(debug_assertions));
    }
}
