---
title: Transcript fields for the session report
type: source
tags: [agents, claude-code, codex, transcripts, export, report]
sources: ["[[subagent-transcripts]]", "[[subagent-activity-measurement]]", "[[context-window-measurement]]"]
raw: raw/2026-11-14-session-report-transcripts-measurement.md
url: ""
retrieved: 2026-11-14
created: 2026-11-14
updated: 2026-11-14
---

# Transcript fields for the session report

Own measurement on Windows 11 (Claude Code 2.1.325 installed, transcripts written by 2.1.275 to
2.1.325; Codex CLI 0.169.0) for "Export as report" (`src-tauri/src/report/`). Key names and
counts only, no content.

## Key points: Claude Code (main transcript)
- Every `user` and `assistant` record carries `timestamp` (RFC 3339 UTC) and `gitBranch`: start,
  last activity, duration and branch of the report come from them.
- **Prompts:** `user` records whose `origin.kind` is `human` or which have no `origin`. Records
  with another `origin.kind` (`task-notification`, `peer`) or `isMeta: true` are not typed by the
  person and stay out.
- **Claude Code's own texts** start with a tag: `<command-name>` (a slash command; its
  `<command-args>` follow), `<bash-input>` (a `!` command), and the outputs
  `<local-command-stdout>`, `<bash-stdout>`, `<bash-stderr>`, `<local-command-caveat>`,
  `<task-notification>`. The report keeps the command and the `!` command as one line and drops
  the outputs.
- `isCompactSummary: true` marks the summary written after compacting; the report shows a marker
  instead of the summary.
- **Assistant:** one content block per record: `text`, `tool_use` (`id`, `name`, `input`),
  `thinking` (never shown).
- **Sub-agent runs** are `tool_use` blocks named `Agent` (input `description`, `subagent_type`,
  `run_in_background`). The `toolUseResult` of a finished run holds `agentId` and
  `totalTokens`; a background run (`status: "async_launched"`, by far the most common) holds
  `agentId` but no tokens, so the report takes them from that sub-agent's own transcript
  (context size of its newest response, [[subagent-activity-measurement]]).

> [!question] Unverified
> That a resumed conversation (`--resume <id>`) keeps writing into the same transcript file in
> every version; after `/clear` Claude Code may continue in a new transcript under a new id
> ([[context-window-measurement]]), and the report then covers only the newest one.

## Key points: Codex CLI 0.169.0
- No option sets the thread id at launch; `resume` and `fork` take an existing one.
- Two rollouts in `~/.codex/sessions/YYYY/MM/DD/` started in the same second: matching a session
  by folder and start time would mix up two sessions.
- `migrate-rollouts` moves "legacy local sessions to paginated thread history": the storage
  format is changing.

## Significance for Hangar
- The report is offered for Claude Code only (sessions with `subAgents`, the same condition as the
  sub-agent view); for Codex and every other program the entry is hidden, not disabled.
- Commits "made during the session" are the commits on HEAD whose author date lies between the
  first record and five minutes after the last one; a rebased or amended commit keeps its old
  author date and may fall outside.
