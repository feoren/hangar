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
