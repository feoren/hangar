---
title: "ADR 0017: macOS release"
type: decision
status: accepted
superseded_by:
tags: [macos, release, dmg, updater, signing, gatekeeper, environment, platform]
sources: ["[[macos-release-reading]]", "[[linux-bundling-reading]]", "[[tauri-updater-and-nsis]]"]
created: 2026-11-20
updated: 2026-11-20
---

# ADR 0017: macOS release

## Context
Hangar ships for Windows, and Linux is prepared ([[0013-linux-release]]). The `check` job already
builds and tests on `macos-latest`, but nothing is bundled. The owner has no Apple Developer
account yet, so there is neither a Developer ID certificate nor notarization. Things that would
hurt a first macOS user:

- **Not starting at all.** Apple silicon runs no native executable without a signature
  ([[macos-release-reading]]).
- **No agent found.** An app started from the Dock or Finder does not get the shell's `PATH`; the
  login shell answer of [[0013-linux-release]] runs on macOS too, but fell back to `/bin/sh` when
  `$SHELL` was missing, whose `PATH` lacks what `~/.zshrc` adds.
- **Cmd+Q bypasses the quit flow.** Tauri's default menu sends `terminate:`, which ends the
  process without the window's close handler: no workspace save, no question before ending running
  sessions ([[macos-release-reading]]).
- **Features without effect**, listed under "Platform check" below.

## Options
1. **Architectures:** one universal binary (`universal-apple-darwin`), or one build per
   architecture.
2. **Intel build:** natively on an Intel runner, or cross-compiled with `--target
   x86_64-apple-darwin` on Apple silicon.
3. **Formats:** `.app` archive (the update format) plus `.dmg`, or `.dmg` only.
4. **Signing without an account:** unsigned, ad hoc (`-`), or wait for a Developer ID.
5. **Updater key:** a CI secret, or signing on the owner's machine as for Windows and Linux.

## Decision
- **One build per architecture**, arm64 and x64. A universal binary doubles the download for every
  user and needs both whisper.cpp builds in one job; separate builds keep each job like the
  Linux one. Platform keys `darwin-aarch64` and `darwin-x86_64`.
- **Native builds:** arm64 on `macos-latest` (macOS 26, arm64), x64 on `macos-26-intel`, a standard
  runner for public repositories ([[macos-release-reading]]). `macos-13` no longer exists. When
  GitHub drops the Intel images, the x64 entry moves to `macos-latest` with the target it already
  names (`--target x86_64-apple-darwin`); whisper.cpp's cross build is then untested.
- **`.app.tar.gz` plus `.dmg`.** The updater replaces the `.app` from a gzip tar with `Hangar.app`
  as its only top folder; the `.dmg` is for the first download. Both hold the same app.
- **One target list** (`bundle.targets` `["nsis", "appimage", "deb", "app", "dmg"]`): the bundler
  keeps only the targets of the platform it runs on (`Settings::package_types`,
  [[linux-bundling-reading]]), so the Windows release (`tauri build` without `--bundles`) still
  builds NSIS alone, and the Linux job names `--bundles appimage,deb` anyway.
- **macOS 13 minimum** (`minimumSystemVersion: "13.0"`, default 10.13); CI sets the same
  `MACOSX_DEPLOYMENT_TARGET` for rustc and whisper.cpp's CMake and checks `minos 13.0` in both
  binaries and `LSMinimumSystemVersion` in `Info.plist`.
- **Ad hoc signature until there is an account** (`signingIdentity: "-"`), the hardened runtime at
  its default (on) and the existing `Entitlements.plist` (microphone). With an account only CI
  secrets change, nothing in the repository: `APPLE_SIGNING_IDENTITY` wins over the `-` in the
  config (Tauri CLI 2.12.4), `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD` import the
  certificate, and `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` (or `APPLE_API_ISSUER`,
  `APPLE_API_KEY` and the key's content as `APPLE_API_PRIVATE_KEY`, written to a file for
  `APPLE_API_KEY_PATH`) notarize. The CI step passes only variables that have a value, and only
  when `APPLE_SIGNING_IDENTITY` is set: an empty one would replace `-`.
- **Signed locally, no key in CI**, as for Linux: updater artifacts off in CI, the job packs the
  archive itself (`COPYFILE_DISABLE=1 tar --no-mac-metadata`, no AppleDouble entries), the job
  `macos-bundles` merges both architectures into the artifact `macos-bundles`, and
  `tools/release.py --macos X.Y.Z` signs both archives with one password prompt and adds them to
  `latest.json` ([[release-rollout]], "Adding macOS"). The plain `darwin-<arch>` key is enough:
  the bundle type on macOS is always `app`, so no other installation type can pick it up.
