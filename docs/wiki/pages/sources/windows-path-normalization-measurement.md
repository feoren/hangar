---
title: Windows path normalization measurement
type: source
tags: [windows, paths, security]
sources: []
created: 2026-11-13
updated: 2026-11-13
raw: raw/2026-11-13-windows-path-normalization-measurement.md
url: ""
retrieved: 2026-11-13
---

# Windows path normalization measurement

Own measurement on Windows 11 10.0.26300, NTFS, Rust `std::fs`; record in
[[2026-11-13-windows-path-normalization-measurement]].

## Key points

- Windows drops trailing dots and spaces: `marker.cmd.` and `marker.cmd ` both find
  `marker.cmd`. A check on the printed name sees no `.cmd` extension, the file opened is a script.
- `fs::canonicalize` returns the real name (`\\?\E:\...\marker.cmd`); local drive paths start with
  `\\?\<drive>:\`, network paths would start with `\\?\UNC\`.
- 8.3 short names: not measured, the test volume creates none.
  > [!question] Unverified
  > That `canonicalize` expands a short name like `MARKER~1.CMD` to the long name is expected from
  > `GetFinalPathNameByHandleW` but not measured here.

## Significance for Hangar

Opening a clicked path decides on the canonical name, never on the printed one, and the opened
types are an allowlist. See [[terminal-links]].
