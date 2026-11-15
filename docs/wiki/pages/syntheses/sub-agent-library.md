---
title: Sub-agent library and figures
type: synthesis
tags: [agents, claude-code, sub-agents, settings, figures]
sources: [[claude-agent-flag-measurement]], [[claude-agent-frontmatter-reading]]
created: 2026-11-15
updated: 2026-11-15
---

# Sub-agent library and figures

Specification and contract of Claude Code's sub-agent definitions in Hangar: listing them,
adding one from pasted text or a file, giving each a figure and starting Claude Code as one.
The view of running sub-agents is a different feature ([[agent-workflow-features]], item 23); it
only shows the figure of a known definition next to a running sub-agent of that type.

## Scope

- **Definitions:** Markdown files with a frontmatter (`name`, `description`, optional `model`,
  `color`, `tools`) in `~/.claude/agents` (global) and `<project>/.claude/agents` (project).
- **Settings > Sub-agents:** one row per name with figure, name, description, where it is
  defined (Global, Project <folder>, "from project <folder>, replaces the global one") and
  model; the whole figure (96 px high) is the button that opens the figure picker; actions Open
  (default editor) and Delete (global definitions only, asked inline). The picker names the
  built-in figures in words ("Developer 2") and marks only a figure the user chose, not one
  derived from the name. "New sub-agent" takes pasted text or a loaded `.md`
  file and saves it as a global definition.
- **Figures:** one per agent name, global. A built-in figure, an own image (PNG, GIF, WebP,
  JPEG, also animated, at most 2 MiB) chosen from a file or pasted from the clipboard, or the
  default.
