# Raw: `hangar-send` in the Linux and macOS bundles of CI run 45043329057

Read 2026-11-22 on Windows 11 for ADR 0012. The bundles are the CI artifacts of the run for commit
`2200d42` on `main` (workflow `CI`, triggered 2026-11-22T16:20:58Z, jobs `linux-bundle` and both
`macos-bundle` green; the run as a whole failed in the separate `audit` job). Downloaded with
`gh run download <run> -n linux-bundles` and `-n macos-bundles` into `<repo>\tests\.tmp\ci-artifacts\`,
listed with 7-Zip 7z.exe and Git Bash `tar`, nothing run or uploaded, deleted afterwards.

## Artifacts

```text
gh api repos/{owner}/{repo}/actions/runs/45043329057/artifacts
macos-bundles          63661691
linux-bundles         113507010
macos-bundles-aarch64  30708532
macos-start-aarch64      129771
linux-start-x64           50801
macos-bundles-x64      32953181
macos-start-x64          213347
```

## Linux

AppImage (`Hangar_0.14.0_amd64.AppImage`, ELF with the type 2 magic `41 49 02`), `7z l`:

```text
Type = SquashFS
2026-11-22 17:29:16 D....                            usr\bin
2026-11-22 17:29:38 .....     52679041     16476901  usr\bin\hangar
2026-11-22 17:29:39 .....       648969       249524  usr\bin\hangar-send
```

.deb (`Hangar_0.14.0_amd64.deb`, starts with `!<arch>\ndebian-binary`, members `control.tar.gz` and
`data.tar.gz`), `tar -tvf data.tar`:

```text
-rwxr-xr-x 0/0          639592 2026-11-22 17:30 usr/bin/hangar-send
-rwxr-xr-x 0/0        52630544 2026-11-22 17:30 usr/bin/hangar
-rw-r--r-- 0/0             185 2026-11-22 17:30 usr/share/applications/Hangar.desktop
```

## macOS

Updater archives, `tar -tvzf`:

```text
== Hangar_0.14.0_aarch64.app.tar.gz
-rwxr-xr-x runner/staff   619008 2026-11-22 17:24 Hangar.app/Contents/MacOS/hangar-send
-rwxr-xr-x runner/staff 40984192 2026-11-22 17:24 Hangar.app/Contents/MacOS/hangar
== Hangar_0.14.0_x64.app.tar.gz
-rwxr-xr-x runner/staff   610432 2026-11-22 17:32 Hangar.app/Contents/MacOS/hangar-send
-rwxr-xr-x runner/staff 43146784 2026-11-22 17:32 Hangar.app/Contents/MacOS/hangar
```

Disk images, `7z l` (`Type = Dmg`, `Type = HFS`):

```text
Hangar_0.14.0_aarch64.dmg
  Hangar\Hangar.app\Contents\_CodeSignature\CodeResources
  Hangar\Hangar.app\Contents\MacOS\hangar          40984192
  Hangar\Hangar.app\Contents\MacOS\hangar-send       619008
Hangar_0.14.0_x64.dmg
  Hangar\Hangar.app\Contents\MacOS\hangar          43146784
  Hangar\Hangar.app\Contents\MacOS\hangar-send       610432
```

## What CI itself checks (`.github/workflows/ci.yml`, read at `2200d42`)

- `linux-bundle`, step "Check bundles": `[ -x "$root/usr/bin/hangar-send" ]` in the extracted
  AppImage, `dpkg-deb -c` must list `usr/bin/hangar-send`; fails the job otherwise.
- `macos-bundle`, step "Check bundles", per architecture: `hangar` and `hangar-send` executable in
  `Contents/MacOS`, `lipo -archs` equal to the target, `minos` 13.0, `codesign --verify --deep
  --strict` of the app, and `Contents/MacOS/hangar-send` started without arguments must print
  `Usage: hangar-send`.

## How a session finds it (code read at `2200d42`)

- `src-tauri/src/lib.rs` (setup): `std::env::current_exe()`'s folder is handed to
  `send_bin::install`, which copies `hangar-send` into `app_local_data_dir()/bin` when the copy is
  missing or differs.
- `src-tauri/src/partner_service.rs` (`PartnerLaunch::prepare`): an agent session gets that folder
  as `path_dir` and `HANGAR_SEND=<port>:<token>`.
- `src-tauri/src/manager.rs` (`path_with`): the folder goes at the end of the child's `PATH`, after
  the profile's own `PATH`. Agents call it by name; no absolute path is handed out.
- So the bundled folder (`Contents/MacOS`, `/usr/bin`, or the AppImage's temporary mount
  `/tmp/.mount_*/usr/bin`) never goes on `PATH`; the copy lives in a stable per-user folder.
