---
title: Rolling out a release (installer and auto-update)
type: synthesis
tags: [release, updater, nsis, windows]
sources: ["[[tauri-updater-and-nsis]]", "[[code-signing-research]]"]
created: 2026-11-07
updated: 2026-11-11
---

# Rolling out a release

## What users get
- **`Hangar-Setup.exe`**, run by double-click: an NSIS installer (German and English). It installs
  for the current user without admin rights (`installMode: currentUser`, into `%LOCALAPPDATA%\Hangar`)
  and creates a Start menu shortcut. Its last page offers to create a desktop shortcut
  ([[tauri-updater-and-nsis]]).
- **Auto-update:** 5 s after startup and every 6 h after that, the app fetches
  `https://github.com/feoren/hangar/releases/latest/download/latest.json`, and only if that fails
  (no successful response or no valid manifest) `https://hangar.feoren.com/download/latest.json`.
  Verified in `tauri-plugin-updater` 2.14.1 (`src/updater.rs`): the endpoints are tried in order
  and the first valid manifest wins. Versions up to 0.1.x only know the second URL. If a newer version exists, the sidebar shows
  "Update x.y.z available · Install". A click downloads the update, verifies the updater signature and
  installs it in passive mode with a progress window. Hangar then restarts.
  Running sessions end in the process; the app asks first. Manually: menu → Help →
  "Check for updates …".

## Steps per release
Rule ([[0002-version-scheme]]): **The code always carries the upcoming version.** Between two releases,
`tauri.conf.json`, `package.json` and `Cargo.toml` already hold the next version, and `src/changelog.json`
collects its items under that same version without a date. `src/versionSync.test.ts` checks this in every check suite run.

1. Check suite green, everything committed. The version's changelog entry has German items.
2. `py -3 tools/release.py --version 0.2.0` (notes come from the changelog; `--notes "..."` only replaces the text;
   `--base-url https://...` builds for a self-hosted folder instead of a GitHub release)
   - give the version as X.Y.Z only; the build number comes from Git ([[0002-version-scheme]]), e.g. `0.2.0+31`, shown as `0.2.0-31`
   - must equal the highest version in `src/changelog.json` and must not be lower than the version in the files
   - must be higher than the last published one (highest dated changelog entry or folder under
     `release/`), otherwise the updater does not detect the update
   - dates the changelog entry to today
   - builds only from a clean working tree
   - sets the version in `tauri.conf.json`, `Cargo.toml` and `package.json`
   - builds with `pnpm tauri build` and signs with `%USERPROFILE%\.tauri\hangar.key`
   - creates `release/0.2.0-31/` containing `Hangar_0.2.0-31_x64-setup.exe`, `Hangar-Setup.exe`,
     `SHA256SUMS.txt`, `latest.json` (its download URL points to the GitHub release `v0.2.0`) and
     `release-notes.md`
   - `SHA256SUMS.txt` lists the SHA-256 of both installers in `sha256sum` format
     (`<hex>  <file name>`, LF line endings), so `sha256sum -c SHA256SUMS.txt --ignore-missing`
     verifies a download; on Windows, compare `Get-FileHash Hangar-Setup.exe` with its line
3. Refresh the README screenshots: `pnpm readme:shots`. It renders the real frontend with a mocked
   backend and a fictional scene (`tools/readme-shots/`) in headless Edge, Chrome or Chromium
   (`README_SHOTS_BROWSER` picks a specific executable) and writes one image per scene and color
   scheme into `docs/images/` (1296 x 839): the overview `hangar-dark.png` and `hangar-light.png`
   (at most 400 KB each) and `hangar-<scene>-dark.png` / `-light.png` for the scenes `subagent`,
   `team`, `notes` and `settings` (at most 300 KB each). The README and the website use these
   stable names. `--scene <name>` renders only some scenes. Times are deterministic: the mock runs
   a fixed clock and the browser renders in UTC; animations stop on a fixed frame. It cleans up
   after itself (temporary files only under `tests/.tmp/readme-shots`), runs its own Vite server
   on a free port and never touches a running Hangar. Run it after `release.py` and not before:
   the script needs a clean working tree and only then do the files carry `X.Y.Z+Build`, so the
   header in the screenshots shows exactly the released version. Look at every image before
   committing.
4. Commit the version change, the dated changelog and the screenshots together:
   `chore: Version 0.2.0-31`.
5. Publish; the script prints these commands, it never publishes by itself:
   1. `git push origin main`
   2. `gh release create v0.2.0 <the four files> --repo feoren/hangar --target main --title "Hangar 0.2.0-31" --notes-file release/0.2.0-31/release-notes.md --draft`
   3. `gh release edit v0.2.0 --repo feoren/hangar --draft=false --latest`

   Draft first, publish last: `/releases/latest` skips drafts, so installed apps see the update only
   once every file is attached. The files go in the order setup, `Hangar-Setup.exe`,
   `SHA256SUMS.txt`, `latest.json`; with `--base-url` the script prints the same order for the
   upload, `latest.json` last because it announces the update.
6. Transition for old installations (0.1.x only poll the own server): also upload `latest.json` to
   `https://hangar.feoren.com/download/`. Its URL points to GitHub, so one build serves both. Once
   no 0.1.x installation is left, this step can go.
