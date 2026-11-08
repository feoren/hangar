---
title: Code signing for the Windows installer (research and verification)
type: source
tags: [release, windows, authenticode, smartscreen, tauri]
sources: []
raw: raw/2026-11-08-code-signing-research.md
url:
retrieved: 2026-11-08
created: 2026-11-08
updated: 2026-11-08
---

# Code signing for the Windows installer

Research result from the main agent dated 2026-11-08, plus our own verification in the package
`@tauri-apps/cli` 2.12.4 and with `Get-AuthenticodeSignature` (`raw/2026-11-08-code-signing-research.md`).

## Key points: providers and costs
Taken from the research, not read up ourselves:

| Route | Cost | Requirement |
|---|---|---|
| Certum Standard OV, SimplySign cloud key | about 209 EUR (169 EUR with a card) | identity verification |
| Certum Open Source | from 49 EUR | public OSS project |
| SignPath Foundation | free | OSS project, signing from CI only |
| Azure Artifact Signing | 9.99 USD/month | individuals in the US/Canada only, onboarding paused according to a Q&A from 2026-03 |

- Since 2024, EV no longer brings instant reputation with SmartScreen (Microsoft Learn, as of 2026-08-29).
- Since June 2023, OV keys also live on an HSM or token; there is no `.pfx` file anymore.
- Certificates last at most about 459 days.

> [!question] Unverified
> - Prices from Sectigo and SSL.com.
> - Whether SimplySign can be automated without manual input (TOTP from the phone app). Assumption:
>   SimplySign Desktop exposes the certificate in the Windows certificate store while the
>   session is logged in, in which case the thumbprint is enough for signtool.
> - Whether `time.certum.pl` speaks RFC 3161 (signtool `/tr`). Hangar therefore defaults to `tsp: true`,
>   which `HANGAR_TIMESTAMP_TSP=0` turns off.

## Key points: Tauri 2 (verified in the schema of @tauri-apps/cli 2.12.4)
- `bundle.windows.certificateThumbprint` (SHA-1 of the certificate), `digestAlgorithm` ("Required for
  code signing", SHA-256 recommended), `timestampUrl`, `tsp` (RFC 3161, default `false`).
- `bundle.windows.signCommand` replaces signtool. As a string, Tauri splits it on spaces; the
  object form `{ cmd, args }` allows spaces (CHANGELOG, PR 10634). `%1` stands for the file.
- `tauri build --config` accepts JSON strings or file paths and merges them with `tauri.conf.json`;
  later values override earlier ones (`--help`, CHANGELOG PR 8783 and 13117).
- After patching the binary with the bundle type, the bundler signs again (CHANGELOG PR 15619).
- According to the research (Tauri PR 7398, issue 9767), Tauri signs the main EXE, the uninstaller and setup.exe.

> [!question] Unverified
> - That the NSIS `setup.exe` itself gets signed: the docs do not say so explicitly.
>   `tools/release.py` therefore checks it after every signed build.
> - Whether `timestampUrl` is honored when `signCommand` is set. Hangar does not pass it in that case;
>   the custom command must apply the timestamp itself.

## Key points: verification (measured)
- `Get-AuthenticodeSignature` reports `NotSigned` for an unsigned script, `UnknownError` for a file that
  is not a valid PE, and `Valid` with `TimeStamperCertificate` set for `pwsh.exe`.
  It follows that only `Valid` counts as success; everything else is a failure.

## Significance for Hangar
Implementation and steps: [[release-rollout]] (section on code signing).
