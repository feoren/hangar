---
title: "ADR 0019: Anonymous usage statistics with opt-in"
type: decision
status: accepted
superseded_by:
tags: [privacy, telemetry, settings, notices, reqwest]
sources: []
created: 2026-11-22
updated: 2026-11-22
---

# ADR 0019: Anonymous usage statistics with opt-in

## Context
The project wants to know which agents, shells, settings and features are used, and how many
installations are active per day, to decide where work goes. The official website takes one
daily report per installation at `POST /api/telemetry/v1/daily` (schema 1, JSON, at most 8 KiB,
answer 204; 400, 413, 415 and 429 possible). Every text value of the schema is an allowlist value;
the server stores anything else as `other` and keeps no IP address. Its public page
`https://hangar.feoren.com/telemetry` lists every field. The people who use Hangar must not feel
watched or nagged: nothing may leave the machine before they agree, and they must be able to take
it back at any time.

## Options
1. **No statistics.** No data, no risk; decisions stay guesses.
2. **On by default with an opt-out.** More reports, but a report would leave before anyone agreed.
   Rejected: it contradicts the promise of the README and the wish not to annoy anyone.
3. **Off by default, asked once, sent by the backend.** One quiet question; reports only after a
   yes; the window never talks to the network itself.
4. **An installation id to count unique users.** Rejected: an id makes the reports of one machine
   linkable, which is exactly what the statistics must not allow.

## Decision
Option 3.

- **Consent** lives in the backend (`usage-statistics.json` in the local app data folder; debug
  builds use `usage-statistics.dev.json`, so a dev instance next to the installed Hangar never
  takes over or overwrites its answer and counters; `telemetry.rs`), not in `localStorage`, which the panel and file viewer windows share: no other
  window can turn it on. `null` means not asked. The main window asks once with a notice in the
  corner (`UsageStatsNotice.tsx`, the same card as the morning summary and the budget notice), 20 s
  after the start, with two answers that look alike and a link "What is sent" that opens the public
  page through the backend (`telemetry_open_info`, a fixed address). Existing installations are
  asked once after the update, new ones once after the first start. After either answer the
  question never comes back. The switch sits in Settings > General > Privacy with the same link.
  The notice is a polite live region (`role="status"`) like every card of the notice stack; the
  stack itself carries none, because some cards are alerts.
- **The answer on disk wins.** An answer is written at once. Every tick, and again right before a
  request, the backend reads the file: if its answer differs from the one in memory (given in
  another Hangar process), the file's state replaces the memory and nothing is written over it. A
  report leaves only when memory and file both say yes.
- **Writing:** counters change in memory only; the file is written when something changed, on the
  minute tick, at once for an answer, for an update about to be installed, and when Hangar quits
  (`stop` in the `Exit` handler). A crash loses at most the last minute's counts.
- **Nothing counts while it is off.** Turning it off drops the day's counters, a pending report and
  the settings snapshot; only `firstReportDone` stays, so a later yes is no second "new
  installation".
- **Days are UTC days.** Counters belong to the UTC day they were made on. On the first count or
  tick of a later day, the previous day's counters become the pending report if that day is
  yesterday and was not reported; older days are dropped (the server takes reports up to two days
  old, and an old day would say little). The worker thread looks 30 s after the start and then
  every 60 s, so a report also goes out when midnight UTC passes while Hangar runs. At most one
  report per date (`lastSent`). Only a failed connection, 429 and 503 get up to three tries 15
  minutes apart while the date is still yesterday: there the server surely kept nothing. A timeout
  after connecting, any other 5xx and every 4xx give the day up at once, because the server may
  already have stored the report and a second one would count the installation twice. Errors go to
  stderr with the date and the reason, never the report.
