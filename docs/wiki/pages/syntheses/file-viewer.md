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
detached side panel window ([[notes]]): only the backend creates windows, with a fixed URL. The
tree's filter and dragging rows onto a terminal are described at the end.

## Commands

| Command | Caller | Arguments | Result |
|---|---|---|---|
| `open_file_viewer` | `main` only (capability and label check) | `sessionId: number`, `path: string` | `void`; opens a new viewer or focuses (unminimizes) the one already showing this file of this session |
| `file_viewer_content` | windows labelled `file-viewer-<n>` only | none | `FileContent` (below); reads the file again on every call (reload button, live refresh) |
| `file_viewer_stamp` | viewer windows only | none | `FileStamp { modified: number \| null, size: number }`: change time (ms since 1970) and size, for the live refresh |
| `file_viewer_diff` | viewer windows only | none | `HeadDiff` (below): the file against the last commit |
| `file_viewer_open_link` | viewer windows only | `url: string` | `void`; opens a plain http(s) address of the Markdown preview in the default browser |
| `list_session_paths` | `main` and `panel` | `id: number` | `PathIndex` (below): every path below the session's folder for the tree's filter |

`path` is relative to the session's folder exactly as `list_session_folder` takes `relative`:
`/` separated, no leading `/`, no `.`/`..`, no `\`, no `:`, no `.git` segment, at most 4096
bytes. Every folder on the way and the file itself must not be a link (symlink or junction); the
resolved path must lie below the resolved session folder, and the file must be a regular file.
The same check runs again on every `file_viewer_content`, `file_viewer_stamp` and
`file_viewer_diff` call, so a file that was deleted, replaced by a folder or swapped for a link
afterwards is refused.

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
  modified: number | null;  // change time in ms since 1970, the stamp the content was read with
  image: { mime: "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/svg+xml";
           data: string } | null;  // Base64 of the whole file
  imageTooLarge: boolean;   // an image file larger than 8 MiB: image is null
};
```

Content rules: at most 2 MiB are read; when cut, the cut drops a character the limit splits, so
the text never ends in U+FFFD because of the cut. Otherwise the bytes are decoded as UTF-8
lossily (invalid bytes become U+FFFD), one leading UTF-8 BOM is stripped, line endings (CRLF)
stay as they are. UTF-16 files contain NUL bytes and therefore show as binary.

Image rules: the extension (`png`, `jpg`/`jpeg`, `gif`, `webp`, `svg`, any case) names the type,
and the content must match it (PNG, JPEG and GIF magic numbers, `RIFF....WEBP`, an SVG names
`<svg` within its first 4 KiB and has no NUL there); otherwise the file is shown like any other.
Image files are read up to 8 MiB + 1 byte; larger ones set `imageTooLarge`. An SVG is text and
image at once.

## HeadDiff

```ts
type HeadDiff = {
  state: "notRepo" | "noHead" | "notInHead" | "same" | "changed";
  diff: string;       // unified diff, only for "changed"; cut at 2 MiB on a whole line
  truncated: boolean;
};
```

`git diff HEAD -- <file>` with the hardened runner and the checks of the changes panel
([[agent-workflow-features]], "Git"): `check_folder` on the file's folder (no git link to a share),
`toplevel` with the work tree check, every clean, smudge and process filter switched off
(`filter_overrides`), `--no-ext-diff --no-textconv --ignore-submodules=all`, and
`--literal-pathspecs`, so a file named `*.txt` compares only itself. It runs from the file's own
folder, so a file in a submodule or a nested repository compares with that repository. Staged
and unstaged changes both count: the comparison is with the commit, not the index. `notInHead`
comes from `git cat-file -e HEAD:<path>` (new, untracked or ignored files).

Residual risk, accepted as for every git call of the changes panel (`src-tauri/src/git.rs`,
`check_git_links`): the diff runs git in whatever repository holds the file, also a nested one
inside the session's folder. Its config is read by git, and `include.path`, alternates or other
paths in it are not followed by Hangar's checks; one naming a UNC path (`\\host\share`) would make
git touch that share, and Windows would send the user's NTLM credentials to that host. Only a
repository the user put into the session's folder can do that.