7. Create the upcoming version (the script prints the command at the end): `py -3 tools/release.py --next 0.3.0`
   - sets the three files and `Cargo.lock` to `0.3.0` (without build number)
   - adds `{"version": "0.3.0", "date": null, "items": []}` at the top of `src/changelog.json`
   - aborts if the highest changelog version has no date yet or `0.3.0` is not higher than everything before
   - then commit: `chore: set upcoming version 0.3.0`

Website, download button (always the newest release):

```html
<a href="https://github.com/feoren/hangar/releases/latest/download/Hangar-Setup.exe" download>Download Hangar for Windows</a>
```

## Keys
- Private key: `%USERPROFILE%\.tauri\hangar.key`, without a password, deliberately outside the repo
  and any cloud sync folder. Public key: in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
- > [!warning] Keep it safe
  > If the private key is lost, installed apps can **no longer accept updates**;
  > users would have to reinstall. If it falls into the wrong hands, someone can sign updates
  > that every installation trusts. So keep an offline copy (e.g. a
  > password manager or USB stick), never in a cloud sync folder or Git.
- Different key file: set the environment variable `HANGAR_SIGNING_KEY`.

## Code signing (Authenticode)
Without a code signing certificate, Windows shows "Windows protected your PC" (SmartScreen) the first
time the installer runs. Users get past it with "More info → Run anyway".
The Authenticode signature is independent of the updater signature (minisign, `hangar.key`), which is
already active.

### Providers and costs
As of 2026-11-08 according to [[code-signing-research]]:
- **Recommendation:** Certum Standard OV with the SimplySign cloud key, about 209 EUR (169 EUR with a card).
- If Hangar becomes public open source: Certum Open Source (from 49 EUR) or SignPath Foundation
  (free, signing from CI only).
- Azure Artifact Signing (9.99 USD/month) is available to individuals only in the US and Canada, and
  onboarding is paused according to a Q&A from 2026-03.
- EV is no longer worth it: since 2024 it brings no instant reputation. Even with OV, SmartScreen builds
  reputation only through downloads; the warning may persist at first (conclusion).
- Since June 2023, keys live on an HSM or token, and certificates last at most about 459 days. Renew
  before they expire; thanks to the timestamp, installers already signed stay valid.

### Setup
`tools/release.py` reads the code signing settings from the environment. `tauri.conf.json` stays
unchanged; the script passes the values to `pnpm tauri build` as a temporary `--config` file and
deletes it afterwards ([[code-signing-research]]: `--config` accepts file paths and merges them).

| Variable | Meaning |
|---|---|
| `HANGAR_CERT_THUMBPRINT` | SHA-1 thumbprint (40 hex characters; spaces, colons and the invisible U+200E from the certificate dialog are stripped). Tauri then signs with signtool, `digestAlgorithm: sha256`. |
| `HANGAR_TIMESTAMP_URL` | Timestamp server, required for signing, default `http://time.certum.pl`. Only `http://` or `https://`. |
| `HANGAR_TIMESTAMP_TSP` | `1` (default) = RFC 3161 (`tsp: true`, signtool `/tr`), `0` = legacy Authenticode (`/t`). |
| `HANGAR_SIGN_COMMAND` | Instead of a thumbprint, a custom signing command as a JSON array, with `"%1"` for the file, e.g. `["signtool.exe","sign","/sha1","<thumbprint>","/fd","sha256","/tr","http://time.certum.pl","/td","sha256","%1"]`. It goes to Tauri as `{cmd, args}` so that paths with spaces survive. A shell string is rejected. The command must set a timestamp itself. |

- If neither variant is set, the script builds as before and reports "unsigned".
- Both set at once, an invalid thumbprint or a bad URL: abort before any change.
- Values never appear in output, error messages or on the command line.
- With a thumbprint, you need `signtool.exe` from the Windows SDK and the certificate in the
  user's certificate store. With SimplySign, SimplySign Desktop must be logged in.

Example (PowerShell, for this session only):

```powershell
$env:HANGAR_CERT_THUMBPRINT = "<thumbprint from certmgr.msc>"
py -3 tools/release.py --version 0.2.0
```

### Verification
After a signed build, the script checks the installer and `src-tauri/target/release/hangar.exe`
(if built in this run) with `Get-AuthenticodeSignature`. It aborts if the status is not
`Valid` or the timestamp is missing; nothing lands under `release/` in that case. By hand:

```powershell
Get-AuthenticodeSignature .\release\0.2.0-31\Hangar-Setup.exe | Format-List Status, SignerCertificate, TimeStamperCertificate
```

> [!question] Unverified
> - Never run with a real certificate: whether Tauri 2.12 actually signs the NSIS `setup.exe` and the
>   uninstaller will only be proven by the first signed build (the script checks the `setup.exe`).
> - Whether SimplySign signs without manual input; otherwise a login prompt appears during the build.
> - Whether `time.certum.pl` speaks RFC 3161 (otherwise `HANGAR_TIMESTAMP_TSP=0`).

## Limits and open points
- The update URLs are hard-coded in `tauri.conf.json`. Moving to a different host requires a
  release that is still served from an old URL.
- Windows x64 only for now. For macOS and Linux, `latest.json` needs `darwin-*` and `linux-*`
  entries.
