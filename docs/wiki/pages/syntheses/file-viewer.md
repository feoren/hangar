---
title: File viewer window (contract)
type: synthesis
tags: [file-tree, window, contract, security]
sources: []
created: 2026-11-12
updated: 2026-11-12
---

# File viewer window (contract)

A read-only window that shows one file of a session's folder, opened from the "Files" tab of the
right panel. Code: `src-tauri/src/file_viewer.rs` (rules and tests), commands in
`src-tauri/src/commands.rs`, capability `src-tauri/capabilities/file-viewer.json`. Modelled on the
detached notes window ([[notes]]): only the backend creates windows, with a fixed URL and size.

## Commands

| Command | Caller | Arguments | Result |
|---|---|---|---|
| `open_file_viewer` | `main` only (capability and label check) | `sessionId: number`, `path: string` | `void`; opens a new viewer or focuses (unminimizes) the one already showing this file of this session |
| `file_viewer_content` | windows labelled `file-viewer-<n>` only | none | `FileContent` (below); reads the file again on every call (reload button) |

`path` is relative to the session's folder exactly as `list_session_folder` takes `relative`:
`/` separated, no leading `/`, no `.`/`..`, no `\`, no `:`, no `.git` segment, at most 4096
bytes. Every folder on the way and the file itself must not be a link (symlink or junction); the
resolved path must lie below the resolved session folder, and the file must be a regular file.
The same check runs again on every `file_viewer_content` call, so a file that was deleted,
replaced by a folder or swapped for a link afterwards is refused.

The viewer window names no path. The backend keeps `label -> { sessionId, root, absolute path,
relative path }` and looks up the calling window's label; a viewer can read exactly its one file.
The entry goes when the window is destroyed. Labels are never reused. A viewer stays bound to its
file after its session has ended; it keeps reading that file until the window is closed.

Remaining race, accepted: the check and the open are two steps, so a folder on the way swapped for
a junction between them would be followed once. Only a process running as the same user with
write access to the session's folder can do that, and it can read the target itself; the viewer
only shows the content to that user.

## Window

Label `file-viewer-<n>` (monotonic counter), URL `index.html?window=file`, 960 x 720 logical,
minimum 480 x 320, resizable, title = file name, default decorations.

## FileContent

```ts
type FileContent = {
  name: string;          // file name
  path: string;          // absolute, for display only (no \\?\ prefix)
  relativePath: string;  // as passed to open_file_viewer
  size: number;          // bytes on disk
  text: string | null;   // null when binary
  truncated: boolean;    // file larger than 2 MiB: text holds the first 2 MiB
  binary: boolean;       // NUL byte within the first 8 KiB
};
```

Content rules: at most 2 MiB are read; when cut, the cut drops a character the limit splits, so
the text never ends in U+FFFD because of the cut. Otherwise the bytes are decoded as UTF-8
lossily (invalid bytes become U+FFFD), one leading UTF-8 BOM is stripped, line endings (CRLF)
stay as they are. UTF-16 files contain NUL bytes and therefore show as binary.

## Errors

Plain English text: `Unknown session <id>`, `Invalid file path: <reason>`, `The file lies outside
the session's folder`, `Links are not followed in the file viewer`, `The file no longer exists`,
`Not a file`, `Only the main window may open a file viewer`, `Only a file viewer window may read
its file`, `Unknown file viewer window`, `The file cannot be read: <io error>`, `The file did not
answer in time` (15 s, as the file tree).

## Capability `file-viewer`

Windows `file-viewer-*`: `core:event:allow-listen`, `core:event:allow-unlisten`,
`core:window:allow-close`, `core:window:allow-set-focus`, `core:window:allow-set-title`,
`clipboard-manager:allow-write-text`, `allow-file-viewer-content`. The main window holds
`allow-open-file-viewer`, never a permission to create webviews. `src-tauri/tests/contract.rs`
pins both.