- **Starting:** Claude Code profiles (`takesAgent`) start as a definition with `--agent <name>`:
  from a small chevron beside the program in the sidebar's program list, shown only with at
  least one definition for the start target folder, which opens a menu (`agentStartItems`,
  the popup menu's keyboard: arrows, Enter, Escape) with each definition's face, name, origin
  and the first line of its description; and from quick access ("Start Claude Code as backend
  in hangar" per start place, a project definition's origin in its detail).
- **Session look:** a session started as a definition shows the figure in place of the
  program badge, with the badge small (42 %) on its lower right corner and a tooltip naming
  program and agent (`SessionLook.tsx`). Where there is room the whole figure stands there
  (`crop="full"`, a 2 : 3 portrait box): sidebar rows in the avatar form (30 px high, 42 px on a
  card), the rail (34 px), quick access and the broadcast dialog (28 px). Where there is not, the
  face takes the badge's size (`crop="head"`): the 14 px title line of a program group, search
  results (16 px), the split view's pane headers, the peek, the overview tiles and the drag ghost
  (18 px). Group headers keep the program badge: they stand for the program, not one agent.
- **Running sub-agents:** the sub-agent tree shows the whole figure (36 px high) of a running
  sub-agent whose `agentType` names a listed definition. The crew column ([[crew-column]]) shows
  every running sub-agent, a built-in type without a definition with the figure its name points
  to.

## Backend (`src-tauri/src/agent_defs.rs`, `agent_figures.rs`)

### Reading definitions

- The frontmatter is read leniently, not as strict YAML: the user's own files hold values like
  `description: Backend: logic, APIs` that strict YAML refuses. Keys start a line; indented lines
  continue a value; `|` and `>` blocks, quoted values, `[a, b]` and `- a` lists are understood;
  `tools` may be a comma separated text or a list. The first of two equal keys wins.
- Limits: 256 KiB per file, 256 files per folder (the rest is one `tooMany` problem), 32 project
  folders per listing, 2000 characters of description to the window.
- A broken file becomes a problem entry (`tooLarge`, `notText`, `noFrontmatter`, `missingName`,
  `invalidName`, `missingDescription`, `duplicate`, `link`, `unreadable`, `tooMany`), never an
  error of the listing. Settings lists them under "Not read".
- Links: a file that is a symlink or any reparse point is not read (`link`). The agents folder
  itself is reached with `no_links::metadata(.., FollowLocal)`: a local link (a dotfiles setup)
  is followed, one to another computer and network or device paths are not.
- Project folders come from the window (start places and the sessions' working folders), must be
  absolute and are read only below `<folder>/.claude/agents`; the home folder counts as global.

### Names

`valid_name`: lowercase ASCII letters, digits and hyphens, starting and ending with a letter or
digit, at most 64. A definition with another name is `invalidName`. The same rule decides what
`--agent` may get, so the name never needs quoting on a command line or in a cmd.exe launcher.

> [!question] Unverified
> Claude Code's own rule for names is assumed from its documentation ("lowercase letters and
> hyphens"); the 2.1.325 binary names no rule for agents ([[claude-agent-flag-measurement]]).

### Same name in several places

The listing keeps every file. The window groups one name into one row (Global and each project
named) and, for a start in a folder, takes that project's definition before the global one
(`defsFor`). Claude Code is assumed to resolve the name the same way at runtime; Hangar only
passes the name.

> [!question] Unverified
> That a project definition outranks a global one of the same name in Claude Code.

### Project definitions are marked

A repository brings its own definitions. Wherever one is offered (menu, quick access, Settings,
the session's tooltip) it says "from project <folder>", and "replaces the global one" when a
global definition has its name. Figures belong to names and so to the global definitions: a
project definition that overrides a global one never wears that figure but its initial on a
neutral grey (`defFigure`), so a repository cannot pass its agent off as the trusted one. In
the sub-agent tree, which knows no folder, any such override keeps the global figure off.

### Fields beyond the prompt

Claude Code 2.1.325 applies an agent's own frontmatter `hooks` and `mcpServers` also to the
main-thread agent (`--agent`) and skips them when the definition's folder is not trusted; a
frontmatter `permissionMode` is ignored where it would widen the inherited mode
([[claude-agent-frontmatter-reading]]). The backend reports which of these fields a definition
sets (`extras`); the window names them for project definitions ("brings hooks, mcpServers").

### Writing definitions

- `create_sub_agent(text, replace)`: checks the text like a file (same parser, same name rule),
  writes `~/.claude/agents/<name>.md` atomically (temporary file and rename, `persist::write_atomic`),
  CRLF becomes LF and a last line break is added. A definition of that name, or a file
  `<name>.md`, is only replaced with `replace`; the window asks first ("exists" error). Replacing
  writes the existing definition's own file, whatever it is called. A link is never written
  through.
- `delete_sub_agent(name)`: global definitions only; project files belong to their repository.
- `open_sub_agent(name, folder)`: opens the file found by the backend's own listing in the default
  editor; the window never names a path.
- A file that merely holds the name (a broken file, another definition, another case of the
  name such as `Backend.md`) is never offered for replacing: the new definition takes the first
  free name (`<name>-2.md` and so on, compared without case). Only a definition of that name is
  replaced, in its own file, after the window asked.
- A new file never overwrites one: the text goes to a temporary file in the same folder (unique
  per process and call), which is hard linked to its name (fails with `AlreadyExists` when taken); without hard links (FAT,
  some network drives) the file is created with `create_new`. Replacing uses the atomic rename.
- Windows device names (`con`, `nul`, `aux`, `prn`, `com1` to `com9`, `lpt1` to `lpt9`) are
  refused when a definition is created (`reserved`); existing definitions of such a name stay
  listed and startable.
- Errors reach the window as `{ kind, message, problem }` (`tooLarge`, `invalid` with the problem,
  `exists`, `reserved`, `notFound`, `io`).

### Figures

- Choice: `agent-figures.json` in the config folder, `{ version: 1, figures: { <name>: { kind:
  "builtin", id } | { kind: "image", file } } }`, at most 256 entries. Unknown ids, invalid names
  and image file names that are not `<sha256>.<png|gif|webp|jpg>` are dropped when read, so the
  file can never point outside the image folder.
- Limits when read: `agent-figures.json` at most 256 KiB, each own image at most 2 MiB read from
  disk (never more, no link followed: `persist::read_regular_capped`), at most 32 MiB of own
  images in one answer; images past it fall back to the derived figure.
- Own images: `agent-figures/<sha256>.<ext>` in the app's data folder, never in the user's
  `.claude` folder. The type comes from the bytes (`avatar::sniff`: PNG, JPEG, GIF87a/89a, WebP);
  SVG is refused (it can carry script). Checked again when read; a damaged file is left out.
  Unused image files are removed after every change.
- To the window as `data:` URLs (`sub_agent_figures`); the CSP allows no other images.
- Upload: the window reads the chosen file and sends it as Base64 (`set_sub_agent_figure`); the
  backend decodes at most 2 MiB worth of Base64.
- Clipboard: `paste_sub_agent_figure` reads the clipboard image with the same code as the
  screenshot paste (`clipboard_image::read`), scales it down to 256 px on the longer side (box
  filter with premultiplied alpha) and saves it as PNG. An animated clipboard image arrives as one
  frame.
- Errors reach the window as text with a stable code first (`[no_image]`, `[not_an_image]`,
  `[too_large]`, `[too_many]`, `[unknown_figure]`, `[invalid_name]`), as `ManagerError` does; the
  window words the first three itself (`figureErrorCode`) and never matches the English text.

### Starting as a definition

- Catalog field `agent_args` (Claude Code: `["--agent", "{agent}"]`, [[claude-agent-flag-measurement]]),
  inherited by a user profile that starts the catalog's program, like `prompt_args`.
  `ProfileView.takesAgent` tells the window.
- `create_session(..., subAgent)`: the manager refuses a name `valid_name` refuses
  (`InvalidSubAgent`) and a program without `agent_args` (`NoSubAgentStart`). The arguments go
  right after the start arguments and before a positional first prompt, as an argv array.
- The session keeps the name (`SessionInfo.subAgent`) across restarts and resume fallbacks, the
  workspace stores it (`SavedSession.subAgent`, checked again on save and load), and a restore
  starts it as that definition again. The session is named after the definition.

> [!question] Unverified
> Whether `--agent` combined with `--resume <id>` keeps the agent in the resumed conversation.

## Window

- `subAgentLibrary.ts`: pure checks of the backend's answers, grouping, `defsFor`, default figure.
- `subAgentLibraryStore.ts`: one shared state; read on start, when the set of folders changes,
  when Settings opens and after a change.
- `AgentFigure.tsx`, `SessionLook.tsx`, `SubAgentSettings.tsx`, `agentStartMenu.tsx` and its
  chevron in `Sidebar.tsx`, quick access items in `launcherItems.ts`, the figure in
  `SubAgentTree.tsx`.

### Default figure

Without a chosen figure: the role figure a word of the name points to (`backend`, `api` and
`server` to backend, `dev` to developer, `ui` to frontend, `reviewer` and `lead` to manager, `qa`
and `test` to tester, and so on), else the initial on the frontmatter color (Claude Code's color
names), else a built-in figure picked by a hash of the name, the same on every start.

### Built-in figures

Twelve chibi figures supplied by the owner (`backend-01`, `developer-01/02`, `frontend-01/02`,
`manager-01/02`, `researcher-01/02`, `security-01/02`, `tester-01`), shrunk from 1254 px PNGs to
portrait WebP with alpha, 240 x 360 px (2 : 3), by `tools/figures.py <in> <out>`: the empty margin
(the figures filled only about half the width of the square originals) is cut to the visible
pixels, the figure fills the height and stands on the bottom edge, so the head sits at the same
height in every file (Lanczos, quality 82, 15 to 28 KiB each).
Vite bundles them (`import.meta.glob`); the Rust list `agent_figures::BUILTIN`, the TypeScript
list `BUILTIN_FIGURES` and the files are kept equal by tests.

### Whole figure and head

`AgentFigure` draws the whole figure (`crop="full"`) in a 2 : 3 box, `size` being its height, and
for tight places the face (`crop="head"`) in a round box: the top 39 % of the file, centered
(`.agent-figure.head img`: width 170 %, left -35 %, top 0). An own image is always shown whole
(`object-fit: contain`), also in the round box. An initial on its color is a round badge in the
head form and a tall capsule in the full form.

### Motion

None. Figures stand still (the old breathing and bobbing, `busy` and the `still` flag are gone);
only the attention ring around a waiting session's figure in the rail pulses, like a badge's. An
own animated GIF or WebP keeps its own animation.
