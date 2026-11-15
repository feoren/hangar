---
title: Terminal links
type: synthesis
tags: [terminal, links, xterm, security]
sources: ["[[xterm-linkifier]]", "[[windows-path-normalization-measurement]]"]
created: 2026-11-13
updated: 2026-11-15
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
  the same way, with one guard (security audit 2026-11-15): their visible text can claim another
  address than they lead to. Hovering shows the real target in a tip, its host in bold; a click
  whose visible text misleads shows the real address (host in bold) with an Open button first
  (`osc8Decision` in `src/termSafety.ts`). An OSC 8 target with a user name or password
  (`https://github.com@example.net/`) is no link at all (`safeLinkUrl`).
- What counts as the visible text (second audit round): the link's cells plus every non-blank
  cell touching them, on the same row and through wrapped rows in both directions
  (`linkWord` in `src/terminalLinkWord.ts`; wide CJK and emoji cells count once). So plain
  `https://github.com/` printed right before a link whose own text is `acme` counts as
  `https://github.com/acme`. The text is read word by word (split at white space), each word
  without the brackets, quotes and sentence punctuation around it, so `(github.com).`,
  `see github.com/org` and `Open https://github.com/org` all claim GitHub.
- A click asks when the text holds any `Default_Ignorable_Code_Point` (zero-width space, soft
  hyphen, word joiner, bidi marks and overrides; a variation selector or joiner inside an emoji
  is fine), when a word names another host (with a scheme, a bare host such as
  `github.com/org`, `localhost:3000`, an IPv4 address or a bracketed IPv6 address; read after
  NFKC with U+3002, U+FF0E and U+FF61 as dots, and with `github .com` joined), when a word looks
  like a path, and when a word is a bare file name (`install.sh`, `update.zip`, `README.md`)
  unless the target's last path name is that very file (also with a line and column:
  `app.ts:42` on a link to `.../app.ts#L42`). A leading `www.` names the same site as the bare
  host (`example.com` on `https://www.example.com/`). Plain words, issue and PR numbers
  (`PR #123`) and versions (`v0.8.0`) open at once; `src/termSafety.test.ts` keeps a table of
  common agent link texts that must not ask.
- Known limits: `github. com` (space only after the dot) and short IPv4 forms (`127.1`) are not
  read as hosts; a link text such as `Node.js` reads like a file name, so a link to `nodejs.org` asks before it
  opens.
- The file viewer's Markdown preview applies the same check to its links (`src/viewerLink.ts`):
  `[github.com/org](https://example.net/)` shows the real address with Open and Cancel (focused,
  also Escape) before anything opens; links with a user name stay plain text.
- Link trimming walks back from the end once, with the brackets counted in one pass; the former
  loop was quadratic and took about 30 ms for a line of 4,000 closing brackets.
- File inside the session's folder: Hangar's viewer, unless the viewer could only show "binary";
  otherwise and on refusal `open_path`.

## Backend rules (`open_path`, `stat_paths`)

- Only local absolute paths; no UNC or device paths, no data streams.
- Hover lookup never touches the network: paths on a `DRIVE_REMOTE` drive, paths whose
  canonical form is UNC, and any path with a symlink or junction in any segment whose target
  (read with `read_link`, which does not touch it) is no local fixed drive path count as missing
  and are not opened. Links to local targets are followed, at most 8 in a row
  (`src-tauri/src/no_links.rs`); the segments are checked from the root down, so no lookup passes
  an unchecked link. On Windows only name surrogate reparse points (symlink, junction, mount
  point) count as links; the tag comes from `FindFirstFileW`. A volume mount point names a
  `\\?\Volume{...}` target and is therefore refused too.
- **Allowlist decision:** a file opens in its default program only when its canonical name has
  a harmless extension (text, data, code of compiled languages, raster images, PDF); everything
  else, including every script type Windows runs (`.js`, `.py`, `.cmd`, ...), HTML, SVG and XML
  (their default program may be a browser, which runs the script they can carry), is only
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