## Errors

Plain English text: `Unknown session <id>`, `Invalid file path: <reason>`, `The file lies outside
the session's folder`, `Links are not followed in the file viewer`, `The file no longer exists`,
`Not a file`, `Only the main window or the side panel may open a file viewer`, `Only a file viewer window may read
its file`, `Unknown file viewer window`, `The file cannot be read: <io error>`, `The file did not
answer in time` (15 s, as the file tree), `Only web addresses (http, https) open from the file
viewer`, `Links open only from the focused viewer, one per second`, `The browser could not be
opened: <io error>`, and the git errors of the changes panel.

## Frontend

- Opening: double click on a file row or Enter on a focused file in the "Files" tab
  (`FilesPanel.tsx`, `treeStep` returns `open`); links are never opened. A refused path shows
  `files.openFailed` in the panel for 6 s.
- Window: `main.tsx` renders `FileViewerWindow.tsx` for `?window=file`. The only Tauri calls are
  the four viewer commands and the clipboard plugin's `writeText` (copy path); the title comes
  from the backend, `document.title` only mirrors it. The content goes into CodeMirror as document
  text, never as HTML.
- Modes (`fileViewer.ts`, `viewerModes`, `viewerBody`): code, preview and diff. The header shows
  a preview switch for Markdown (`.md`, `.markdown`, `.mdown`, `.mkd`) and SVG files, and a diff
  switch only for a text file in a repository with a commit: the first text read asks
  `file_viewer_diff` once (`diffWorks`; hidden for `notRepo` and `noHead`). A raster image only
  ever shows its picture. Images open in the preview, everything else in the code view. The
  WebView's own context menu is suppressed (fields keep theirs).
- Markdown preview: `renderViewerMarkdown` (`markdownPreview.ts`, markdown-it already in the tree
  for notes) with `html: false`: HTML in the file shows as text, images show as their alt text
  (nothing is fetched; the CSP allows only `'self'` and `data:` images anyway), no `<a>` is
  rendered. An http(s) link becomes a focusable `span` with the address in `data-href`; a click or
  Enter hands it to `file_viewer_open_link`, which parses it again and opens only `http`/`https`
  with a host and without user name or password, at most 8 KiB, only while the viewer has the
  focus and at most once per second per viewer (`Viewers::allow_link`), through the same opener as
  the account pages (`desktop::open_url`).
  Any other link (relative, `mailto:`) stays text; markdown-it refuses `javascript:`, `file:` and
  non-image `data:` while parsing. Task checkboxes are disabled.
- Images: `<img src="data:<mime>;base64,...">` on a checkerboard, never scaled up. An SVG is only
  ever drawn through `<img>`, where it runs no script and loads nothing; it is never inlined.
- Diff: read on switching to it, on every reload while it is shown, and when the window comes
  back to the front (`focus`, `visibilitychange`; a commit changes it without changing the file),
  never on a timer; rendered with the changes
  panel's line classes (`splitDiff`, `lineKind`, 300 lines per "Show more"). States other than
  `changed` show one sentence (`diffNote`).
- Live refresh (`pollStamps`): every second, every 4 s for an image (`refreshInterval`), while
  the window is visible (`document.hidden`
  pauses it, as does a read under way) the window asks `file_viewer_stamp` and compares it with
  the stamp of what it shows; a new stamp that held for two asks in a row reads the file again
  (a file still being written is read once it settled). A file that cannot be read counts as the
  stamp `missing`. Polling and not a file watcher: no backend state per window, nothing to stop
  in the backend, and it ends with the window's page. Cost by design: one path resolution and
  `stat` per ask per visible viewer.
- Editor: `viewerEditor.ts` (read-only CodeMirror 6 with line numbers, folding, search) is a
  dynamic import, and `@codemirror/language-data` loads each language's parser on demand, so the
  main bundle carries neither.
