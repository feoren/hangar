---
title: macOS release (runners, signing, Gatekeeper, updater)
type: source
tags: [macos, tauri, updater, signing, gatekeeper, ci]
sources: []
raw: raw/2026-11-20-macos-release-reading.md
url: https://v2.tauri.app/distribute/sign/macos/
retrieved: 2026-11-20
created: 2026-11-20
updated: 2026-11-20
---

# macOS release (runners, signing, Gatekeeper, updater)

Reading of GitHub's runner list, Tauri's macOS signing guide, the Tauri CLI 2.12.4 source, the
local sources of tauri-utils 2.10.2, tauri-plugin-updater 2.14.1, tauri 2.12.5, tao 0.37.2 and
four Apple pages. Nothing was built or run on a Mac.

## Key points
- **Runners (2026-11-20):** `macos-latest` is macOS 26 on arm64; standard Intel runners for
  public repositories are `macos-15-intel` and `macos-26-intel`. `macos-13` is no longer listed,
  macOS 14 images retired on 2026-11-02. Standard runners are free for public repositories.
- **Ad hoc signing:** Apple silicon runs no native executable without a signature, and an ad hoc
  one is enough; it "cannot pass through Gatekeeper". Tauri takes `"signingIdentity": "-"` for it.
  Intel Macs and Rosetta do not need a signature to run.
- **Gatekeeper:** since macOS 15 Sequoia, Control-click no longer overrides Gatekeeper for software
  that is not signed with a Developer ID and notarized; users go to System Settings > Privacy &
  Security > "Open Anyway" ([Apple support 102445](https://support.apple.com/en-us/102445)).
- **Identity precedence:** the Tauri CLI 2.12.4 takes `APPLE_SIGNING_IDENTITY` over
  `bundle.macOS.signingIdentity`; no variable for the hardened runtime or the minimum version.
- **Defaults:** `minimumSystemVersion` defaults to 10.13 and drives `LSMinimumSystemVersion` and
  `MACOSX_DEPLOYMENT_TARGET`; the hardened runtime is on by default.
- **Updater on macOS:** key `darwin-<arch>` (or `darwin-<arch>-app` first, the bundle type is
  always `app`); the update is a gzip tar whose first path part (the `.app`) is dropped while
  extracting, then the bundle is swapped in place.
- **Cmd+Q:** the default menu's Quit sends `terminate:`, which ends the event loop
  (`LoopDestroyed`) without a window close request or `ExitRequested`.
- zsh is the default login shell of every account created since macOS 10.15.

> [!question] Unverified
> - Whether the Tauri bundler applies the hardened runtime and the entitlements to an ad hoc
>   signature; the CI job prints the signature flags and checks the entitlement when the runtime
>   flag is set.
> - What a quarantined, ad hoc signed arm64 app shows on macOS 15 and 26 the first time (the
>   "cannot be verified" dialog, or "is damaged"), and whether `xattr -dr com.apple.quarantine`
>   skips it. Needs a download on a real Mac.

## Significance for Hangar
- [[0017-macos-release]]: arm64 on `macos-latest`, x64 on `macos-26-intel`, ad hoc signature
  until an Apple Developer ID exists, `.app.tar.gz` plus `.dmg`, macOS 13 minimum, own Quit item.
- [[release-rollout]]: "Adding macOS".
