---
title: Session resources and push to the phone
type: synthesis
tags: [sessions, resources, process-tree, notifications, push, keychain, contract]
sources: []
created: 2026-11-12
updated: 2026-11-12
---

# Session resources and push to the phone

Two small features of 0.6.0 that share one idea: the backend owns the system access (process
trees, keychain, network), the window decides when to ask and what to show.

## Resources per session

**What it shows.** The tooltip of a session row names `CPU 12 % · 340 MB` for the session's whole
process tree (the PTY child and everything it starts). A session that is an outlier gets a small
gauge mark in its title line, in the warning color; the tooltip and the screen reader text say why.
Setting `showResources` (Settings > Attention > Resources, default on); off, the window never asks.

**Contract.**

| Layer | Name | Shape |
| ----- | ---- | ----- |
| Command (main window only) | `session_resources` | `() -> SessionResources[]` |
| Rust | `resources::SessionResources` | `{ id, cpu: number \| null, memory: number, outlier: boolean }` (camelCase) |
| TS | `resources.ts` `parseResources` | the same, per session id; malformed answers are dropped whole |

- `cpu` is the share of the whole machine in percent (0 to 100, one decimal), like the task
  manager: CPU time used since the previous answer divided by wall time times logical processors
  (`std::thread::available_parallelism`). `null` on a session's first answer.
- `memory` is the resident memory (working set) in bytes of the processes alive now.
- `outlier`: the tree keeps at least `BUSY_CORES` (0.9) processors' worth of CPU time busy for
  `CPU_SUSTAIN` (30 s) without a break, or holds more than `MEMORY_LIMIT` (4 GiB). Core
  equivalents, not the displayed share: on 16 processors a runaway single-threaded loop is only
  6 % of the machine but a whole core for a long time. The stretch starts at the previous reading;
  after a pause (window hidden) the use is the average over the pause.
- An answer younger than `CACHE_FOR` (1 s) is repeated without reading again.

**Polling, not events.** `useSessionResources.ts` asks every `RESOURCE_POLL_MS` (2 s) and only
while the setting is on, `document.visibilityState` is `visible` and at least one row listens.
Chosen over a backend thread with events because the window knows all three conditions; a thread
would need them pushed in and would sample for nobody. Rows read the store through
`useSyncExternalStore`; unchanged numbers keep their object, so only changed rows re-render.

**Sampling** (`proctree::usages`, `SessionManager::tree_usages`). The trees are `Arc<ProcTree>`:
cloned under the session lock and asked after it is released, so a slow read never holds up the
sessions. A closed session still has its job ended by `Runtime::kill`; the last clone only closes
the handle.

- Windows: the session's Job Object. `QueryInformationJobObject(JobObjectBasicAccountingInformation)`
  gives `TotalUserTime + TotalKernelTime` (100 ns units, includes processes that already ended, so
  it never shrinks); `JobObjectBasicProcessIdList` gives the ids alive now, and the working sets
  come from `K32GetProcessMemoryInfo` with `PROCESS_QUERY_LIMITED_INFORMATION` (windows-sys feature
  `Win32_System_ProcessStatus`). A process whose id was listed but that is no longer in the job
  (`IsProcessInJob`, the id went to another process) is skipped. Checked by
  `tree_usage_of_a_running_session` in `tests/pty.rs`.
