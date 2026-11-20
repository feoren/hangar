---
title: Launcher entry and macOS usage reading
type: source
tags: [linux, macos, badge, launcher, resources, reading]
sources: []
raw: raw/2026-11-20-launcher-entry-and-macos-usage-reading.md
url: https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/linux/freedesktop/mod.rs
retrieved: 2026-11-20
created: 2026-11-20
updated: 2026-11-20
---

# Launcher entry and macOS usage reading

Which desktop id the Linux launcher count must name, and which macOS calls give the CPU and
memory of the machine and of each session.

**Key points.**

- The Tauri bundler writes the .deb's desktop file as `<productName>.desktop`, here
  `Hangar.desktop`; Tauri's own badge count names the same id.
- GLib launchers set `GIO_LAUNCHED_DESKTOP_FILE` (path of the desktop file) and
  `GIO_LAUNCHED_DESKTOP_FILE_PID` (the launched process), the latter documented as the way to
  ignore a variable inherited by further processes.
- What KDE Plasma's launcher sets, and AppImageLauncher's file naming, are not from a fetched
  source.
- libc 0.2.191 binds everything macOS needs: `host_statistics` (CPU ticks), `host_statistics64`
  (page counts), `sysctlbyname`, `proc_listallpids`, `proc_pidinfo(PROC_PIDTBSDINFO)` (parent,
  no session id), `proc_pid_rusage(RUSAGE_INFO_V2)` (own and reaped children's CPU time, resident
  size) and `getsid`. `mach_host_self` and `mach_timebase_info` are marked deprecated in favor of
  the mach2 crate.

**Significance for Hangar.** The desktop id order in `src-tauri/src/launcher_badge.rs`
([[0013-linux-release]]) and the macOS readers in `src-tauri/src/system_usage.rs` and
`src-tauri/src/proctree.rs` ([[session-resources-and-phone-push]], [[0017-macos-release]]).