- Look: `viewerThemes.ts` holds seven families (`oneDarkPro`, `github`, `vscode`, `darcula`,
  `solarized`, `catppuccin`, `monokai`), each with a dark and a light palette that follows the
  app theme; the choice is `viewerTheme` in the settings (Settings, Appearance). The palettes are
  applied as `--viewer-*` CSS variables, so a switch never rebuilds the editor. Token colors keep
  the originals' contrast (Solarized and Catppuccin Latte are below 4.5:1 for some tokens); the
  text, line number and active line number contrasts are pinned by `viewerThemes.test.ts`.
- Settings changes reach an open viewer through the `storage` event of the shared localStorage,
  as in the detached side panel window ([[notes]]).
- Tree colors: rows on the path to a file this worktree changed (`onChangePath` in
  `fileTree.ts`) show their name in the "working" color; the change letter and the folder dot say
  the same without color.

## Tree filter

A field above the tree ("Filter files"), shown only while it holds text or has the focus; the
magnifier button in the Files header opens and focuses it, and typing a character while the tree
has the focus opens it too (`startsFilter`: one code point, no Ctrl, Alt or Meta, not Space). Matching
(`filterScore`): every word of the query appears in the relative path (case-insensitive
substring) and at least one in the last name, so "src" finds the folder `src`, not everything
inside it, while "src main" finds `src/main.rs`. The best 300 matches (`MAX_FILTER_MATCHES`)
show with the folders that lead to them, all open, those folders dimmed; more matches show a hint.

Source: what the tree has read answers at once (`loadedPaths`); on the first character the window
asks `list_session_paths` once per filter and searches that index when it arrives. The index
(`FileTree::index` in `file_tree.rs`): in a repository whose top level git confirmed,
`git ls-files -z --cached --others --exclude-standard` in the session's folder (ignored files
left out, the folders derived from the paths, each path's kind from `symlink_metadata`, gone
files left out, and every path behind a link or junction on the way left out, as git lists
untracked files inside a junction; each folder is checked once); otherwise, or when git fails, a breadth-first walk that lists links but never
enters them. Bounds: 20,000 paths, depth 16 for the walk, 200,000 entries read, 5 s, 8 MiB of
`ls-files` output; beyond any of them `truncated` is set and the panel says not all files were
searched. Paths the tree could not name (not UTF-8, `\`, `:`, `.git`) are left out. One index
per session runs at a time (`InFlight`, key `\0index`), 15 s timeout.

Keys in the filtered tree (`filteredStep`): arrows move as in the tree, Left goes to the folder
above instead of closing one, Right/Enter/Space or a click on a folder clears the filter and
reveals that folder in the whole tree (`revealFolders`: the folders above it open and load);
Enter on a file opens the viewer. Escape in the field clears it; a second Escape closes the field
and returns to the tree; Escape on the tree clears the filter and reveals the focused row; Down or Enter in the field
moves to the first match.

## Dragging a row onto a terminal

A row of the tree dragged onto a terminal pane types its mention (`rowMention`, the same text as
the right click: `@path `, a folder with `/`, quoted when it has spaces) into the session under
the drop point, as a paste, and activates that session; the pane shows the drop hint while the
row is over it. Pointer events with pointer capture, not HTML5 drag and drop: Tauri's native
file drop owns that in the window (see `useSessionDrag.ts`). The drag starts beyond the session
drag's threshold, so clicks and double clicks stay; Escape cancels. Only the main window drags
(the detached panel has no panes). The mention is relative to the tree's folder, so only a pane
whose session works in that same folder takes the drop (`takesMention`); other panes show no
drop hint.

## Capability `file-viewer`

Windows `file-viewer-*`: `core:event:allow-listen`, `core:event:allow-unlisten`,
`core:window:allow-close`, `core:window:allow-set-focus`, `core:window:allow-set-title`,
`clipboard-manager:allow-write-text`, `allow-file-viewer-content`, `allow-file-viewer-stamp`,
`allow-file-viewer-diff`, `allow-file-viewer-open-link`. The main window holds
`allow-open-file-viewer`, never a permission to create webviews; the main window and the panel
hold `allow-list-session-paths`. `src-tauri/tests/contract.rs` pins all three lists.
