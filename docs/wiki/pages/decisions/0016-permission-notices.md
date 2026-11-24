---
title: "ADR 0016: Permission notices from Claude Code hooks"
type: decision
status: accepted
superseded_by:
tags: [claude-code, hooks, attention, links, queue, sessions]
sources: ["[[claude-notification-hooks-reference]]", "[[claude-permission-hooks-measurement]]", "[[claude-http-hook-measurement]]"]
created: 2026-11-20
updated: 2026-11-24
---

# ADR 0016: Permission notices from Claude Code hooks

## Context
Hangar presses Enter on its own in three places: a link group message ([[0014-session-links]]),
a queued message ([[queued-messages-and-stall-hint]]) and an agent command. An Enter into a
permission dialog answers it with the preselected choice. Until now Hangar knew of such a dialog
only from the gate's held tool calls (team sessions) and from reading the screen
([[agent-prompt-shapes]]), which cannot be sure; ADR 0014 names this as its remaining gap.
Claude Code reports dialogs through hooks: `PermissionRequest` at once, `Notification` with
`permission_prompt` or `elicitation_dialog` later ([[claude-notification-hooks-reference]]).
Private sessions had no hooks at all ([[0008-private-and-team-sessions]]).

## Options
1. **Keep the screen check alone.** No change for private sessions; the gap stays.
2. **Report-only http hooks for every Claude Code session, private ones included,** on the
   existing endpoint with a token of their own scope. Claude Code takes http hooks on all events
   needed; an empty 200 makes no decision and any failure is non-blocking.
3. **The gate for private sessions too.** Contradicts ADR 0008: a private session must never be
   held or decided for.
4. **A command hook through a Hangar helper.** Needed only where http hooks are not allowed; for
   these events they are, so it would add a process start per event for nothing.

## Decision
Option 2.

- **Hooks** (`gate::claude_notify_settings`, through `--settings` like the gate's,
  [[claude-http-hook-measurement]]): `PermissionRequest` (matcher `*`) and `Notification` with
  `permission_prompt|elicitation_dialog|elicitation_url_dialog` report a dialog to
  `/v1/notification/asks`; `Notification` with `elicitation_response|elicitation_complete`,
  `UserPromptSubmit`, `PostToolUse`, `PostToolUseFailure` (matcher `*`), `Stop` and `SessionEnd`
  report that it is gone to `/v1/notification/clear`. Timeout 5 seconds, no `onFailure`: a stopped
  Hangar holds a dialog up by 5 seconds at most and blocks nothing. No `PreToolUse`.
- **Who gets them:** every private Claude Code session whose installed version is known and at
  least 2.1.268 (`CLAUDE_NOTIFY_SINCE`: `PermissionRequest` fires in every mode from there,
  measured with 2.1.329), unless its profile passes `--settings` of its own. Team sessions get the
  same hooks next to the gate's in one `--settings`. The session's gate status does not change: a
  private session shows no gate.
- **Token scope `Notify`** in `HANGAR_NOTIFY_TOKEN`, apart from the gate's `Hooks` and the send
  client's `Send`: a notice token opens the two notice routes only, a hook or send token opens
  neither of them. The token is in the child's environment only and ends with the process.
- **Routes:** the route decides what happened; the body is read to its end but at most 64 KiB of
  it is kept and parsed, and only `hook_event_name` and `notification_type` are looked at (a
  `Notification` of another type, `idle_prompt` say, changes nothing). No text of a notification
  leaves the parser. The answer is always an empty 200.
