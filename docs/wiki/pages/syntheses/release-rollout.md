---
title: Rolling out a release (installer and auto-update)
type: synthesis
tags: [release, updater, nsis, windows, linux, appimage, deb]
sources: ["[[tauri-updater-and-nsis]]", "[[code-signing-research]]", "[[linux-bundling-reading]]"]
created: 2026-11-07
updated: 2026-11-18
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
   - builds only from a clean working tree. git runs with `-c core.fsmonitor=false`, a
     `core.hooksPath` below the git program itself (a folder that can never exist), no `GIT_*` or
     `TAURI_SIGNING_PRIVATE_KEY*` variable, and the status check additionally switches off every
     configured filter driver (`filter.<name>.clean=` etc.) and runs
     `status --porcelain --ignore-submodules=all --untracked-files=all`, so no program the
     repository's config names runs and `status.showUntrackedFiles` cannot hide files
   - refuses to build (also with `--allow-dirty`) when a file exists that changes the build
     although `git status` does not show it when it is ignored or excluded:
     `src-tauri/tauri.<anything>.conf.json`, `Tauri.toml`, `tauri.conf.json5` and their platform
     forms next to `tauri.conf.json`; `.cargo/config`, `.cargo/config.toml`, `rust-toolchain` and
     `rust-toolchain.toml` in `src-tauri`, the repository and every folder above it up to the drive
     root; `.npmrc` in the repository. A file Git tracks counts as reviewed and passes
     (`tracked_build_inputs`, `git ls-files`): the repository's own `.cargo/config.toml` sets the
     whisper.cpp build flags of the local dictation ([[local-dictation]]). Cargo's own `%USERPROFILE%\.cargo\config.toml` is not
     checked (see "Release from a fresh clone")
   - refuses to run while `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is set in the environment (every
     program started from that shell could read it); the password is asked for at the console
     when the installer is signed
   - sets the version in `tauri.conf.json`, `Cargo.toml` and `package.json`
   - builds the signature verifier first (`cargo build --locked --offline --example
     verify_update_sig`, its path taken from Cargo's JSON report and its SHA-256 noted), so nothing
     compiles between signing and collecting the installer
   - builds with `node.exe node_modules/@tauri-apps/cli/tauri.js build` (never through
     `pnpm.cmd`) with `bundle.createUpdaterArtifacts: false` (a `--config` override) and without
     any `TAURI_SIGNING_PRIVATE_KEY*` variable in its environment, so no build script or
     dependency sees the key
   - then signs the finished installer in a separate
     `node.exe node_modules/@tauri-apps/cli/tauri.js signer sign --app-version <X.Y.Z+Build>` with
     `%USERPROFILE%\.tauri\hangar.key`. Only this process gets the key and the password typed at
     the prompt (`getpass`, never `os.environ`), and with a reduced
     environment: `PATH`, the system, profile and temporary folders and the locale, nothing else
     (no `NODE_OPTIONS` or `NODE_PATH`, through which node would load other code). The `.sig` is
     the same as from `tauri build` (trusted comment
     `timestamp:...<TAB>file:<installer>.exe<TAB>version:<X.Y.Z+Build>`, measured with Tauri CLI 2.12.4
     and a throwaway key). An empty password is refused; a key without one is used only with
     `--allow-unencrypted-key`, and the script then warns
   - notes the installer's SHA-256 around the Authenticode check (the same before and after it;
     unsigned: right before the updater signer) and requires it before and after the signer, so the updater signs exactly
     the bytes Windows' signature check accepted
   - verifies the signature before anything is published: the key id in the `.sig` must match the
     `pubkey` in `tauri.conf.json` (printed in minisign's order, the same id as the comment
     `minisign public key: E34C168FC5B115C8`), and, once the copies under `release/` are made,
     the verifier built before the build (`verify_update_sig <release copy> <text file> <pubkey> <X.Y.Z+Build>`,
     `src-tauri/examples/verify_update_sig.rs`, run only if its SHA-256 is unchanged, no key in its
     environment) checks it the way
     installed apps do: the same `minisign-verify` call as `tauri-plugin-updater` 2.14.1, including
     the global signature over the trusted comment, which must carry exactly `version:<X.Y.Z+Build>`.
     It checks the copy that is uploaded against the signature text `latest.json` gets, written
     to a file of its own next to the copies (`.updater-signature.tmp`, removed right after), never
     the `.sig` again. On failure the `.sig` and the copies are deleted and the script aborts. The
     app itself requires the signed version (`requireSignedVersion`, [[process-launch-hardening]])
   - requires both copies under `release/` to have that SHA-256; a file changed in between
     aborts and leaves no installer behind. `latest.json` gets the signature text that was
     verified, not a fresh read of the `.sig`
   - runs PowerShell (`Get-AuthenticodeSignature`) without any `TAURI_SIGNING_PRIVATE_KEY*` variable
   - runs `git`, `node`, `cargo` and PowerShell only as absolute programs from `PATH` (on Windows
     only `<name>.exe`), never from a `PATH` folder inside the repository, the working folder itself or a
     relative `PATH` entry, and sets `NoDefaultCurrentDirectoryInExePath=1` for itself and every
     child process. Windows otherwise searches the working folder first and prefers `.com` over
     `.exe`, so a file planted in the repository (`node.com`, `git.exe`) could run instead and read
     the key or skip the clean working tree check
   - checks that `node_modules/@tauri-apps/cli/package.json` has the version `pnpm-lock.yaml` pins
     before running the CLI (see the residual risk under Keys)
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
   (at most 400 KB each) and `hangar-<scene>-dark.png` / `-light.png` for six scenes (at most
   300 KB each): one per most visible feature of the release. Since 0.11.0 these are `partners`,
   `messages`, `sub-agents`, `crew`, `dictation` and `server-check`; each release keeps exactly seven
   scenes and replaces the ones whose feature is no longer among the six best. The README and the website use these
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
   once every file is attached. The script ends with "Without Linux"; to ship Linux too, push first,
   then follow "Adding Linux" below and use the commands `--linux` prints instead of steps 2 and 3. The files go in the order setup, `Hangar-Setup.exe`,
   `SHA256SUMS.txt`, `latest.json`; with `--base-url` the script prints the same order for the
   upload, `latest.json` last because it announces the update.
6. Transition for old installations (0.1.x only poll the own server): also upload `latest.json` to
   `https://hangar.feoren.com/download/`. Its URL points to GitHub, so one build serves both. Once
   no 0.1.x installation is left, this step can go.
