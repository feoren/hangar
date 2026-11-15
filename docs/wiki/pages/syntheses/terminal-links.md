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
- What counts as the visible text (second audit round, widened in the third): the link's cells
  plus every non-blank cell touching them, on the same row and through wrapped rows in both
  directions (`linkWord` in `src/terminalLinkWord.ts`; wide CJK and emoji cells count once). A
  hard line break counts as a wrap when it looks like one: the row above is filled up to its
  last column and the row below starts with text. So plain `https://github.com/` printed right
  before a link whose own text is `acme` counts as `https://github.com/acme`, also across such a
  break. Cells nobody sees end the word like a space: drawn invisible (SGR 8), two palette colors
  of the same index (bold base colors count as their bright variant) or two RGB colors with a
  contrast ratio below 1.1 (`cellHidden`). Default colors and mixed color kinds count as visible,
  since the color scheme is not known there. The color rules apply only while
  `minimumContrastRatio` is 1 (dark theme): above it (light theme, 4.5) xterm's renderer lifts
  text in its background color until it reads (`_applyMinimumContrast` in
  `DomRendererRowFactory.ts`), so there only SGR 8 hides a cell; otherwise a shown prefix would
  drop out of the word and a misleading link would open at once.
- Decision (third audit round, inverted from a blocklist): a click opens at once only when the
  visible text names no address at all, or when everything it names is the target. The text is
  read as a person sees it: NFKC, combining marks (`\p{M}`) dropped, UTS 39 look-alikes of the
  dot (U+3002, U+FF0E, U+FF61, U+A4F8, U+0660, U+06F0, U+30FB, U+FF65, U+00B7, U+2027, U+2219,
  U+22C5) and of the slash (U+2215, U+2044, U+29F8) taken for them, any other non-ASCII sign
  between two ASCII letters or digits taken for a dot, `github .com` joined. Each word (split at
  white space) is then searched, not matched whole: every run of letters, digits, dots and
  hyphens is a candidate, so no sign around a host hides it (`**github.com**`, `|...|`,
  `url=github.com`, `x;github.com`, `git@github.com:org/repo`, `github.com:org/repo`). A host run
  (`localhost`, IPv4, labels ending in a letter TLD or `xn--`, or anything right after
  `scheme://`) must be the target's site (`www.` ignored), with the target's port if it names one
  (`:3000`, compared with the default port of the target's scheme) and the target's scheme if it
  names one; a non-ASCII letter at a dot of a host run asks. A file name (`README.md`,
  `install.sh`) must be the target's last path name (also `app.ts:42` on `.../app.ts#L42`); any
  name in the path behind the target's own host (`vercel/next.js`, `acme/app.git`) is a page of
  that site, not a host. A path (`/x`, `./x`, `~/x`, `C:\x`, `\\server`,
  also behind a sign) and any `Default_Ignorable_Code_Point` (an emoji's own joiner and variation
  selector excepted) ask. A target whose host has an `xn--` label always asks.
  `src/termSafety.test.ts` keeps the auditor's inputs as tests and a table of common agent link
  texts that must not ask (`PR #123`, `#42`, `docs`, `v0.8.0`, `Python 3.12`, `README.md` and
  `app.ts:42` on that file, `localhost:3000` on the same port, `github.com/org/repo` on that URL,
  `www.` variants).
- Known limits: `github. com` (space only after the dot) and short IPv4 forms (`127.1`) are not
  read as hosts. Some honest texts ask once: `Node.js` on `nodejs.org` (reads like a file name),
  library names that read like hosts (`socket.io`, `lodash.debounce`, `foo.bar()`), `St.Louis`, `http://` shown for an
  `https://` target, any link to an internationalized host.
- Printed (plain text) addresses: hovering shows where they lead in the same tip as OSC 8 links.
  An address is read from visible cells only, so an invisible rest printed right after a shown
  address is not part of it (`logicalLine` in `src/terminalLinkProvider.ts`).
- The file viewer's Markdown preview applies the same check to its links (`src/viewerLink.ts`):
  `[github.com/org](https://example.net/)` shows the real address with Open and Cancel (focused,
  also Escape) before anything opens; links with a user name stay plain text. Since the third
  audit round the link is judged with the text touching it in its block (paragraph, list item,
  table cell, heading, quote), widened to white space on both sides (`wordAround`), so
  `github.com/[acme](https://example.net/)`, a code span before the link and a neighbouring link
  (`[github.com/](https://github.com/)[acme](https://example.net/)`) count as `github.com/acme`.
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
