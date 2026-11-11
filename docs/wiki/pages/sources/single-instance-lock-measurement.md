---
title: "Measurement: single-instance lock on Windows"
type: source
tags: [tauri, plugin, windows, measurement]
sources: []
raw: raw/2026-11-11-single-instance-lock-measurement.md
url:
retrieved: 2026-11-11
created: 2026-11-11
updated: 2026-11-11
---

# Measurement: single-instance lock on Windows

The lock tests of tauri-plugin-single-instance 2.5.5 (`tests/instance_lock.rs`, MockRuntime,
tauri 2.12) run on Windows 11 (build 26300) with rustc 1.99.1: 3 passed, 1 helper ignored.

## Key points
- While a primary holds the lock, a second process with the same identifier is forwarded to it and
  exits.
- The lock is still held inside the `RunEvent::Exit` handler and is released once `App::run`
  returns (that is, after `cleanup_before_exit`).
- `cleanup_before_exit` and `destroy` each release the lock; a later start runs normally.

## Significance for Hangar
Hangar ends its sessions in the `RunEvent::Exit` handler, so it stays the single instance during
that shutdown, and the updater's restart (`request_restart`, which spawns the new process after
`cleanup_before_exit`) finds the lock free. See [[single-instance]].