7. Create the upcoming version (the script prints the command at the end), after `--linux` if Linux
   ships: `py -3 tools/release.py --next 0.3.0`
   - sets the three files and `Cargo.lock` to `0.3.0` (without build number)
   - adds `{"version": "0.3.0", "date": null, "items": []}` at the top of `src/changelog.json`
   - aborts if the highest changelog version has no date yet or `0.3.0` is not higher than everything before
   - then commit: `chore: set upcoming version 0.3.0`

Website, download button (always the newest release):

```html
<a href="https://github.com/feoren/hangar/releases/latest/download/Hangar-Setup.exe" download>Download Hangar for Windows</a>
```

## Adding Linux
[[0013-linux-release]]: the CI job `linux-bundle` (ubuntu-22.04) builds the AppImage and the .deb
with updater artifacts off and uploads them unsigned as the workflow artifact `linux-bundles`
(kept 14 days). The updater key never goes to CI; the owner's machine signs, as for Windows.

Why a second step: the Linux bundles must carry the released `X.Y.Z+Build`, and only the release
commit (`chore: Version X.Y.Z-Build`, step 4) has it. That commit exists only after the Windows
run, so the CI run of that commit builds the Linux bundles of exactly this release.

1. Windows release as above up to step 4, then `git push origin main`. Wait for the CI job
   `linux-bundle` of that commit.
