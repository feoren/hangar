---
title: Costs per session and day, session handover
type: synthesis
tags: [costs, tokens, handover, claude-code]
sources: ["[[token-usage-measurement]]", "[[context-window-measurement]]"]
created: 2026-11-13
updated: 2026-11-15
---

# Costs per session and day, session handover

## Costs: data sources per agent

| Agent | Source | Shown |
| --- | --- | --- |
| Claude Code | `assistant` records of the main transcript and of `<session>/subagents/agent-*.jsonl`, usage per `message.id` ([[token-usage-measurement]]) | tokens, estimated cost |
| Codex CLI | `token_count` events in rollout files, but no reliable link from a Hangar session to its rollout ([[context-window-measurement]]) | nothing |
| Gemini CLI, OpenCode, Aider and the rest | no documented local usage log Hangar can tie to a session | nothing |

> [!question] Unverified
> Gemini CLI keeps chat files under `~/.gemini/tmp/` that may carry token counts; neither the
> format nor a session link is documented, so Hangar does not read them.

## Backend contract (`src-tauri/src/cost_service.rs`)

- One thread polls every [`POLL_INTERVAL`] (5 s) the sessions of `SessionManager::cost_targets`
  (the context meter's sessions plus title and program name). Each transcript file is read
  incrementally from its start (`Tail` with an unbounded first read, 4 MiB per poll); unchanged
  files cost one `stat`.
- `token_usage.rs`: `response(line)` takes input, output, cache writes (5 minute and 1 hour split,
  the uncovered rest counts as 5 minutes) and cache reads, the model, the time and fast mode.
  Synthetic and empty responses are skipped. A response id seen in two files counts once.
- Local day: `system_local_day` (Windows `SystemTimeToTzSpecificLocalTime`, Unix `localtime_r`), so
  daylight saving time is applied; tests use a fixed Europe/Berlin zone.
- Prices: `model_prices.toml` (built in, with source and date) plus the user's
  `model-prices.toml` next to profiles.toml (same format; same `id` replaces). An id matches
  exactly or with a date (`-20251101`) or option suffix (`[1m]`), never a longer model id. A
  response of an unknown model, or a fast response without `fast_factor`, has no price; its tokens
  count as `unpriced`. The user file is read with `take(256 KiB + 1)` and refused when it holds
  more, so a size checked before and a file that grows in between cannot meet.
- Ledger `cost-ledger.json` in the local data folder: per conversation id and local day the
  tokens, the cost of the priced part and the unpriced tokens. Replaced from the transcript on every
  change (never added), 31 days, at most 2000 conversations, written at most every 30 s and on exit.
  Read with `persist::read_regular_capped` (8 MiB, regular files only, never more than the limit
  plus one byte); a larger or foreign file starts an empty ledger.
- `cost-usage-changed` is a signal without payload (main window only, but any window may listen
  to an event, see [[process-launch-hardening]], and the report carries session titles); the main
  window then reads `cost_report`, and only the answer to the newest read counts.
- `cost_report` returns the `CostReport`: `today`, `sessions`
  (running sessions, whole conversation), `days` (today first, 7 entries, each with its sessions,
  most expensive first, `sessionId: null` once closed), `priceSource`, `priceDate`. An amount has
  `tokens`, `total`, `costUsd` (`null` when nothing has a price) and `partial`.
- Reuse (for example a comparison of sessions): `CostService::session(id)` in the backend,
  `api.costReport()` with `parseCostReport` and `report.sessions.get(id)` in the window
  (`useSessionCost` in `src/useCosts.ts`).

## Window

- Session peek: "Tokens 123k · ~$0.42" (a trailing "+" when part is unpriced; tokens only without
  any price), after the load and context lines the row builder leaves to the peek. Nothing for
  sessions without data. The row builder's "tokens" element stays the context window's tokens;
  costs have no row element.
- View menu "Costs …": today per session, the last 7 days as bars, the price source and date.
- Settings, Attention, Resources: daily budget in dollars. When today's estimate reaches it, a
  notice shows once per local day; during quiet hours it waits until they end.
- The budget (`dailyBudget`) and the day the notice last showed (`budgetWarnedDay`) live in the
  protected `main-prefs.json` (`MainPrefs` in `main_store.rs`, main window only), not in
  localStorage: the panel and file viewer windows share the origin and could otherwise set the
  budget out of reach or mark today as warned. The backend refuses a budget that is not finite,
  below one cent (`MIN_BUDGET_USD`, so the window never rounds a stored budget to zero) or above
  `MAX_BUDGET_USD` (100,000), and a day that is not strictly `YYYY-MM-DD` (month 01 to 12, day 01
  to 31); the window rounds to cents before it checks, so it never sends a zero. Files written before these fields existed load with both unset. An older value in
  localStorage (`dailyBudget` in `hangar.settings`, `hangar.costs.warnedDay`) is taken over once
  with the other preferences, only while the backend has none stored, and then removed.

## Handover

- Session menu "Hand over to …" for agent sessions (not shells, not updates), and only while
  another installed agent can take over; otherwise the entry is hidden. The dialog lists the
  other installed agents; "Request summary" (running and not working) pastes a summary request
  with Enter into the old session.
- Picking an agent starts it in the old session's folder. A team session keeps its worktree; the
  new session is a team session where its program can be one, private in the same worktree
  otherwise, so closing either never removes the worktree under the other.
- Hangar waits until the new agent turned on bracketed paste and its output paused for 1 s (at
  most 30 s), then pastes without Enter (`pasteIntoSession`, which asks first when the text would
  run lines). After the 30 s bracketed paste alone is enough (an agent that keeps drawing a
  spinner). Without bracketed paste the text is never pasted, because every line break would
  submit a line and the text carries foreign content (note excerpts, git file names): a notice
  says the agent was not ready and offers "Copy" (`waitUntilReady` answers `ready`, `gone` or
  `notReady`).
- The text (`src/handover.ts`): introduction with program and folder, the last request (at most
  1500 characters, quoted; the old terminal tracks the user's submitted lines in agent sessions,
  `SessionTerminal.lastRequest`, skipping slash commands and one-character answers, since the last
  prompt bar it used to read is gone), the changed files from `git_diff` by session id (at most
  40, then a count), the
  project notes (at most 5 titles with excerpts of 160 characters), and the instruction to
  summarize first. At most 8000 characters: excerpts go first, then file lines.

> [!question] Unverified
> The old agent's summary is not copied into the handover; it stays in the old session.
