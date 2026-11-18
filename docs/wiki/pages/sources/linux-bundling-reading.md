---
title: Linux bundling (AppImage, .deb, updater)
type: source
tags: [linux, tauri, appimage, deb, updater, environment]
sources: []
raw: raw/2026-11-18-linux-bundling-reading.md
url: https://v2.tauri.app/distribute/appimage/
retrieved: 2026-11-18
created: 2026-11-18
updated: 2026-11-18
---

# Linux bundling (AppImage, .deb, updater)

Reading of the Tauri updater and AppImage docs, the bundler's target filter, the `DebConfig` of
tauri-utils 2.10.2 and the AppRun hook of linuxdeploy-plugin-gtk. Nothing was built or run; the
CI job `linux-bundle` is the first real build.

## Key points
- The updater replaces the AppImage on Linux; the docs name no other Linux format. The platform
  key in `latest.json` is `linux-x86_64`.
- A binary only runs where glibc is at least as new as on the build machine, so the docs say to
  build on the oldest base system to support.
- GUI apps on macOS and Linux do not inherit the `PATH` of the shell dotfiles (the AppImage page
  says so itself).
- `bundle.targets` is filtered per platform: a target the platform cannot build is dropped
  without an error. One list `["nsis", "appimage", "deb"]` builds NSIS on Windows and AppImage
  plus .deb on Linux.
- linuxdeploy's GTK hook exports `GDK_BACKEND=x11`, `GTK_THEME` (usually empty) and several GTK
  and GLib search paths below the mount point, unconditionally. No `LD_LIBRARY_PATH`.

> [!question] Unverified
> That Tauri's AppImage carries exactly this hook. The CI job prints the image's `apprun-hooks`
> so the first build shows it.

> [!question] Unverified
> What `tauri build` does with `createUpdaterArtifacts: true` and no private key. Hangar never
> relies on it: release.py and CI both turn updater artifacts off for the build.

## Significance for Hangar
- [[0013-linux-release]]: AppImage as the update format plus a .deb, build on ubuntu-22.04,
  login shell `PATH`, AppImage variables removed for child processes.