- **Login shell on macOS** (`src-tauri/src/login_path.rs`): `$SHELL` when absolute, otherwise the
  account's shell from the user database (`getpwuid_r`), otherwise `/bin/zsh` (the default since
  macOS 10.15; `/bin/sh` stays the Linux fallback).
- **Own Quit item** (`src-tauri/src/app_menu.rs`): Tauri's default menu with only Quit replaced by
  an item with the same shortcut (Cmd+Q) that shows the main window and sends it `tray-quit`, so
  it quits the usual way. A click on the Dock icon while the window is hidden shows it
  (`RunEvent::Reopen`).

## Platform check (macOS)
| Area | State on macOS | What Hangar does |
| --- | --- | --- |
| Desktop notifications | no backend (`notify.rs` returns "not shown") | hidden (`hasDesktopNotifications`), unchanged |
| GPU readings | Windows performance counters only (`gpu.rs`) | hidden (`hasGpuReading`), unchanged |
| Battery or AC | unknown ([[power-detection]]) | hidden, unchanged |
| Notification area icon | Tauri `tray-icon` draws a menu bar icon | offered; closing hides the window, the Dock icon or the menu bar icon brings it back (new: `Reopen`) |
| Dictation audio | CoreAudio through cpal; `NSMicrophoneUsageDescription` in `Info.plist`, audio-input entitlement | offered; CI checks both |
| Shortcuts | every Hangar shortcut tests `ctrlKey`, not `metaKey` (`src/shortcuts.ts`) | the text "Ctrl" is right on macOS; the link dialog says ⌘ for multi-select because Ctrl+click is a right click there; Cmd+C, Cmd+V and Cmd+A come from the default Edit menu |
| Cmd+Q | ended the process without the quit flow | own Quit item (new) |
| Keychain | `keyring` 4.2 with `apple-native-keyring-store` | offered |
| Dock badge | `setBadgeCount` (`src/taskbarBadge.ts`, kind `count`) | offered, unchanged |
| "Show in Finder" | `notes.revealFinder` (`src/platform.ts`) | unchanged |

> [!question] Unverified
> - Every row above on a real Mac: nothing has run outside CI. CI checks the bundle (signature,
>   architecture, minimum version, `hangar-send` starts, archive unpacks to a valid app), not the
>   window.
> - The ad hoc signature changes with every build. macOS ties privacy grants (microphone) and
>   keychain item access to the code's designated requirement, so after each update macOS may ask
>   again for the microphone and for the keychain items.
> - Whether macOS takes Ctrl+Shift+Space (dictation) for itself; Ctrl+Space switches input sources
>   by default.
> - Whether the menu bar icon should be a template image (it uses the colored app icon now).

## What users get without an Apple Developer account
- **Apple silicon:** the ad hoc signature lets the app run at all ([[macos-release-reading]]).
- **Gatekeeper:** a downloaded app that is not Developer ID signed and notarized is blocked on its
  first start. Since macOS 15 Sequoia, Control-click > Open no longer overrides that; users open
  System Settings > Privacy & Security and click "Open Anyway", then "Open" in the dialog that
  follows (Apple developer news of 2024-08-06, Apple support 102445). On macOS 13 and 14 the
  Control-click > Open path still exists.
- **Terminal alternative:** `xattr -dr com.apple.quarantine /Applications/Hangar.app` removes the
  quarantine attribute the browser set (`-d` deletes, `-r` recursive; `man xattr`), so Gatekeeper
  does not check the app on its first start.
- **Updates:** the in-app update replaces the bundle with an archive Hangar downloaded itself.

> [!question] Unverified
> - The exact first-start dialog for an ad hoc signed arm64 app on macOS 15 and 26, and that the
>   `xattr` line skips it.
> - Whether an update that Hangar downloads gets a quarantine attribute (and so a Gatekeeper check
>   again); the updater writes the bytes with its own HTTP client, not through the browser.

## Consequences
- README "Requirements" and the website's download list stay as they are until macOS ships; the
  first release with macOS adds them, plus a short Gatekeeper note on the download page.
- Each release has two more CI jobs on macOS runners (and one merge job); the Intel runner can be
  slow to start under load.
- With an Apple Developer ID the bundles are signed and notarized in CI; the Gatekeeper section
  above then no longer applies. Nothing in the repository changes for that.

## Evidence
- [[macos-release-reading]]: runner labels, ad hoc signing, Gatekeeper, identity precedence,
  updater archive layout, Cmd+Q path, zsh default.
- [[linux-bundling-reading]]: the bundler's target filter per platform.
- Code: `src-tauri/tauri.conf.json`, `src-tauri/src/login_path.rs`, `src-tauri/src/app_menu.rs`,
  `src-tauri/src/lib.rs`, `src-tauri/tests/contract.rs`, `.github/workflows/ci.yml`
  (`macos-bundle`, `macos-bundles`), `tools/release.py` (`--macos`).
