---
title: Terminal links
type: synthesis
tags: [terminal, links, xterm, security]
sources: ["[[xterm-linkifier]]", "[[windows-path-normalization-measurement]]"]
created: 2026-11-13
updated: 2026-11-13
---

# Terminal links

Web addresses and existing file or folder paths in a session's output are links. Code:
`src/terminalLinks.ts` (finding, segments, resolving), `src/pathCheck.ts` (existence cache),
`src/terminalLinkProvider.ts` (xterm), `src/terminalLinkOpen.ts` (what a click does),
`src-tauri/src/path_links.rs` (backend rules).

## Behavior

- Found: http(s) addresses, relative paths (`docs/a.md`, `./x`, `../x`), Windows (`C:\x`, `C:/x`)
  and Unix absolute paths, Git Bash `/c/...`, bare names with an extension, quoted paths with
  spaces, an optional `:line[:col]`; sentence punctuation and unpaired closing brackets are cut.
- Only existing paths are underlined. The frontend asks `stat_paths` when xterm asks for a line
  ([[xterm-linkifier]]), batched (40 ms, at most 64 paths per call) and cached (found 30 s,
  missing 3 s).
- A plain click opens; with Shift a path splits into segments, each opening the path up to it.
  When the program tracks the mouse, plain clicks stay the program's (links invisible and inert),
  Shift+click still opens.
- Address: default browser (`open_web_url`, plain http(s) without credentials). OSC 8 links open
  the same way.
- File inside the session's folder: Hangar's viewer, unless the viewer could only show "binary";
  otherwise and on refusal `open_path`.

## Backend rules (`open_path`, `stat_paths`)

- Only local absolute paths; no UNC or device paths, no data streams.
- Hover lookup never touches the network: paths on a `DRIVE_REMOTE` drive, paths whose
  canonical form is UNC, and a link or junction in the last segment count as missing.
- **Allowlist decision:** a file opens in its default program only when its canonical name has
  a harmless extension (text, data, code of compiled languages, images, PDF, SVG); everything
  else, including every script type Windows runs (`.js`, `.py`, `.cmd`, ...) and HTML, is only
  shown selected in the file manager. A denylist was rejected: no list of runnable types is
  complete, and Windows' handling of trailing dots and spaces ([[windows-path-normalization-measurement]])
  hides extensions from a check on the printed name. On Unix an execute bit also means reveal.
- Folders open through `ShellExecuteW` on Windows (explorer.exe splits its command line at
  commas); macOS bundle folders (`.app`, `.pkg`, ...) are revealed, never opened.
- Both opening commands run only while the main window has the focus.

## Limits

- `:line:col` is recognized but the viewer does not jump to the line.
- Sessions with a UNC working folder (WSL shares) get no path links.
- > [!question] Unverified
  > `FileType::is_symlink` reporting junctions as links on Windows is expected from std's
  > documentation but not measured.
