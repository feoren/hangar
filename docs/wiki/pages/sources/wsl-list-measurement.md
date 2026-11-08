---
title: "Measurement: WSL distribution list and keychain round trip on Windows 11"
type: source
tags: [windows, wsl, keychain, keyring, measurement]
sources: []
raw: raw/2026-11-08-wsl-list-measurement.md
url:
retrieved: 2026-11-08
created: 2026-11-08
updated: 2026-11-08
---

# Measurement: WSL distribution list and keychain round trip

Own measurement on Windows 11 Pro 10.0.26300 with WSL 2.7.14.0.

## Key points
- `wsl.exe --list --quiet` writes into a pipe as UTF-16LE without byte order mark, one
  distribution per line, CRLF line ends, exit code 0.
- With `WSL_UTF8=1` in its environment the same output comes as UTF-8. Hangar removes the variable
  for the child and its parser accepts both encodings.
- Docker Desktop shows up as the distribution `docker-desktop`; it is no terminal.
- Detection through Hangar (system `wsl.exe`, `CREATE_NO_WINDOW`, Job Object) took about 45 ms
  with the WSL service running. A cold start of the service was not measured.
- keyring 4.2.0 stores, overwrites, reads and deletes entries in the Windows Credential Manager;
  deleting a missing entry reports `NoEntry`; 1280 characters fit.

## Significance for Hangar
WSL entries and secrets: [[agent-workflow-features]], [[keyring]]. Code:
`src-tauri/src/wsl.rs`, `src-tauri/src/secrets.rs`.

> [!question] Unverified
> Output on Windows 10 1809 to 21H2 (inbox WSL, `wsl.exe` before the Store version) and the time
> of a cold service start.
