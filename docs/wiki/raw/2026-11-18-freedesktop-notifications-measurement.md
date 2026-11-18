# Freedesktop notifications: specification excerpt and WSL measurement (2026-11-18)

## Specification
Desktop Notifications Specification, Version 1.3, 18 August 2024,
https://specifications.freedesktop.org/notification/latest/ (pages `protocol.html`,
`basic-design.html`, `markup.html`). Excerpts as read:

- `UINT32 org.freedesktop.Notifications.Notify (STRING app_name, UINT32 replaces_id, STRING
  app_icon, STRING summary, STRING body, as actions, a{sv} hints, INT32 expire_timeout);`
- replaces_id: "The server must atomically (ie with no flicker or other visual cues) replace the
  given notification with this one. [...] A value of value of 0 means that this notification won't
  replace any existing notifications."
- Actions: "The default action (usually invoked by clicking the notification) should have a key
  named "default". The name can be anything, though implementations are free not to display it."
- Capability "actions": "The server will provide the specified actions to the user. Even if this
  cap is missing, actions may still be specified by the client, however the server is free to
  ignore them."
- Capability "body-markup": "Supports markup in the body text. If marked up text is sent to a
  server that does not give this cap, the markup will show through as regular text so must be
  stripped clientside."
- `ActionInvoked (UINT32 id, STRING action_key)`: emitted when "The user performs some global
  "invoking" action upon a notification. For instance, clicking somewhere on the notification
  itself."
- `ActivationToken (UINT32 id, STRING activation_token)`: "This signal can be emitted before a
  ActionInvoked signal. It carries an activation token that can be used to activate a toplevel."

## Crates (read 2026-11-18)
- zbus 5.19.0: license MIT, repository github.com/z-galaxy/zbus; default features `async-io`,
  `blocking-api`; in `src-tauri/Cargo.lock` through zbus-secret-service-keyring-store (keyring)
  and tauri-plugin-single-instance.
- notify-rust 4.19.0 (crates.io API): MIT OR Apache-2.0, released 2026-11-08, default feature `z`
  = optional dependency `zbus ^6`.

## Measurement
Platform: WSL 2, Ubuntu, cargo 1.100.0. A standalone crate under `tests/.tmp/linux-notify/` built
`src-tauri/src/notify.rs`, `notify/freedesktop.rs`, `notify/dbus.rs` and `text.rs` unchanged
against zbus 5.19.0, then:

```
cargo test --lib                      -> 14 passed
cargo clippy --all-targets -- -D warnings -> clean
dbus-run-session -- cargo test --test live -- --nocapture
no server: Err(Service("org.freedesktop.DBus.Error.ServiceUnknown: The name org.freedesktop.Notifications was not provided by any .service files")) after 31.990823ms
test no_server_fails_fast_then_a_server_shows_and_reports_clicks ... ok
```

The live test serves a fake `org.freedesktop.Notifications` with zbus on the private bus: the
`Notify` call arrived as `("Hangar", 0, "Claude <b>", "is &amp; waiting", ["default", "Hangar"])`,
the second notification of the same session carried `replaces_id` 41, an emitted
`ActionInvoked(41, "default")` reached the newest click handler, an unknown id was ignored, and
`forget` sent `CloseNotification(41)`. No real desktop notification server was tried.

## Second run (sender check, closed and restarted servers)
Same setup after the sender check: `cargo test --lib` 19 passed, clippy clean, live test passed
in 0.97 s. Added to the live test: an `ActionInvoked(41, "default")` broadcast from a third
connection was ignored; after the server's `NotificationClosed(id, 2)` the session's next `Notify`
carried `replaces_id` 0; after the server's connection was dropped and a new server took the
name, its first `Notify` carried `replaces_id` 0.