- Linux: one pass over `/proc/<pid>/stat` for all sessions, summed by field 6 (session id). The
  session and not the process group, because an interactive shell puts each job into a group of
  its own; portable-pty starts the child as session leader. CPU is `utime + stime + cutime +
  cstime` (a child's times move to its parent once waited for), memory `rss * page size`. The
  parser is tested on every platform; the `/proc` pass itself is not run in CI on Windows.
- macOS and other systems: no reading, the window shows nothing.

> [!question] Unverified
> The Linux path has not run on a Linux machine yet (written against proc(5)); macOS reports
> nothing until a `proc_pidinfo` based reader exists.

**No new dependency.** `sysinfo` would cover all platforms but is not in the tree and pulls a
large Windows API surface; the Job Object Hangar already holds per session answers both questions
on Windows with one more `windows-sys` feature.

## Push to the phone

**What it does.** Opt-in. Settings > Attention > To your phone: the service (Off, ntfy, Pushover),
its address or keys, the events (waiting, done, failed) and "Send test". Default off; nothing leaves
the machine before a service is saved.

**When a push goes out** (`phonePush.ts` `PushScheduler`, `App.tsx` attention listener):

- `pushEventFor`: attention while running is `waiting`, attention after an exit with code 0 is
  `done`, any other exit is `failed`; working and resting send nothing. Attention itself is only
  reported while the user does not look at the session (`attention.ts`).
- The state must last `PUSH_DELAY_MS` (60 s); another state before that replaces it, the same
  state again changes nothing. One stretch of a state is one push at most. A closed session forgets
  its pending push. Placeholders and update sessions never push.
- Quiet hours (`quietHours.ts` `isQuietTime`) and attention off hold every push back; nothing is
  sent later.
- The backend adds its own limits, counted on the attempt so failures do not retry: at most one
  push per session every `MIN_GAP` (30 s), at most `MAX_PER_MINUTE` (10) pushes in any minute in
  all (tests included), and at most one test every `TEST_GAP` (5 s).
- Waiting for a decision at the agent gate does not push (it is not a state of the tracker).

**Contract.** All commands main window only (`tests/contract.rs`).

| Command | Arguments | Answer |
| ------- | --------- | ------ |
| `push_settings` | none | `PushView` |
| `push_save` | `input: PushInput` | `PushView` |
| `push_test` | `text: string` | nothing |
| `push_session` | `id`, `event: "waiting" \| "done" \| "failed"`, `template: string` | `boolean` (false: none due) |

- `PushView`: `{ provider: "off" \| "ntfy" \| "pushover", ntfyUrl, includeTitle, events: {
  waiting, done, failed }, ntfyToken: boolean, pushoverUser: boolean, pushoverToken: boolean }`;
  the last three say whether a secret is stored, never its value.
- `PushInput`: the same settings plus `ntfyToken`, `pushoverUser`, `pushoverToken` each `null`
  (keep), `""` (remove) or the new value. A chosen provider must be complete (ntfy address, or both
  Pushover keys); everything is checked before anything is written. `PushInput` has no derived
  `Debug` (its own names a secret only as `<redacted>`) and wipes its secrets when dropped.
- The ntfy token is bound to the origin (`scheme://host:port`) it was entered for
  (`ntfyTokenOrigin` in `push.json`). Keeping it while the address moves to another origin is
  refused (`tokenOrigin`), and a send refuses it too if the file was changed by hand; the form
  asks for the token again or its removal (`needsTokenAgain`).
- Without a token the topic must be at least `MIN_OPEN_TOPIC_CHARS` (16) long (`weakTopic`): an
  open topic is readable by anyone who guesses it. The form offers a random topic of 128 bits as
  base64url (22 characters, `randomTopic`).
- Errors reach the window as a kind string (`PushError::kind`): `invalidUrl`, `invalidToken`,
  `invalidKey`, `incomplete`, `keychain`, `storage`, `network`, `unauthorized` (401, 403),
  `rejected`, `task`, `tokenOrigin`, `weakTopic`, `invalidTemplate`, `unknownSession`,
  `rateLimited`. Network error texts are dropped, they may name the topic.
- The message is built in the backend. The window sends the translated sentence with one `{name}`
  (`push.waiting`, `push.done`, `push.failed` in `i18n.ts`, at most 160 characters, no control
  characters); the backend fills in the program and its running number among the sessions of that
  program, "Claude Code (3)" (`push::session_names`), and refuses an unknown session id. The
  session's name, which may come from the first prompt, goes out only with the opt-in
  `includeTitle` ("Send session names"). Never terminal content; control characters are stripped
  and the text is cut at 200 characters.

**Storage.** `push.json` in the config folder holds provider, ntfy address and events. The ntfy
access token and both Pushover keys live only in the OS keychain, service `hangar-push`, accounts
`ntfy-token`, `pushover-user`, `pushover-token` (the existing `secrets::KeyringStore`), apart from
the profile secrets so no profile variable shares an entry.

**Sending** (`push::deliver`): one POST, reqwest with rustls (already in the tree), `https_only`,
no redirects (a redirect could carry the token elsewhere), 10 s total time limit, no retries.

- ntfy: `POST <address>`, body the message (`text/plain; charset=utf-8`), header `Title: Hangar`,
  `Authorization: Bearer <token>` when a token is stored. The server's cache stays on on purpose:
  `Cache: no` would deliver only to subscribers connected at that moment, and `since=` or `poll=1`
  would not return the message later (ntfy docs, "Publishing", sections "Message caching" and
  "Message title", https://docs.ntfy.sh/publish/, retrieved 2026-11-12), so a phone that polls (the
  iOS app) could miss every push. The text names only program and number unless the user opts in.
- Pushover: `POST https://api.pushover.net/1/messages.json`, form `token`, `user`, `title`,
  `message`.
- The ntfy address: https, a host, no user info, query or fragment, a topic of `[-_A-Za-z0-9]`
  (1 to 64, as ntfy's "Picking a topic" states) as the last path segment (a server may sit under a
  path prefix). The same check runs in the form (`isNtfyUrl`).

> [!question] Unverified
> The Pushover key format (letters and digits) follows Pushover's documentation as remembered, not
> a fetched page. No real push was sent while building this. Tests run against a local plain-HTTP server with a
> test-only client.
