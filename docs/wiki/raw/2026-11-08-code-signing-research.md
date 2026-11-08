# Research: code signing for the Windows installer (as of 2026-11-08)

Handed over by the main agent (research assignment "code signing certificate for the
installer"), plus own local checks on the same day. Contains no secrets.

## A. Research result (handed over by the main agent, sources researched there)

- Recommendation: Certum Standard OV Code Signing with the SimplySign cloud key, approx. 209 EUR; with
  card (token) 169 EUR.
- For a public open source project: Certum Open Source Code Signing (from 49 EUR) or
  SignPath Foundation (free, signing only from CI).
- Azure Artifact Signing (formerly Trusted Signing), 9.99 USD/month: for individuals only in the
  USA and Canada; onboarding paused according to Microsoft Q&A from 2026-03.
- Since 2024, EV certificates no longer grant instant SmartScreen reputation
  (Microsoft Learn, as of 2026-08-29).
- Since June 2023, keys must reside on an HSM or hardware token for OV certificates as well
  (CA/Browser Forum Baseline Requirements Code Signing).
- Validity of code signing certificates at most approx. 459 days.
- Tauri 2: `bundle.windows.certificateThumbprint`, `digestAlgorithm`, `timestampUrl` or
  `bundle.windows.signCommand` with `%1`. According to the Tauri repo (PR 7398, issue 9767), the main EXE,
  uninstaller and setup.exe are signed. The documentation does not mention NSIS explicitly.
- Unverified according to the research: automating SimplySign (sign-in via phone app/TOTP),
  prices of Sectigo and SSL.com.

## B. Own check in the repo (2026-11-08, Windows 11, @tauri-apps/cli 2.12.4)

1. `pnpm tauri build --help`:
   `-c, --config <CONFIG>  JSON strings or paths to JSON, JSON5 or TOML files to merge with the
   default configuration file. Configurations are merged in the order they are provided, which
   means a particular value overwrites previous values when a config key-value pair conflicts.`
2. `node_modules/@tauri-apps/cli/config.schema.json`, definition `WindowsConfig`:
   - `certificateThumbprint`: "Specifies the SHA1 hash of the signing certificate." (string|null)
   - `digestAlgorithm`: "Required for code signing. SHA-256 is recommended." (string|null)
   - `timestampUrl`: "Server to use during timestamping." (string|null)
   - `tsp`: "Whether to use Time-Stamp Protocol (TSP, a.k.a. RFC 3161) for the timestamp server.
     ... like e.g. SSL.com does. If so, enable TSP by setting to true." (bool, default false)
   - `signCommand`: "Specify a custom command to sign the binaries. This command needs to have a
     `%1` in args which is just a placeholder for the binary path ... By Default we use
     `signtool.exe`". `CustomSignCommandConfig` = String ("Tauri will split the string with ' '")
     or object `{ cmd: string, args: string[] }` ("allows you to use whitespace").
3. `node_modules/@tauri-apps/cli/CHANGELOG.md`:
   - PR 10634: "Custom sign command with object notation for whitespaces in the command path and
     arguments."
   - PR 13117: "Fix setting merge config value to null with `--config` arg" (merge semantics).
   - PR 8783: "Fixes a regression on the `--config` argument not accepting file paths."
   - PR 15619: "the bundler skips patching the main executable with bundle type information (and
     the subsequent re-signing)" (without the flag, re-signing happens after patching).
4. `Get-AuthenticodeSignature` (PowerShell 7, own test in `tools/test_release.py`):
   - unsigned `.ps1` with a space in the path: status `NotSigned`.
   - made-up file `MZ unsigniert` with extension `.exe`: status `UnknownError` (not a valid PE).
   - `pwsh.exe`: status `Valid`, `TimeStamperCertificate` set.
