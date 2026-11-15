---
title: git behavior behind the timeline's jump back
type: source
tags: [git, timeline, measurement, windows, security]
sources: []
created: 2026-11-15
updated: 2026-11-15
raw: raw/2026-11-15-timeline-restore-measurement.md
url: ""
retrieved: 2026-11-15
---

# git behavior behind the timeline's jump back

Own measurement with git 2.56.0.windows.4 and rustc 1.100.0 on Windows 11; record in
[[2026-11-15-timeline-restore-measurement]].

## Key points

- `git checkout-index --prefix=<absolute path>/` accepts a drive letter path, creates the folders
  below it and converts line endings by the attributes of the entry's own path (also those of a
  `.gitattributes` in a subfolder). The work tree is not touched.
- `add --all` leaves an assume-unchanged entry at its old blob even when the file changed;
  `update-index --no-assume-unchanged -z --stdin` on a copy of the index clears the bit, the
  user's index keeps it.
- `git gc --prune=1.hour.ago` in a repository whose objects partly live in an alternate (given by
  `GIT_ALTERNATE_OBJECT_DIRECTORIES`) packs only its own objects.
- `std::fs::File::try_lock` conflicts with a shared lock held by another handle of the same
  process, so one process can test the cross-process lock.

## Significance for Hangar

The timeline stages a jump back's files in Hangar's data folder and installs them itself
([[work-tree-timeline]]).

> [!question] Unverified
> NTFS short names were not measured: 8dot3 names are disabled on the test volume. That
> `std::fs::canonicalize` returns the long name of a folder named by its short name rests on
> the documented behavior of `GetFinalPathNameByHandleW`. Other git versions were not measured.
