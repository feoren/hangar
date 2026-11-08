---
title: Tauri updater docs and NSIS installer template
type: source
tags: [tauri, updater, nsis, windows]
sources: []
raw:
url: https://raw.githubusercontent.com/tauri-apps/tauri-docs/v2/src/content/docs/plugin/updater.mdx
retrieved: 2026-11-07
created: 2026-11-07
updated: 2026-11-07
---

# Tauri updater docs and NSIS installer template

Sources (retrieved on 2026-11-07):
- Updater docs: `tauri-docs/v2/src/content/docs/plugin/updater.mdx`
- NSIS template: `tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`
- Versions according to crates.io/npm: tauri-plugin-updater 2.14.1, tauri-plugin-process 2.4.0

## Key points: updater
- The updater signature is mandatory and cannot be turned off. Generate keys with
  `tauri signer generate -w <path>`. Sign during the build with `TAURI_SIGNING_PRIVATE_KEY` (path or
  content) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- `bundle.createUpdaterArtifacts: true` produces `…-setup.exe` and `…-setup.exe.sig` on Windows.
- `plugins.updater.pubkey` is the content of the public key, not a path. `endpoints` is a
  list. TLS is mandatory in production.
- Windows `installMode`: `passive` (default, small progress window, no interaction),
  `basicUi`, `quiet`.
- Static `latest.json`: `version`, `platforms.<os-arch>.url` and
  `platforms.<os-arch>.signature` are required (content of the `.sig` file, not a URL). `notes` and
  `pub_date` (RFC 3339) are optional. Platform keys look like `windows-x86_64`.
- JS API: `check()` from `@tauri-apps/plugin-updater`, then `update.downloadAndInstall(cb)` with the
  events `Started` (contentLength), `Progress` (chunkLength) and `Finished`, then `relaunch()`
  from `@tauri-apps/plugin-process`.
- Permissions must be granted in the capabilities (Hangar: `updater:default`,
  `process:allow-restart`, `process:allow-exit`).

## Key points: NSIS template
- The last page uses the "Readme" checkbox as the "create desktop shortcut" option
  (`MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateOrUpdateDesktopShortcut`). There is also "launch app".
- In passive and silent mode, and therefore during updates too, the desktop shortcut is created or
  updated automatically, because the last page is skipped.
- A Start menu shortcut is always created. Uninstalling removes both shortcuts.

> [!question] Unverified
> Whether the "create desktop shortcut" checkbox is ticked by default depends on the NSIS MUI default
> (with `MUI_FINISHPAGE_SHOWREADME_NOTCHECKED` not set). Only a test run of the installer will confirm it.
