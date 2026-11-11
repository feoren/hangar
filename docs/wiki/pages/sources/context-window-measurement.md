---
title: Context use and window size of a Claude Code session
type: source
tags: [agents, claude-code, tokens, context-window, sidebar]
sources: ["[[subagent-activity-measurement]]"]
raw: raw/2026-11-11-context-window-measurement.md
url: ""
retrieved: 2026-11-11
created: 2026-11-11
updated: 2026-11-11
---

# Context use and window size of a Claude Code session

Own measurement on Windows 11 with Claude Code 2.1.322 for the context meter in the session rows
(`src-tauri/src/context_usage.rs`, `src/ContextMeter.tsx`). Key names and numbers only.

## Key points (2.1.322)
- **Use:** main transcripts carry the same `message.usage` as sub-agent transcripts; input, cache
  and output tokens of the newest `assistant` record are the context size
  ([[subagent-activity-measurement]]). `<synthetic>` records have zero usage and are skipped.
- **Window:** stored nowhere Hangar can read. `message.model` is a plain id (`claude-opus-5-5`)
  without a `[1m]` suffix; settings hold an alias; only the status line command gets the window
  in its JSON.
- **Evidence for 1M:** contexts of 625,834 (`claude-opus-5-5`) and 579,989 (`claude-sonnet-5`)
  tokens were seen without a `context-1m` beta, so these models have more than 200K.
- **Table:** Hangar maps the model id to a window (1M for Fable, Mythos, Opus 4.6 and later,
  Sonnet 4.6 and later, Haiku 5.5; 200K otherwise, the default for unknown ids) after the model
  table bundled with Claude Code 2.1.322 (cached 2026-11-03). A context larger than the table's
  window counts as 1M (a 1M option the id does not show, or a newer model).

> [!question] Unverified
> Window sizes of models not seen in the transcripts (Fable, Mythos, Opus 4.6 to 4.8, Sonnet 4.6)
> come from the docs table only; a 1M option on a 200K model is only seen once more than 200K is
> in use.

## Codex CLI
Codex writes `token_count` events with `model_context_window` into its rollout files, but Hangar
cannot tell which rollout belongs to a session: no flag fixes the thread id at launch
([[subagent-transcripts]]). Matching by working folder and start time would mix up two sessions
in one folder, so the meter shows for Claude Code sessions only.

> [!question] Unverified
> A reliable link from a Hangar session to its Codex rollout (for example a future flag that sets
> the thread id).

## Significance for Hangar
- One thread polls every Claude session's main transcript every 2 s (only the end of the file on
  the first read, then what was appended) and emits a snapshot only when a number changed.
- After `/clear` Claude Code continues in a new transcript under a new id; the meter keeps the
  last numbers of the old one until the session restarts.
> [!question] Unverified
> That `/clear` starts a new transcript file in 2.1.322.
