---
title: "ADR 0013: Linux x64 release"
type: decision
status: accepted
superseded_by:
tags: [linux, release, appimage, deb, updater, environment, platform]
sources: ["[[linux-bundling-reading]]", "[[tauri-updater-and-nsis]]"]
created: 2026-11-18
updated: 2026-11-18
---

# ADR 0013: Linux x64 release

## Context
Hangar ships for Windows only. The code already builds and tests on Linux in CI (the `check`
job on ubuntu-latest), but nothing is bundled, and three things would hurt a first Linux user:

- **No agent found.** A program started from the application menu or as an AppImage gets the
  desktop session's environment, not the shell's. Whatever `~/.profile`, `~/.bashrc` or
  `~/.zshrc` add to `PATH` (`~/.local/bin`, nvm, npm's global folder) is missing, and Claude Code
  or Codex count as not installed ([[linux-bundling-reading]]).
- **The AppImage leaks into child processes.** AppRun and linuxdeploy's GTK hook point GTK and
  GLib search paths into the image's mount point and force `GDK_BACKEND=x11`. A shell, an agent or
  the file manager Hangar opens would inherit them.
- **Features without effect.** GPU readings (`gpu.rs`) and desktop notifications (`notify.rs`)
  exist only on Windows; the launcher badge count needs the Unity launcher API, which most desktops
  do not show.

## Options
1. **Formats:** AppImage only; .deb only; AppImage plus .deb; also .rpm or Flatpak.
2. **Build machine:** ubuntu-latest (24.04) or ubuntu-22.04.
3. **Signing:** updater key as a CI secret, or signing on the owner's machine as for Windows.
4. **`PATH`:** the `fix-path-env-rs` crate; ask the login shell with own code; tell users to
   start Hangar from a terminal.

## Decision
- **AppImage plus .deb, x64.** The AppImage is the only format the updater replaces on Linux
  (platform key `linux-x86_64`), so it is the update format. The .deb is for users who want the
  package manager; it gets no in-app update. No .rpm or Flatpak for now.
- **One target list:** `bundle.targets` is `["nsis", "appimage", "deb"]`. The bundler drops
  targets the platform cannot build, so Windows keeps building NSIS alone and the Windows release
  does not change.
- **Built on ubuntu-22.04** (glibc 2.35): a binary runs only where glibc is at least as new as on
  the build machine. The CI job `linux-bundle` prints the highest glibc symbol version.
- **Signed locally, no key in CI.** CI builds both bundles with updater artifacts off (the same
  `--config` override `tools/release.py` uses) and uploads them as a workflow artifact. Updater
  signatures come from the owner's machine, as for Windows ([[release-rollout]]).
- **Login shell `PATH`, own code** (`src-tauri/src/login_path.rs`). At startup, before any other
  thread, Hangar runs `$SHELL` (fallback `/bin/sh`) once with `-ilc` and a `printf` that prints
  `PATH` between two markers, stdin empty, stderr discarded, in a session of its own, 3 s timeout.
  Text the dotfiles print around the markers is ignored. The result goes in front of Hangar's own
  `PATH` (each folder once, only absolute entries: `.` or `./node_modules/.bin` would resolve in
  a session's foreign working folder) and is set on the process, so every program search and every child
  process sees it without changes at each call site. A crate would add a dependency for about one
  hundred lines; starting from a terminal is no answer for a desktop app. Windows is unchanged.
- **AppImage variables removed for children** (`child_env::appimage_fixes`): `APPIMAGE`,
  `APPDIR`, `ARGV0`, `OWD` go; entries below the mount point are taken out of every variable;
  `GDK_BACKEND=x11` and an empty `GTK_THEME` go. Only when Hangar's own executable lies inside
  `APPDIR`: an `APPDIR` inherited from another program's environment stays untouched. Hangar's
  own process keeps them, its bundled GTK needs them.
- **Only offer what works:** on Linux the GPU wording, desktop notifications (also on macOS until
  implemented) and the launcher badge are hidden, and "Show in Explorer" reads "Show in file
  manager" (`src/platform.ts`).

## Consequences
- Startup on Linux and macOS waits for the login shell, usually well under a second, at most
  3 s; after a timeout Hangar keeps the inherited `PATH` and says so on stderr.
- After the answer or the timeout the shell's process group gets SIGKILL. A program the dotfiles
  start in a session or group of its own (a daemonizing `ssh-agent`, say) survives that, just as
  it survives closing a terminal; a shell that does not go away within 200 ms is reaped by a
  thread of its own, so it cannot hold the window back.
- A login shell that rejects `-ilc` (csh, tcsh, nushell) yields no `PATH`; Hangar then works as
  before.
- The .deb depends on `libasound2 | libasound2t64` (dictation's ALSA, renamed in Ubuntu 24.04);
  Tauri adds WebKitGTK and GTK itself.
- Part 2 (release tool, website downloads, README and changelog) follows once the CI bundles
  have been tried on a real desktop. The release tool is done: `tools/release.py --linux X.Y.Z`
  signs the CI AppImage locally and announces it under `linux-x86_64-appimage`, not the plain
  `linux-x86_64`: tauri-plugin-updater 2.14.1 tries `linux-x86_64-<bundle type>` first and falls
  back to `linux-x86_64`, so a plain key would offer the AppImage to .deb installations, which
  cannot install it ([[release-rollout]], "Adding Linux").

## Evidence
- [[linux-bundling-reading]]: updater format, platform key, glibc advice, target filter, GTK hook.
- [[tauri-updater-and-nsis]]: signing is mandatory; release.py signs after the build.
- `tauri-plugin-updater` 2.14.1 `src/updater.rs` (`get_urls`, `install_appimage`, `install_deb`)
  and `tauri-utils` `platform::bundle_type`: platform key lookup and install per bundle type.
- Code: `src-tauri/src/login_path.rs`, `src-tauri/src/child_env.rs`, `src-tauri/tauri.conf.json`,
  `.github/workflows/ci.yml` (`linux-bundle`), `src/platform.ts`.