2. `py -3 tools/release.py --linux X.Y.Z` (before `--next`). It
   - requires the release state in the files (`X.Y.Z+Build` in `tauri.conf.json`), a clean working
     tree and `release/X.Y.Z-Build/latest.json` of the Windows run
   - finds the newest finished push run of `ci.yml` on `main` for `HEAD`
     (`gh run list --commit <HEAD> --json ...`, pull request runs never count) and downloads its
     artifact with `gh run download <run> --name linux-bundles` into
     `release/X.Y.Z-Build/linux-download/` (removed afterwards). `--linux-bundles <folder>` takes the
     files from a folder instead (that folder stays)
   - requires exactly one `.AppImage` and one `.deb` whose names carry this version
     (`Hangar_X.Y.Z+Build_amd64.AppImage`: the bundler names both after `productName`, the
     unchanged `version` of `tauri.conf.json` and the arch, read in tauri-bundler's source). A bundle
     of another commit aborts
   - signs the AppImage itself with `tauri signer sign --app-version X.Y.Z+Build`: same key, same
     password prompt and refusal of `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, same reduced environment
     and key id check as the installer. With `createUpdaterArtifacts: true` the signature covers the
     `.AppImage` file and the AppImage is the updater bundle itself (Tauri updater docs;
     `v1Compatible` would use a `.tar.gz` instead)
   - copies `Hangar_X.Y.Z-Build_amd64.AppImage` and `Hangar_X.Y.Z-Build_amd64.deb` to the release
     folder, checks their SHA-256 against the signed and the found bytes, and runs the verifier on
     the AppImage copy against the text `latest.json` gets, as for the installer
   - rewrites `SHA256SUMS.txt` over the installer, `Hangar-Setup.exe`, the AppImage and the .deb, and
     adds `platforms["linux-x86_64-appimage"]` (signature text, URL in the download folder the
     Windows entry already names, GitHub or `--base-url`) to `latest.json`; the Windows entry, the
     version, the notes and `pub_date` stay
   - prints the `gh` commands with all six files (setup, `Hangar-Setup.exe`, AppImage, .deb,
     `SHA256SUMS.txt`, `latest.json`), or the upload order for a self-hosted folder
3. Publish with those commands, then step 6 and step 7 as above.

Without the artifact (job failed, older than 14 days, run not finished) `--linux` aborts and
changes nothing; the release can still go out without Linux with the commands of the Windows run.

**Platform key.** `tauri-plugin-updater` 2.14.1 (`src/updater.rs`, `get_urls`) looks for
`linux-x86_64-<installer>` first and then `linux-x86_64`, the installer being the bundle type the
bundler patches into the binary (`__TAURI_BUNDLE_TYPE_VAR_APP` for AppImage, `_DEB` for .deb;
`tauri-utils` `platform::bundle_type`). A .deb installation installs an update with `dpkg -i` and
rejects anything that is not a .deb. A plain `linux-x86_64` entry would therefore offer the AppImage
to .deb installations, which would fail on install. `linux-x86_64-appimage` reaches AppImages only;
a .deb installation finds no entry, which a background check ignores (`src/useUpdater.ts`) and a
manual check reports as an error. The CI job checks that the AppImage's `hangar` binary carries the
AppImage bundle type; without it AppImages would get no update.

**File names.** The URL in `latest.json` is per release, so names with the version are fine; the
updater downloads the URL, checks the signature and writes the bytes over the running AppImage's
own path (`install_appimage`), so the user's file name stays as it was (read in the plugin source,
not measured). The Tauri updater docs say nothing about stable names.

> [!question] Unverified
> - Not yet run end to end: no Linux release has been published, and no AppImage has updated itself.
> - Whether a .deb installation should get updates too (a signed .deb under `linux-x86_64-deb`,
>   installed through `pkexec dpkg -i`); [[0013-linux-release]] says no for now.

## Release from a fresh clone
The clean tree check and the build input check cover the files the script knows about. Whatever
else an ignored file, an old `node_modules` or a file planted outside the checked names could
change is ruled out only by a checkout nothing has run in. Recommended for every release:

```powershell
git clone --no-local <path of the working repository> <new empty folder>
cd <new empty folder>
pnpm install --frozen-lockfile
cargo fetch --locked --manifest-path src-tauri/Cargo.toml
py -3 tools/release.py --version X.Y.Z
```

`--no-local` copies the objects instead of hard-linking them, so the clone shares no files with
the working repository. `cargo fetch` is needed because the verifier builds `--offline`. Close
every agent and editor working in the clone first, and check `%USERPROFILE%\.cargo\config.toml`
(Cargo reads it for every build; the script does not check it). Afterwards take
`release/X.Y.Z-Build/` and the version and changelog commit from the clone. A `--fresh-clone` mode
in the script is not implemented: it would have to run `pnpm install` itself, through the
`pnpm.cmd` shim the script otherwise avoids.

## Keys
- Private key: `%USERPROFILE%\.tauri\hangar.key`, deliberately outside the repo and any cloud sync
  folder. Public key: in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
- > [!warning] Keep it safe
  > If the private key is lost, installed apps can **no longer accept updates**;
  > users would have to reinstall. If it falls into the wrong hands, someone can sign updates
  > that every installation trusts. So keep an offline copy (e.g. a
  > password manager or USB stick), never in a cloud sync folder or Git.
- Different key file: set the environment variable `HANGAR_SIGNING_KEY`.
- **The key needs a password.** `release.py` asks for it when it signs and refuses an empty one;
  `--allow-unencrypted-key` exists only for the transition. Without a password, every program that
  can read `%USERPROFILE%\.tauri\hangar.key` can sign updates. Never set
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the shell: the script refuses to run then.

- > [!warning] Residual risk: `node_modules` is not integrity-checked
  > The signer is the Tauri CLI from `node_modules`, and it holds the key. The script only compares
  > the CLI's `package.json` version with `pnpm-lock.yaml`; whoever can write `node_modules`
  > (any build script or agent running in the repository) can change the CLI's code and both files.
  > So run the release from a fresh clone (see "Release from a fresh clone"), with no agent or
  > untrusted build running in it, and use a key password, which a planted CLI would still see
  > but which keeps a merely copied key file useless.

### Giving the existing key a password
A new key would change the public key, and installed copies would refuse every update signed with
it. Re-encrypting keeps the key pair and only changes the password.

Format, measured with Tauri CLI 2.12.4 and throwaway keys (2026-11-15): the key file is base64 of a
minisign secret key file (`untrusted comment: rsign encrypted secret key`, then base64 of the
158-byte key). The key is scrypt-encrypted **even with an empty password**: the bytes start with
`Ed` `Sc` `B2` (Ed25519, scrypt, BLAKE2b), then a 32-byte salt, opslimit 1048576 and memlimit
33554432 (8 bytes each, little-endian), then the 104 encrypted bytes (key id, secret key,
checksum), XORed with scrypt(password, salt). For these limits libsodium's parameter choice gives
N = 2^15, r = 8, p = 1, and the checksum is BLAKE2b-256 over `Ed`, key id and secret key.

Procedure. **Try every step on a throwaway key first**, and only then on `hangar.key`:

1. Back up `hangar.key` first (offline copy, see "Keep it safe"); without the key no update can
   ever be signed again.
2. Save the script below as `reencrypt_key.py` in `%USERPROFILE%\.tauri\` (it holds no secret).
   It reads both passwords with `getpass` (never from an argument, a variable or the shell
   history), keeps the decrypted key only in memory, checks the current password against the
   key's checksum, uses a new random salt and writes only the re-encrypted key, to
   `<key>.new`, which it never overwrites.
3. Throwaway run, in PowerShell in the repository:
   `node node_modules/@tauri-apps/cli/tauri.js signer generate --ci -p "" -w tests/.tmp/rekey/test.key`,
   then `py -3 $env:USERPROFILE\.tauri\reencrypt_key.py tests/.tmp/rekey/test.key` (Enter for
   the current password). The key id it prints must match the `.pub` file (for `hangar.key`:
   `minisign public key: E34C168FC5B115C8`, the comment of the `pubkey` in `tauri.conf.json`).
4. Check the result by signing a harmless file. Never pass the password with `-p` (shell history),
   and note that `tauri signer sign` does not ask for one: without the variable it assumes an
   empty password (measured with Tauri CLI 2.12.4). So set the variable for this one test only:
   ```powershell
   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Net.NetworkCredential]::new('', (Read-Host -AsSecureString 'New password')).Password
   node node_modules/@tauri-apps/cli/tauri.js signer sign -f <key>.new --app-version 0.0.1 <file>
   Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
   ```
   It must succeed (and fail without the variable, which means the empty password). Then
   `cargo run --locked --offline --manifest-path src-tauri/Cargo.toml --example verify_update_sig --
   <file> <file>.sig "<pubkey>" 0.0.1` must print "Updater signature verified". Close that
   PowerShell window afterwards; `release.py` refuses to run while the variable is set. Delete the
   throwaway folder.
5. Real run: `py -3 $env:USERPROFILE\.tauri\reencrypt_key.py $env:USERPROFILE\.tauri\hangar.key`,
   check `hangar.key.new` as in step 4, then replace `hangar.key` with it.
6. Replace the offline copy with the new file, and destroy every copy without a password (the old
   `hangar.key`, the old offline copy and any backup made in step 1) once a release has been
   signed with the new one. Delete `reencrypt_key.py`.

Verified on a throwaway key with this script (2026-11-15, Python 3.14, Tauri CLI 2.12.4): a wrong
current password writes nothing; the new key signs with the new password, the empty password is
refused, and the signature carries the unchanged key id and passes `verify_update_sig` against
the throwaway key's original public key.

```python
"""Gives a Tauri updater key a new password; the key pair and its key id stay the same.

Usage: py -3 reencrypt_key.py <key file>
Writes <key file>.new (never overwrites). Passwords are read with getpass only.
"""
import base64
import getpass
import hashlib
import hmac
import os
import sys

