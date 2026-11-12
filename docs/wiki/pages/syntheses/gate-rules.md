---
title: "Gate rules and why an agent is not team-capable"
type: synthesis
tags: [gate, rules, team, catalog, ui]
sources: []
created: 2026-11-12
updated: 2026-11-12
---

# Gate rules and why an agent is not team-capable

Two additions to the agent gate of [[team-service-v1]] step B and the team sessions of
[[0008-private-and-team-sessions]]. Both are Hangar's own design; nothing here depends on an
agent CLI's behavior beyond what [[agent-gate-adapters]] already records.

## Gate rules (remembered answers)

When the gate holds an edit that collides with another session or a teammate, the card now has
"Remember for this project": "always allow" or "never allow", for the file or for its folder. The
answer goes to the waiting edit at once and becomes a gate rule.

- **Code.** `src-tauri/src/gate_rules.rs` (pure: path rules, matching, rule book, tolerant
  parsing), `gate_service.rs` (`decide` applies rules, `answer` saves them, `load_rules`,
  `delete_rule`), `gate.rs` (`Answer::AlwaysFile` and siblings, `Gate::settle`, `Gate::record`, the
  snapshot's `log`). Window: `GateCard.tsx`, `GateRulesSettings.tsx`, `useRuleNotes.ts`, `gate.ts`.
- **Project.** A rule belongs to the repository's git common folder (`WorktreeClaims::repo`), so
  every worktree of one repository shares it, and a team session's fresh worktree is covered.
  The path is relative to the worktree's top, as git prints it.
- **Never beyond the project.** Rule paths are normalized and refused when empty, absolute, with a
  drive or device prefix, with `..`, control characters or over 1024 bytes. A folder rule covers
  files strictly below the folder (`src/auth` covers `src/auth/x.ts`, not `src/authority.ts`). A
  file at the top of the project gets no folder option: its folder would be the whole project.
- **Case.** Paths and project keys compare without case on Windows and macOS, with case on Linux,
  as `claims::file_key` does.
- **Precedence.** Any matching deny rule wins over any allow rule; within one effect the most
  specific rule is named (file before folder, deeper folder first). A deny rule also wins over the
  session-only "Allow for this file".
- **Where it is decided.** In the backend, in `decide`, after the overlap is found and before a
  card is opened: the window is never asked. A new rule also settles the other waiting decisions it
  covers. Rules only answer asks; they never pause an edit the gate would let through.
- **Storage.** `<app local data>/gate-rules.json`, `{"version":1,"rules":[...]}`, written with
  `persist::write_atomic`. Parsing is tolerant: unknown fields are ignored, entries that do not
  parse, repeat an id or name an unsafe path are skipped, a broken or oversized file (over 4 MiB,
  room for 500 rules at the longest paths) is an empty book; at most 500 rules. The same project, path and scope again changes the effect
  instead of adding a rule.
- **Visible.** Every automatic answer goes into a log of the last 50 (in memory, per app run),
  sent with the gate snapshot. The session's pane shows a one-line note for 6 s ("Remembered
  answer: src/x.ts allowed (src/)"); Settings > General lists the rules (effect, path, project,
  delete) and the last 10 automatic answers with time and session, under the gate setting. The
  block is hidden while there is no rule and no automatic answer.
- **Agent's reason.** A deny by rule tells the agent that the user's standing rule for the project
  does not allow the edit while another session changes the file (`gate::rule_denied_reason`).

## Why an agent is not team-capable

`Capabilities::team_block` names the first reason a program cannot be a team session;
`team_ready` is `team_block(..).is_none()`, so the offer and the backend's refusal are unchanged.
`ProfileView.teamBlock` carries it to the window (serde tag `kind`, camelCase):

| `kind` | When | Shown under Team |
| --- | --- | --- |
| `noHooks` | no hook format, or no pre-edit block or per-session config | no |
| `notMeasured` | recipe built, not run live yet | no |
| `adminSettings` | an administrator's settings file the session's would replace | yes |
| `unknownVersion` | the version query gave nothing or nothing parsable | yes |
| `belowMinVersion {min, installed}` | installed below the catalog's `min_version` | yes |
| `fixedFolder` | otherwise ready, but the profile fixes a working folder (manager) | yes |

Administrator settings are checked before the version: updating would not help. The Team list of
"New session" still lists only what works (owner rule); below it one collapsed line "N agents not
available" (`TeamUnavailable.tsx`, grouping in `startMode.ts`, `unavailableForTeam`) lists the
installed agents with a fixable reason and a short hint each. Agents without a recipe or
measurement are not listed: there is nothing the person can do about them. An older backend that
sends no `teamBlock` falls back to `teamCapable` and the profile's folder.

## Tests

- Rust: `gate_rules::tests` (file and folder matching, sibling prefixes, other projects, case per
  platform, precedence, path safety, round trip and tolerant parsing),
  `gate::tests::rule_answers_resolve_like_their_effect_and_waiting_decisions_can_be_settled`,
  `gate::tests::the_rule_log_keeps_the_newest_answers_with_visible_names`,
  `gate_service::tests::gate_rules_answer_before_the_person_is_asked`,
  `gate_service::tests::never_for_this_file_denies_now_and_later`,
  `agent_catalog::tests::team_block_names_the_first_reason_the_program_cannot_be_a_team_session`,
  `manager::tests::a_fixed_folder_is_shown_only_when_nothing_else_stands_in_the_way`.
- Vitest: `gate.test.ts` ("remembered answers"), `startMode.test.ts` ("agents not available for a
  team session").
