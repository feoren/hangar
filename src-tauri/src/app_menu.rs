//! The application menu on macOS (ADR 0017).
//!
//! Tauri gives a macOS app a default menu bar whose Quit item (Cmd+Q) sends `terminate:` to the
//! application. That ends the process straight away: the window's close handler never runs, so
//! Hangar would neither save the workspace nor ask before ending running sessions, as it does
//! for closing the window and for Quit in the notification area. This menu is the default one
//! with only that item replaced: an own Quit with the same shortcut that shows the main window
//! and asks it to quit the usual way ([`crate::tray::QUIT_EVENT`]), so a question about running
//! sessions is never asked in a hidden window. Windows and Linux get no menu bar.
//!
//! The functions compile on every platform, so clippy and the tests check them everywhere; only
//! [`crate::run`] on macOS installs them.

use tauri::menu::{Menu, MenuEvent, MenuItem, MenuItemKind};
use tauri::{AppHandle, Emitter as _, Runtime};

/// Id of the own Quit item; distinct from the tray menu's `quit`, because every menu listener
/// of the app sees every menu event.
pub const QUIT_ITEM: &str = "app-quit";
/// The shortcut macOS users expect for Quit.
pub const QUIT_ACCELERATOR: &str = "CmdOrCtrl+Q";
const MAIN_WINDOW: &str = "main";

/// Tauri's default menu with the Quit item of the application submenu (the first one, Quit as
/// its last item in tauri 2.12 `Menu::default`) replaced by the own one.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;
    let Some(MenuItemKind::Submenu(application)) = menu.items()?.into_iter().next() else {
        return Ok(menu);
    };
    let items = application.items()?;
    if let (Some(last), Some(MenuItemKind::Predefined(_))) =
        (items.len().checked_sub(1), items.last())
    {
        application.remove_at(last)?;
    }
    let name = app.package_info().name.clone();
    let quit = MenuItem::with_id(
        app,
        QUIT_ITEM,
        quit_label(&name),
        true,
        Some(QUIT_ACCELERATOR),
    )?;
    application.append(&quit)?;
    Ok(menu)
}

/// The text of the Quit item, as macOS names it in every app ("Quit <name>"). The menu bar
/// exists before the window and its translations, so it stays in English like the rest of
/// Tauri's default menu.
pub fn quit_label(name: &str) -> String {
    format!("Quit {name}")
}

/// Whether a menu event is the own Quit item.
pub fn is_quit(id: &str) -> bool {
    id == QUIT_ITEM
}

/// Menu listener: the own Quit item lets the main window quit the usual way.
pub fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if is_quit(event.id().as_ref()) {
        crate::tray::show_main_window(app);
        let _ = app.emit_to(MAIN_WINDOW, crate::tray::QUIT_EVENT, ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_is_the_own_item_only() {
        assert!(is_quit(QUIT_ITEM));
        // The tray menu's Quit has its own listener; this one must not answer it a second time.
        assert!(!is_quit("quit"));
        assert!(!is_quit(""));
    }

    #[test]
    fn quit_keeps_the_macos_name_and_shortcut() {
        assert_eq!(quit_label("Hangar"), "Quit Hangar");
        assert_eq!(QUIT_ACCELERATOR, "CmdOrCtrl+Q");
    }
}