OPSLIMIT = 1048576
MEMLIMIT = 33554432


def keystream(password, salt):
    # libsodium's parameter choice for these limits: N = 2^15, r = 8, p = 1.
    return hashlib.scrypt(password.encode("utf-8"), salt=salt, n=1 << 15, r=8, p=1, maxmem=64 << 20, dklen=104)


def xor(data, stream):
    return bytes(a ^ b for a, b in zip(data, stream))


def main():
    source = sys.argv[1]
    target = source + ".new"
    lines = base64.b64decode(open(source, "rb").read()).decode("ascii").splitlines()
    raw = base64.b64decode(lines[1])
    if len(raw) != 158 or raw[:6] != b"EdScB2":
        sys.exit("not an scrypt-encrypted minisign key")
    salt = raw[6:38]
    if int.from_bytes(raw[38:46], "little") != OPSLIMIT or int.from_bytes(raw[46:54], "little") != MEMLIMIT:
        sys.exit("unexpected scrypt limits; nothing written")
    plain = xor(raw[54:], keystream(getpass.getpass("Current password (Enter if none): "), salt))
    key_id, secret, checksum = plain[:8], plain[8:72], plain[72:]
    if not hmac.compare_digest(hashlib.blake2b(b"Ed" + key_id + secret, digest_size=32).digest(), checksum):
        sys.exit("wrong current password; nothing written")
    new = getpass.getpass("New password: ")
    if not new or new != getpass.getpass("New password again: "):
        sys.exit("empty or different passwords; nothing written")
    new_salt = os.urandom(32)
    body = raw[:6] + new_salt + raw[38:54] + xor(plain, keystream(new, new_salt))
    text = "%s\n%s\n" % (lines[0], base64.b64encode(body).decode("ascii"))
    with open(target, "xb") as handle:
        handle.write(base64.b64encode(text.encode("ascii")))
    print("Written %s (key id %s). Test it before replacing the old key." % (target, key_id[::-1].hex().upper()))


if __name__ == "__main__":
    main()
```

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
unchanged; the script passes the values to `tauri build` (`node.exe node_modules/@tauri-apps/cli/tauri.js build`) as a temporary `--config` file and
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
- Windows x64 and Linux x64 (AppImage updates, .deb without). macOS needs `darwin-*` entries.
- The website has no Linux download link yet ([[0013-linux-release]], part 2).
