---
title: "Measurement: Git with * -text -diff -merge for encrypted note files"
type: source
tags: [git, gitattributes, merge, conflicts, windows, notes, measurement]
sources: []
raw: raw/2026-11-10-note-gitattributes-measurement.md
url:
retrieved: 2026-11-10
created: 2026-11-10
updated: 2026-11-10
---

# Measurement: Git with `* -text -diff -merge` for encrypted note files

Own measurement on Windows 11 (build 26300) with git 2.56.0.windows.4, for the phase 1 blocker of
[[0006-encrypted-project-notes]]: does `.hangar/notes/.gitattributes` with `* -text -diff -merge`
keep the bytes of note files stable and leave a conflict that Hangar can resolve itself? Shell
script in fresh repositories with `core.autocrlf` `true`, `false` and `input`, plus the app's own
conflict test. Raw data: `raw/2026-11-10-note-gitattributes-measurement.md`.

## Key points
- **`-text`:** with every `core.autocrlf` value the note file comes back from a fresh checkout byte
  for byte (LF stays LF, a CR inside stays, nothing is added), and the committed blob keeps the CR.
  The comparison file without attributes is checked out with CRLF under `autocrlf=true`.
- **`-diff`:** `git diff` prints "Binary files ... differ" for a note instead of a text diff.
- **`-merge`:** when both branches changed a note, `git merge` exits 1, `git status` shows `UU`,
  the working tree keeps "ours" with no conflict markers, and the index holds stages 1, 2 and 3,
  readable with `git cat-file blob :2:<path>` and `:3:<path>`. Writing the file and `git add`
  resolves it (`M ` afterwards).
- **Modify/delete:** `UD` with stages 1 and 2 (the deleted side has no stage).
- **Rebase:** `UU` as well, but stage 2 is the upstream and stage 3 the commit being replayed, and
  the working tree holds the upstream's version. "Ours" and "theirs" swap meaning, so Hangar
  decides by the `updated` time inside the note, never by the stage number.
- **Add/add of `vault.json`** (two branches turned notes on): `AA` with stages 2 and 3, working
  tree "ours". Identical `.gitattributes` and `README.md` on both sides merge without conflict.
- The app's test `merge_conflicts_are_resolved_without_loss` (`src-tauri/src/project_notes/tests.rs`)
  repeats the merge case end to end through `Git::run`: both versions survive, both paths are
  staged, nothing is unmerged and the merge commits.

## Significance for Hangar
- The ADR's [UNVERIFIED] point is settled for Windows: `.gitattributes` stays as decided, and
  `project_notes.rs` reads stages 2 and 3 and resolves `UU`, `AA`, `UD` and `DU` by the rules of
  ADR 0006 section 6. Because rebase swaps the stages, the newer `updated` time decides.
- macOS and Linux are not measured yet; Git's attribute handling is the same code there, but the
  claim stays open until measured on those platforms.
