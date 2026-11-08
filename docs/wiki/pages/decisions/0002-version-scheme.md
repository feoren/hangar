---
title: "ADR 0002: Version scheme Major.Minor.Revision-Build"
type: decision
status: accepted
superseded_by:
tags: [release, version, updater, windows]
sources: ["[[version-number-wikipedia]]", "[[tauri-updater-and-nsis]]"]
created: 2026-11-07
updated: 2026-11-08
---

# ADR 0002: Version scheme

## Context
The user wants versions in the scheme `N.N.N-NNNN`, for example `0.12.224-1548`, following
[[version-number-wikipedia]]. In that order they are the major version, minor version,
revision and build number. The build number is never reset. With version control, a common choice is a
number that uniquely identifies the source state.

Technical constraints:
- The Tauri updater compares versions by SemVer ([[tauri-updater-and-nsis]]). In SemVer,
  `0.12.224-1548` would be a pre-release that sorts before `0.12.224`.
- Tauri's NSIS bundler derives `VIProductVersion` (the Windows file version, four numbers) from the
  SemVer **build metadata**: `0.12.224+1548` becomes `0.12.224.1548`. With `-1548` it would write
  `0.12.224.0`. Evidence: `try_add_numeric_build_number` in `tauri-bundler/src/bundle/windows/nsis/mod.rs`,
  retrieved 2026-11-07.

## Options
Not recorded separately; the constraints above leave the split into an internal and a displayed form.

## Decision
- **Internally** (`tauri.conf.json`, `Cargo.toml`, `package.json`, `latest.json`): `X.Y.Z+B`, for example `0.12.224+1548`.
- **Displayed and in file names:** `X.Y.Z-B`, for example `0.12.224-1548`. A `+` is awkward in download URLs because
  forms treat it as a space. Converted in the frontend by `formatVersion` in `src/version.ts`, in the script
  by `display()` in `tools/release.py`.
- **Build number** = `buildNumberBase` from `package.json` plus the number of commits in the built state
  (`git rev-list --count HEAD`). It grows with every commit and is never reset. The base (84) holds the
  commit count of the history before the public start on 2026-11-08, when the repository began with a
  fresh history; without it the numbers would have gone down. The commit for build `B` can be found with
  `git rev-list --reverse HEAD | sed -n <B minus base>p` (on `main`). `tools/release.py` and
  `vite.config.ts` both read the base.
- **Every release increments X, Y or Z.** The updater ignores build metadata when comparing, so a
  release that differs only in its build number would not be detected as an update. `tools/release.py` refuses that.
- **The code always carries the upcoming version** (added 2026-11-08). `tauri.conf.json`, `package.json` and
  `Cargo.toml` (with `Cargo.lock`) state the version that will be released next, not the last
  published one. It is also the highest version in `src/changelog.json`, where it has no date until the release
  (`"date": null`). Reason: after the 0.1.0 release the files still said 0.1.0 while the changelog
  was already collecting changes under 0.2.0. The app showed `0.1.0-53`, What's New showed 0.2.0.
- When to increment which part: X for fundamental or incompatible changes (while X is 0, Y may also
  jump further), Y for new features, Z for bug fixes.

## Consequences
- `py -3 tools/release.py --version 0.12.224` sets `0.12.224+<Build>` in all three files, builds and creates
  `release/0.12.224-<Build>/` containing `Hangar_0.12.224-<Build>_x64-setup.exe`.
- The app displays the version as `0.12.224-1548`: in the About Hangar dialog, in the sidebar footer and in the
  update notice. Development builds show the upcoming version; the build number is set by `vite.config.ts`
  (`__HANGAR_BUILD__`, `withBuild` in `src/version.ts`).
- What's New (`src/WhatsNewDialog.tsx`) shows the same full version as the sidebar for the entry of the running
  X.Y.Z (`entryHeading` in `src/changelog.ts`, displayed through `VersionText`) along with the label
  "this version"; other entries stay at X.Y.Z.
- Consistency is checked: `src/versionSync.test.ts` (Vitest) requires the same version in all three files and
  that it is the highest in the changelog. `tools/release.py --version X.Y.Z` requires it to equal the highest
  changelog version, to be no lower than the version in the files and to be greater than the last published one
  (highest dated changelog entry or folder under `release/`). The earlier check "greater than the files if
  they carry a build number" is gone; under the new rule it no longer applied and, after a `--no-build` run,
  would have blocked the real release of the same version.
- After the release, `py -3 tools/release.py --next X.Y.Z` creates the upcoming version: files without a build number,
  an empty changelog entry without a date. The schema in `src/changelog.ts` allows empty items only for this
  newest, undated entry; the dialog hides it while it is empty.
- The release script only builds from a clean working tree (`--allow-dirty` as the exception), so that the
  build number truly identifies the source state.

## Evidence
- [[version-number-wikipedia]], [[tauri-updater-and-nsis]], `try_add_numeric_build_number` (see Context).
- Code: `src/version.ts`, `src/changelog.ts`, `tools/release.py`; test `src/versionSync.test.ts`.
