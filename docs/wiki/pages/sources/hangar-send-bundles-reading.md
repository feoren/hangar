---
title: "Reading: hangar-send in the Linux and macOS bundles"
type: source
tags: [reading, ci, bundling, linux, macos, appimage, deb, dmg, path]
sources: []
raw: raw/2026-11-22-hangar-send-bundles-reading.md
url:
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Reading: hangar-send in the Linux and macOS bundles

The artifacts of CI run 45043329057 (commit `2200d42`, jobs `linux-bundle` and `macos-bundle`
green) downloaded and listed, plus the CI checks and the code path that hands the client to a
session. Nothing was run. Record: `raw/2026-11-22-hangar-send-bundles-reading.md`.

## Key points

- **Linux AppImage:** `usr/bin/hangar-send` next to `usr/bin/hangar` in the SquashFS.
- **Linux .deb:** `usr/bin/hangar-send` with mode `rwxr-xr-x` next to `usr/bin/hangar`.
- **macOS (aarch64 and x64):** `Hangar.app/Contents/MacOS/hangar-send` with mode `rwxr-xr-x` next to
  `hangar`, in the updater archive and in the disk image, inside the signed app.
- **CI fails without it:** `linux-bundle` checks both Linux bundles; `macos-bundle` checks the
  file, its architecture, its minimum macOS, the app's signature and that the signed client starts.
- **How a session finds it:** at startup Hangar copies the client from its own folder into
  `<app local data>/bin` (`src-tauri/src/send_bin.rs`); an agent session of a project with partners
  or links gets that folder at the end of its `PATH` (`src-tauri/src/manager.rs`, `path_with`).
  Agents call `hangar-send` by name. The install folder itself (`Contents/MacOS`, `/usr/bin`, the
  AppImage's temporary mount) never joins `PATH`.

## Significance for Hangar

- ADR 0012's assumption that Tauri's bundler installs the second binary next to the main one is now
  evidenced for AppImage, .deb, .app and .dmg, not only read from Tauri's source.
- The copy keeps the bundled file's mode (`fs::copy` copies permission bits). Since 2026-11-22 a copy
  whose executable bit is gone, for example after a data folder was restored or synced by a tool
  that drops modes, counts as different and is replaced (`send_bin.rs`, `runnable`).

> [!question] Unverified
> Not run on a real Mac or Linux desktop: that an agent's own command shell keeps the appended
> `PATH` entry (a login shell's profile that sets `PATH` from scratch would drop it), and whether a
> copy of a quarantined app's `hangar-send` outside the bundle is held by Gatekeeper on macOS.
