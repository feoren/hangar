---
title: "Windows toast notifications from a Tauri app"
type: source
tags: [notifications, windows, tauri]
sources: []
raw:
url: https://crates.io/crates/tauri-plugin-notification
retrieved: 2026-11-08
created: 2026-11-08
updated: 2026-11-08
---

# Windows toast notifications from a Tauri app

Read in the crate sources (cargo registry) of tauri-plugin-notification 2.8.0,
tauri-winrt-notification 0.8.1 and windows-core 0.62.2.

## Key points
- tauri-plugin-notification 2.8.0 shows toasts through notify-rust, which uses
  tauri-winrt-notification on Windows.
- App id rule (`desktop.rs`, `build()`): the bundle identifier is set as AppUserModelID only when
  the executable does not lie in `...\target\debug` or `...\target\release`. Otherwise the default
  applies, `Toast::POWERSHELL_APP_ID`, so the toast names Windows PowerShell as its origin.
- tauri-winrt-notification 0.8.1 has no public setter for a toast tag or group; it sets a tag only
  together with a progress bar. A "one toast per session" rule therefore needs the WinRT API
  directly (`ToastNotification::SetTag`, `SetGroup`); the `windows` crate is already in Tauri's
  dependency tree.
- `Toast::show` registers the `Activated` handler on the `ToastNotification` and drops it after
  `Show`. Hangar keeps the last toast per session alive instead, so the click handler lives as long
  as the toast can be clicked.
- windows-core 0.62.2 (`imp/factory_cache.rs`) retries a WinRT activation that fails with
  `CO_E_NOTINITIALIZED` after `CoIncrementMTAUsage`, so toasts work from a worker thread without
  its own COM initialization.

## Open questions
> [!question] Unverified
> That the NSIS installer registers the bundle identifier as AppUserModelID on its Start menu
> shortcut, which an installed app needs for its toasts to appear under its own name.

> [!question] Unverified
> Whether a click on a toast that has already moved to the notification center still reaches the
> in-process `Activated` handler of an unpackaged app.

## Used in
- [[agent-workflow-features]]: `notify_session` and the `notification-activated` event.