- **Signal:** `permission_wait.rs` keeps the sessions with a dialog. Set on asks, cleared on clear,
  when the process ends, when the person presses a key that answers a dialog in that session
  (Enter or Esc only: `answersDialog`, command `permission_answered`; a digit is left out because it may be no choice of the dialog or text in a form, and a digit that does answer leaves the signal to the next hook or its expiry), and after 10 minutes
  without a new notice. The key is needed: after a "No" Claude Code sends no hook at all
  (measured, [[claude-permission-hooks-measurement]]). A notice that arrives after its process
  ended sets nothing (the token is checked under the gate's lock). The backend emits `permission-waits-changed`
  without payload; `permission_waiting` (main window only) returns the set.
- **Window:** a session in the set waits with the attention reason `permission` ("waits for your
  approval"), unless a gate decision is pending. `awaitsAnswer` and so `asksQuestion` are true for
  it without reading the screen; `autoReadiness` (`permissionPending`) never lets a link message
  go in, and the message queue treats the end of the phase as a question. The screen check stays
  for every other program and for Claude Code versions without the hooks.

## Consequences
- ADR 0008 changes in one point: private Claude Code sessions now carry report-only hooks. They
  still get no gate, no context and no decision.
- New glossary terms: notice hook, notice token, waits for approval.
- A parallel tool call that ends while another one's dialog is open clears the signal early; the
  screen check then stays the fallback. A stale signal only ever keeps a message as a card.
- Every tool call of a Claude Code session makes one local request on `PostToolUse`.
- On a busy machine Claude Code may connect for several hooks at once and write their heads more
  than a second later. Up to 0.15.0 the endpoint closed such silent connections for newer ones
  once 8 were open, which Claude Code reported as `Stop hook failed: read ECONNRESET` (and the same
  for `UserPromptSubmit`); seen repeatedly on another PC with 0.12.0 and 0.15.0. Since 0.16.0 up to
  64 may wait, a connection counts as silent after 5 s and gets 15 s for its head in total
  (`src-tauri/src/gate_service.rs`, test `hooks_of_a_briefly_stalled_client_are_not_closed_for_newcomers`).
  > [!question] Unverified
  > That this was the cause on that PC is a conclusion: the test reproduces the closing, the
  > other PC was not measured.

## Evidence
- [[claude-notification-hooks-reference]]: event types, hook types per event, http answers.
- [[claude-permission-hooks-measurement]]: `PermissionRequest` over http right after `PreToolUse`,
  `Stop` after a denial in print mode, no `PostToolUseFailure` or `PermissionDenied` then.
- Interactive measurement: `raw/2026-11-20-claude-permission-hooks-interactive-measurement.md`.
- Code: `src-tauri/src/gate.rs` (`claude_notify_settings`, `parse_notice`, `notice_counts`,
  `TokenScope::Notify`), `src-tauri/src/gate_service.rs` (`notify`, `notice`,
  `permission_waiting`), `src-tauri/src/permission_wait.rs`, `src-tauri/src/manager.rs`
  (`LaunchGate::notify` for private sessions), `src/permissionWaits.ts`,
  `src/usePermissionWaits.ts`, `src/links.ts` (`autoReadiness`), `src/agentCommands.ts`.
- Tests: `gate::tests::private_sessions_get_report_only_hooks_without_any_gate`,
  `gate::tests::team_sessions_get_the_notices_next_to_the_gate`,
  `gate::tests::a_notice_body_gives_its_event_and_type_only`,
  `gate_service::tests::a_notice_token_opens_the_notice_routes_only_and_decides_nothing`,
  `gate_service::tests::notices_set_and_clear_the_signal_and_tell_the_window`,
  `gate_service::tests::a_private_session_gets_report_only_hooks_and_no_gate`,
  `gate_service::tests::the_persons_answer_clears_the_signal_of_that_session_only`,
  `gate_service::tests::a_notice_that_arrives_after_its_session_ended_sets_nothing`,
  `permission_wait::tests::set_clear_and_expire`,
  `tests/pty.rs` `a_private_session_gets_the_notice_hooks_but_never_the_gate`,
  `tests/contract.rs` `permission_signal_carries_no_payload_and_its_command_checks_the_window`,
  `src/permissionWaits.test.ts`, `src/links.test.ts`.

Measured interactively (Claude Code 2.1.329 in a real ConPTY): `PermissionRequest` as the dialog
appears, the dialog stays after the empty answer, `permission_prompt` 6 s later, no
`idle_prompt` while it stands, and no hook at all after a "No" until the session ends.

> [!question] Unverified
> `elicitation_dialog` and its answer (an MCP server that asks) and a trust dialog at the first start
> in an untrusted folder were not measured in Hangar itself.
