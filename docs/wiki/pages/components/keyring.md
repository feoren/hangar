---
title: "keyring (OS keychain access)"
type: component
tags: [secrets, keychain, windows, macos, linux, dependency]
sources: ["[[wsl-list-measurement]]"]
version: 4.2.0
license: MIT OR Apache-2.0
repo: https://github.com/open-source-cooperative/keyring-rs
created: 2026-11-08
updated: 2026-11-08
---

# keyring

Hangar keeps the values of secret profile variables (`secret_env`) in the keychain of the
operating system through `keyring` 4.2.0 (`src-tauri/src/secrets.rs`).

## Structure (read from the 4.2.0 sources in the Cargo registry)
- Since version 4 the crate is a thin wrapper: the API lives in `keyring-core` 1.0, each platform
  store is a crate of its own. The default feature `v1` keeps the old `Entry` API and picks the
  store per platform: `windows-native-keyring-store` (Credential Manager),
  `apple-native-keyring-store` with `keychain` (macOS Keychain),
  `zbus-secret-service-keyring-store` with `crypto-rust` (Secret Service over D-Bus, pure Rust).
- The store is set once, on the first `Entry::new`; when that fails every entry reports
  `NoDefaultStore`.
- `rust-version` 1.88, edition 2024.
- `keyring_core::Error` is `#[non_exhaustive]`. `BadEncoding` and `BadDataFormat` carry the raw
  stored bytes and `Ambiguous` debug-prints entries, so Hangar maps every variant to its own text
  and never shows or logs the library's `Display` output for those.

## Windows store details (read from `windows-native-keyring-store` 1.1.0 sources)
- Target name `<account>.<service>`, so Hangar's entries are `<profile id>/<VARIABLE>.hangar`.
  Credential Manager target names ignore case, hence Hangar refuses profile ids that differ
  only in case.
- Default persistence is Enterprise (roams with the user profile in a domain). The entry
  modifier `persistence` (`Session`, `Local`, `Enterprise`) applies when a value is written;
  `keyring`'s v1 `Entry` cannot pass modifiers, so Hangar calls
  `keyring_core::Entry::new_with_modifiers` after `keyring::Entry::store_status()` has set the
  default store. Measured: the ignored test `secrets::tests::keychain_round_trip` reads back
  `persistence = Local` (Windows 11 Pro 10.0.26300, 2026-11-08).
- `get_attributes` reads the credential with `CredReadW`, copies only target, user name, alias,
  comment and persistence, wipes the blob and frees it; a missing credential gives `NoEntry`.
  Hangar uses it to tell "stored" without reading the value. The Secret Service store does the
  same with item attributes. The macOS store has no attributes: `keyring-core`'s default
  `get_attributes` calls `get_secret` (so a missing entry gives `NoEntry`) and drops the value
  without wiping it, so on macOS Hangar calls `get_secret` itself and zeroizes the bytes.

## No per-app access control on Windows
The Credential Manager has no access list per application: every process running as the same
user can enumerate and read generic credentials, including `*.hangar`. The keychain protects the
values at rest and from other users, not from other programs of the same user. (General Windows
behavior of `CredReadW` for the caller's own logon session.)

> [!question] Unverified
> No measurement with a second program reading a `*.hangar` credential; the statement follows
> from the Credential Manager API, which checks only the user, not the calling program.

## Build cost
- Windows: `keyring-core`, `windows-native-keyring-store`, `byteorder`, `zeroize`; `regex` and
  `windows-sys` 0.61 were in the build already.
- Linux: `zbus` 5 with the async-io stack (`async-*`, `polling`, `blocking`, ...) and RustCrypto
  (`aes`, `cbc`, `hkdf`, `num-bigint`) for the Secret Service session encryption. The largest
  addition of the three platforms.
- macOS: `apple-native-keyring-store` on top of the Security framework.

## Measured
Round trip against the Windows Credential Manager on Windows 11: [[wsl-list-measurement]].

> [!question] Unverified
> Behaviour on macOS (prompt when an unsigned build reads an entry another build wrote) and on
> Linux without a running Secret Service (expected: `NoDefaultStore` or a platform failure, which
> Hangar reports as "keychain not available" or "keychain failed").
