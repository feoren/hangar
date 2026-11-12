---
title: "tauri-plugin-single-instance (one Hangar per user session)"
type: component
tags: [tauri, plugin, windows, macos, linux, dependency, updater]
sources: ["[[single-instance-lock-measurement]]"]
version: 2.5.5
license: Apache-2.0 OR MIT
repo: https://github.com/tauri-apps/plugins-workspace
created: 2026-11-11
updated: 2026-11-12
---

# tauri-plugin-single-instance

Release builds of Hangar run once per user session (`src-tauri/src/single_instance.rs`,
registered first in `src-tauri/src/lib.rs`). A second start hands its command line to the running
Hangar and exits; the running Hangar shows, unminimizes and focuses its main window through
`tray::show_main_window`, the same path as the tray icon. Debug builds (`pnpm tauri dev`,
`tauri build --debug`) do not register the plugin (`single_instance::ENFORCED`), so they run next
to the installed app and next to each other, and they neither take nor look for the lock.

Hangar has no command line contract (no file associations, no deep links), so the forwarded
arguments and working folder are ignored.

## Behavior (read from the 2.5.5 sources in the Cargo registry, with tauri 2.12.5)

- **When it runs:** the plugin's `setup` runs in `Builder::build` (`initialize_plugins`), before
  the windows from `tauri.conf.json` are created and before the app's own `setup`. A second
  instance therefore exits before any window, session, team connection or file watcher exists.
  Plugins initialize in the order they were added, hence "register it first" in the README.
- **Windows:** `CreateMutexW` with the name `<identifier>-sim` (unprefixed, so in the session
  namespace: one instance per logged on user session). On `ERROR_ALREADY_EXISTS` it looks for the
  hidden message window `<identifier>-siw` (up to 40 tries of 25 ms, for two starts at once),
  calls `AllowSetForegroundWindow` for the first instance's process, so its `set_focus` may take
  the foreground, sends cwd and arguments NUL separated as `WM_COPYDATA` (10 s timeout), then
  `cleanup_before_exit` and `exit(0)`. The callback runs on the first instance's main thread
  (window procedure).
- **Linux:** the D-Bus name `<identifier>.SingleInstance` on the session bus (zbus 5). Without a
  session bus, or when connecting fails, it logs a warning and starts normally: then several
  instances are possible.
- **macOS:** a Unix socket `/tmp/<identifier with _>_si.sock`. A second instance connects, sends
  its arguments and exits; on `NotFound` or `ConnectionRefused` (a stale socket after a crash) it
  removes the file and becomes the first instance. Any other error starts normally.
- **Release of the lock:** the lock is a resource in the app's resource table; Tauri clears that
  table in `AppHandle::cleanup_before_exit`, which runs after the `RunEvent::Exit` handler. So the
  app stays the single instance while Hangar ends its sessions on exit (measured on Windows:
  [[single-instance-lock-measurement]]). When the process dies
  without it (crash, `std::process::exit`), the OS closes the mutex handle (Windows) and drops the
  D-Bus name (Linux); the macOS socket file stays and is cleaned up by the next start as above.

## Interplay with other parts of Hangar

- **Updater restart (macOS, Linux; on Windows the process ends earlier, next point):** `relaunch()` from `@tauri-apps/plugin-process` calls
  `AppHandle::request_restart`. In tauri 2.12.5 (`app.rs`, `RuntimeRunEvent::Exit`) the order is
  the Exit handler, `cleanup_before_exit` (releases the lock), then `process::restart` spawns the
  new process. The new process does not find the lock and starts. The direct `restart()` path also
  calls `cleanup_before_exit` first.
- **Update on Windows:** tauri-plugin-updater 2.14.1 (`updater.rs`, `install_inner`) starts the
  NSIS installer with `ShellExecuteW` (`/P /UPDATE`, plus `/R` to start the app again in passive
  mode) and then calls `std::process::exit(0)` at once, without `cleanup_before_exit`. The OS
  closes the mutex handle with the process, which is gone long before the installer has replaced
  the files and starts the new version, so the new version becomes the single instance
  (conclusion from the code, not measured with a real update).
- **Team:** the team's lock file in the config folder (`team_service.rs`) still guards against a
  second Hangar running a team; it shows `otherInstance`. With this plugin that only happens with
  a debug build next to a release build (same identifier, same config folder) or with the same
  person signed in to two Windows sessions, so the fallback stays.
- **Side panel window:** not involved; only the main window is brought to the front. If the
  person is looking at the detached side panel, the main window comes up in front of it.

## Unverified

> [!question] Unverified
> - Elevation on Windows: if one instance runs elevated, a normal start may get
>   `ERROR_ACCESS_DENIED` instead of `ERROR_ALREADY_EXISTS` and start a second instance, and UIPI
>   may drop the `WM_COPYDATA` from a lower integrity level. Not measured.
> - macOS: LaunchServices already reactivates a running bundle when it is opened from the Dock or
>   Finder, so the plugin mostly matters for starts from a terminal. The socket in `/tmp` is shared
>   by all users of the machine; whether another user's start reaches it depends on the socket's
>   permissions. Not measured.
> - A real update from a build without the plugin to one with it, and between two builds with it.
> - Linux under Flatpak or Snap needs `Builder::dbus_id` if the identifier differs from the
>   package id (README); Hangar ships neither.
> - Starting the installed release twice and from the tray was not measured in a session; to be
>   checked by hand with an installed release build.
