---
title: "ADR 0008: Private and team sessions"
type: decision
status: accepted
superseded_by:
tags: [teams, sessions, worktree, gate, hooks, claims, ui]
sources: ["[[claude-http-hook-measurement]]"]
created: 2026-11-12
updated: 2026-11-12
---

# ADR 0008: Private and team sessions

## Context
Until 0.6.0 every program row under "New session" had a "+ Worktree" button (and Shift+click),
and three things were tied to other settings than the session itself:

- the own worktree was an option per start, independent of the team;
- the agent gate ([[team-service-v1]], step B) applied to every agent session whose program takes
  it, as long as the global setting was on;
- the team's presence and shared claims (step F part 2b) took every running agent session in a
  repository linked to a shared project.

The owner asked for one clear choice per session instead: work alone, or work with the team.
A follow-up asked that the choice not be bound to Claude Code: Codex CLI, Gemini CLI, Copilot CLI
and Qwen Code shall become team-capable once their hooks are verified, and a team with mixed
agents shall work.

## Options

1. **Keep the worktree button, add a "share with team" toggle per session.** Two independent
   switches give four combinations, two of which make no sense (a shared session in the project
   folder collides with the person's own work; a worktree without the gate gives no protection).
2. **One switch "Private | Team" below "Starts in", remembered per device.** Each mode fixes all
   three properties at once.
3. **Team per project, not per session.** Hides the decision; a quick private experiment in a
   shared project would be reported to the team.

## Decision
Option 2.

- **Private session:** starts directly in the folder, without the agent gate, and is never part
  of the team's presence, shared claims or the repositories offered for linking. Claims between
  the person's own sessions (step A) still cover it.
- **Team session:** always a git worktree of its own on its own branch (the former worktree
  flow, `create_worktree`), the agent gate on (where the global gate setting allows it), visible
  to the team when its repository is linked to a shared project. Without sign-in it still works
  locally: worktree, gate and local claims.
- **Where it lives:** `SessionMode` (`private`, `team`) on `SessionInfo`, set by
  `create_session(mode)` and kept by restart and resume fallback (`manager.rs`). The manager asks
  the gate only for team sessions. `team_share::team_labels` gives the team service labels for
  team sessions only, so `local_sessions` leaves private ones out of presence, `claim.set` and
  the repository choices. The window remembers the switch (`startMode.ts`, `localStorage`,
  default private).
- **Folder outside git:** Team is disabled with a tooltip, the switch shows Private.
- **Which programs:** data only. `ProfileView.teamCapable` is `Capabilities::gate()` from the
  catalog: `pre_edit_block`, `per_session_config` and a `hook_format` Hangar can write. Nothing
  in the backend or the window names a program. Today only Claude Code has a format
  (`hook_format = "claude_settings"`); the others show greyed out with "cannot be a team session
  yet". A program gets listed by adding its `HookFormat` variant (and the builder for its hook
  configuration in `gate_service.rs`) and flipping its catalog capabilities.
- **Mixed teams:** what a teammate sees does not depend on the agent. Presence and shared claims
  come from git: `claims_service::watched_sessions` takes every running agent session regardless
  of its program, the rescan reads git's changes, `team_share::local_sessions` builds the shared
  session from those claims and carries the program only as a display label. A teammate's Codex
  team session therefore shows up, and its files pause a Claude Code session's edit, just like a
  Claude Code session's would; only the gate on the Codex side waits for its hook format.
- **Older workspaces:** a session saved with a worktree is a team session after restore
  (`sessionMode` in `startMode.ts`); a private session never has a worktree.

## Consequences
- No worktree without the gate any more: the per-program "+ Worktree" button, Shift+click and the
  quick access entry "own worktree" are gone; quick access offers "program in folder, team
  session" for team-capable programs in git folders instead.
- The global gate setting stays as the master switch; its help text now names team sessions.
- Private sessions are invisible to the team, also in repositories linked to a shared project.
  A repository is offered for linking only once a team session runs in it.
- A shortcut or menu start of a program that cannot be a team session starts it private.
- A restored worktree session of a program without a hook format is a team session without the
  gate: visible to the team, not paused.

## Evidence
- `src-tauri/src/session.rs` (`SessionMode`), `src-tauri/src/manager.rs` (`spawn`, gate only for
  `SessionMode::Team`), `src-tauri/src/agent_catalog.rs` (`HookFormat`, `Capabilities::gate`,
  `ProfileView::team_capable`), `src-tauri/src/gate_service.rs` (`prepare` per format),
  `src-tauri/src/team_share.rs` (`team_labels`).
- Tests: `tests/pty.rs` `a_private_session_never_gets_the_gate_not_even_after_a_restart`,
  `team_share::tests::only_team_sessions_get_a_label_whatever_their_program`,
  `agent_catalog::tests::team_capable_comes_from_the_capabilities_alone`, `src/startMode.test.ts`.
- Claude Code's hook behavior: [[claude-http-hook-measurement]].