- **`firstReport: true`** exactly once per installation: on the first report that arrived.
- **What is counted where.** New sessions per program key and their mode in `create_session` (a
  restored session, `resume`, is no new start); session minutes by the worker thread, one minute
  per running agent session per tick (update sessions left out); `fileViewerOpened` when a viewer
  window is created, `notesCreated` after a note was created, `folderRevealed` after the file
  manager opened a session's folder (all three in the backend commands); `commitHistoryOpened`
  when the person picks the Commits view, `sidePanelDetached` on "Open in own window",
  `broadcastSent` after "Send to several", `pathMentioned` when a path from the Files tab goes into
  a session, `updateInstalled` when a Hangar update was downloaded and is about to be installed
  (the window downloads, waits until the backend stored the count, then starts the install,
  because the Windows installer ends the app; an install that fails after that point is still
  counted); these five through `telemetry_count`. That command takes the eight fixed names only,
  from the panel window only `commitHistoryOpened`, from the main window none of the three the
  backend counts itself (`window_may_count`).
- **Program keys** (`program_key`): a shell by its program file name (`pwsh`, `powershell`, `cmd`,
  `bash`, `zsh`, `fish`, `wsl`), an agent by its catalog id, also when a profile of the person's own
  starts the same program, a catalog agent the schema does not list as `other`, any other own
  program as `custom`. Never the command.
- **Settings** are a snapshot the main window sends (`telemetry_settings`) while the statistics are
  on: theme, terminal color scheme, font, font size, cursor, viewer theme, dot animation, attention,
  session restore, detached side panel, collapsed sidebar, and the UI language for
  `system.language`. The schema keeps the 0.6.0 palette names of the color schemes and viewer
  themes ([[0009-own-color-scheme-names]]); the backend maps each current key to its palette's
  old name. The font size is sent in steps only (up to 11: 10, 12 to 14: 13, 15 to 18: 16, from
  19: 20; outside 8 to 32 left out), so an unusual size does not make one installation
  recognizable among the others; every step is a valid size of the schema.
- **One list.** Every allowlist and mapping is in `telemetry_catalog.rs`; a vitest
  (`usageStats.test.ts`) fails when the app offers a scheme, font, cursor, animation, theme or
  language the list does not know. Values read back from the state file are checked again.
- **Machine:** `os` from the build target, `osRelease` from the Windows build number (22000 and up
  is 11), the macOS major version from `SystemVersion.plist` (13 to 30), `linux` on Linux (no
  distribution, no kernel); `arch` `x64` or `arm64`; `app.version` is `X.Y.Z` without the build,
  `channel` `beta` while the major version is 0, as the window shows it. `team.signedIn` and
  `team.sharedProjects` are read when the report is built; the number of shared projects is sent
  in steps (0, 1, 2 for 2 to 3, 5 for 4 to 10, 10 above), for the same reason as the font size.
- **Sending** only from the backend with the existing reqwest (10 s total, 5 s to connect,
  https only, no redirects, no cookie store, user agent `Hangar`), only to the official endpoint,
  never to a self-hosted team server. The window's CSP stays as it was. Debug builds
  (`cfg(debug_assertions)`) count and build reports but never send them.

## Consequences
- Active installations per day and new installations can be counted without any id; unique users
  across days cannot, on purpose.
- Every new agent of the catalog, color scheme, viewer theme or font needs a row in
  `telemetry_catalog.rs` and in the website's catalog, or it is reported as `other`.
- Minutes count only while Hangar runs and the machine is awake; a day with Hangar open but idle
  still sends a report, which is what "active installation" means.
- A day whose report could not be sent by the end of the next day is lost. Accepted: the
  statistics are a trend, not an invoice.

## Evidence
- Server contract: the field list at `https://hangar.feoren.com/telemetry` (schema 1, allowlists,
  counters 0 to 10000, `sharedProjects` 0 to 100, font size 8 to 32).
- Tests: `telemetry.rs` (day rollover, one report per day, retries, first report, consent off,
  schema 1 exactly, size below 8 KiB with every key at its cap, steps of font size and shared
  projects, which window counts what, the debug build's own file), `telemetry_service.rs` (no
  sending in debug builds or without consent, an answer of another process on the same file
  wins, the check right before the request, writing only on change and at once for an update,
  restart, damaged file, which errors retry), `manager.rs` (program keys), `UsageStatsNotice.test.tsx`, `GeneralSettings.test.tsx`, `usageStats.test.ts`.

> [!question] Unverified
> The macOS release read from `/System/Library/CoreServices/SystemVersion.plist` is not tested on
> a Mac yet; the parser is unit-tested with a sample file only. Without a value, `osRelease` is
> left out of the report.
