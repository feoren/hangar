---
title: "Linux desktop notifications over D-Bus"
type: source
tags: [notifications, linux, dbus, zbus]
sources: []
raw: raw/2026-11-18-freedesktop-notifications-measurement.md
url: https://specifications.freedesktop.org/notification/latest/
retrieved: 2026-11-18
created: 2026-11-18
updated: 2026-11-18
---

# Linux desktop notifications over D-Bus

Read in the Desktop Notifications Specification 1.3 (18 August 2024), the crate sources of
zbus 5.19.0 and tauri-plugin-notification 2.8.0 (cargo registry) and the crates.io metadata of
notify-rust 4.19.0. Built against a private session bus in WSL Ubuntu with a fake server.

## Key points
- `Notify(app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout) -> id`.
  A `replaces_id` of a shown notification replaces it atomically, the counterpart of the toast tag
  on Windows. `CloseNotification(id)` withdraws one.
- `actions` is a flat list of key and label; the key `default` is the click on the notification
  itself, its label may not be shown. `ActionInvoked(id, action_key)` reports the click. Servers
  without the capability `actions` may ignore actions; some never report clicks.
- Spec 1.2 added `ActivationToken(id, token)` before `ActionInvoked`, for raising a window on
  Wayland. Hangar does not pass it on (see open questions).
- Capability `body-markup`: the body is a small HTML subset. Text sent to such a server must be
  escaped (`&`, `<`, `>`), otherwise a terminal title could carry `<a href>` or `<img>`. The
  summary is always plain text.
- zbus 5.19 (MIT, z-galaxy, pure Rust, no libdbus) is already in the Linux tree with its default
  features through keyring and tauri-plugin-single-instance. `blocking::connection::Builder::
  method_timeout` makes a call fail with `TimedOut` instead of waiting for D-Bus activation.
  `blocking::MessageIterator::for_match_rule` adds the match rule on the bus.
- notify-rust 4.19 (MIT/Apache-2.0, maintained, default feature `z` = zbus) depends on zbus 6:
  a second D-Bus stack next to zbus 5 in the Linux build.
- tauri-plugin-notification 2.8 shows notifications through notify-rust; it has no per-session
  replace or withdraw and its own JavaScript API, a second path next to the WinRT one.

## Decision
`src-tauri/src/notify/dbus.rs` talks to `org.freedesktop.Notifications` with zbus directly:
no new crate, one connection and one signal thread for all clicks. The bookkeeping sits behind
the `Bus` trait in `src-tauri/src/notify/freedesktop.rs` and is unit tested on every platform.

## Behavior as built
- Title and body cleaned as on Windows (`clean_text`), body escaped only for `body-markup`,
  empty title becomes "Hangar". Capabilities are asked before every notification.
- One notification per session through `replaces_id`; closing a session closes it.
- Signals are believed only from the current owner of `org.freedesktop.Notifications`
  (`GetNameOwner` at connect, then `NameOwnerChanged`, itself believed only from
  `org.freedesktop.DBus`). A sender in the match rule would not do: zbus 5.19 does not check a
  well-known sender (`match_rule/mod.rs`, `matches`), and a signal sent straight to Hangar
  bypasses the bus's matching.
- `NotificationClosed` drops the session's id; a new server owner drops all ids, so no
  `replaces_id` of an old server reaches a new one.
- Two reader threads only parse and forward into a channel; one dispatch thread runs the click
  handlers, so a full signal queue cannot hold back method replies.
- A click (`default` action) brings the window to the front and switches to the session, only
  where the server has `actions`. Otherwise the notification only informs.
- No session bus or no server: `notify_session` returns `false`, the reason goes to stderr. Each
  call waits at most 1 s (`method_timeout`); measured in WSL with no server, the bus answered
  `ServiceUnknown` after 32 ms.

## Open questions
> [!question] Unverified
> Which common servers report the `default` action (GNOME Shell, KDE Plasma, dunst, mako,
> xfce4-notifyd are expected to); not tried on a real desktop.

> [!question] Unverified
> Whether a click raises the window on Wayland: compositors usually refuse focus without an
> activation token, so the window may only be marked as wanting attention.

> [!question] Unverified
> No icon is passed (`app_icon` empty); whether servers pick the icon from the .deb or AppImage
> desktop entry without the `desktop-entry` hint.

## Used in
- [[0013-linux-release]], [[agent-workflow-features]].
