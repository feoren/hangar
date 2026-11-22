---
title: "Measurement: a Claude Code and a Codex session in one team project, live"
type: source
tags: [team, gate, hooks, codex, claude-code, relay, interop, measurement, context]
sources: []
raw: raw/2026-11-22-mixed-team-live-run.md
url:
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: a Claude Code and a Codex session in one team project, live

Own run on Windows 11 (build 26300): Claude Code 2.1.330 (`claude -p`, `haiku`) on Alice's device
and Codex CLI 0.171.0 (`codex exec` through `codex.cmd`, `gpt-6-luna`, effort low) on Bob's,
two real `TeamService`s against `hangar-server` 0.5.1 in self-host mode on the loopback, each
device with its own `GateService` wired as the app wires it. Test:
`live_agents_claude_and_codex_hold_each_others_files` in `src-tauri/tests/relay_interop.rs`
(ignored; variables in its documentation).

## Key points

- **Codex to Claude Code:** Codex's `apply_patch` on the file Alice's Claude Code session claims
  reached Bob's gate through the `-c` hook and `hangar gate-hook codex`, was held with the
  teammate named (`#u_jyQLRz… (Claude Code)`), and the deny (JSON, exit 0) stopped it:
  `hook: PreToolUse Blocked`, file unchanged, the model repeated the reason.
- **Claude Code to Codex:** Claude Code's `Edit` on the file Bob's Codex session claims was held
  by Alice's gate (`#u_Jl1CTD… (Codex)`) and denied; file unchanged, the model quoted the reason.
- **Context:** Claude Code's `UserPromptSubmit` context carried Bob's file and Bob's reservation
  (`{"task":"Write the guide","reservedBy":..,"area":"docs/guide"}`); the model copied both lines.
  Codex gets no context (`context_inject = false`, no context hook in its start); it learns of a
  teammate's file or reservation only from the deny.
- **Names in agent texts:** the short member id's ellipsis reaches both agents as `<U+2026>`
  (`#u_Jl1CTD<U+2026>`), in the deny reason and the context, and Claude Code repeated it.
  Fixed on 2026-11-22: `gate::literal` keeps U+2026 (unit tests
  `a_shortened_member_id_reads_the_same_*` in `src-tauri/src/gate.rs`, [[agent-gate-adapters]]).
- **Harness only:** inside `cargo test`, `std::process::Command` on `codex.cmd` did not find
  `node` while cargo's build folders lead `PATH`; the test removes them for the agents.
- `hangar-server` 0.5.1 serves no `/api/v1/projects/{id}/members`; the app logs that and keeps
  the log's members ([[hangar-account-api]]).

## Significance for Hangar

- The mixed team works end to end on the released relay: presence, claims and reservations travel
  through the relay, and each agent's own hook path holds the other's file in both directions.
  Reservation pauses are covered in both directions by the integration test (below), not live.
- The integration test `a_claude_code_and_a_codex_session_hold_each_others_files_and_reservations`
  in `src-tauri/tests/team.rs` covers files and reservations in both directions, the own
  reservation going ahead and the context, with recorded hook inputs, on every `cargo test`.
