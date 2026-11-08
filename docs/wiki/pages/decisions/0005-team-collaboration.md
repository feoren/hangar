---
title: "ADR 0005: Team collaboration of people and agents"
type: decision
status: proposed
superseded_by:
tags: [collaboration, teams, agents, server, e2e, git, worktree]
sources: []
created: 2026-11-08
updated: 2026-11-08
---

# ADR 0005: Team collaboration of people and agents

## Context
Several people, each with their own AI plan (Claude, Codex, Gemini …) and their own machine,
want to work on one project the way they already do with Git. With agents doing most of the
edits, four problems appear that Git alone does not solve:

1. **Same files:** two agents change the same files at the same time; the conflict shows up only
   at merge time.
2. **Duplicate work:** two agents solve the same task without knowing it.
3. **Contradicting decisions:** one agent changes the architecture, another builds on the old one.
   No textual conflict, but the result does not fit together.
4. **Missing context:** agent B does not know what agent A is doing or why.

The goal is a solution that is absolutely simple for the user. Requirements were collected in an
interview with the owner on 2026-11-08; the answers are recorded below.

## Requirements (owner's answers)
| Topic | Decision |
| --- | --- |
| Setup | Every person works on their own machine with their own Hangar and agents; code is exchanged through a shared Git repository. |
| Problems to solve | All four above. |
| Transport | All methods over time: Git, a sync service, direct peer to peer. |
| Scale | Open: from two people up to open source projects with many contributors. |
| Prevention | Task reservation, file and area claims, early conflict detection, automatic merging. All combined. |
| On overlap | The agent pauses, the human decides; both people get a message with the conflict and a proposal. |
| Precedence | Whoever reserved first wins. The second agent pauses; its person can request a release, the first person accepts or declines with one click. |
| Expiry | Reservations expire after inactivity (e.g. 30 minutes without changes) while the person is offline, with a warning first. |
| Visibility | Task and status, changed files (live, uncommitted), decisions with reasons, full transcripts. |
| Transcripts | Private by default, shared per session on purpose; detected secrets are redacted. |
| Participants | Hangar only (no open protocol for other front ends in version 1). |
| Identity | Hangar account with teams. |
| Service | An official service plus an open source server for self-hosting. |
| Data on the server | End-to-end encrypted: tasks, claims, status, decisions and shared transcripts are readable by the team only, not by the operator. Code stays in Git. |
| Claims | Observed automatically: Hangar watches which files an agent changes in its worktree and reports them at once. Agents can also announce areas in advance through a Hangar tool. |
| Platforms | Windows, macOS and Linux before collaboration ships, since teams are mixed. |
| Tasks from | A task list in Hangar, GitHub and other Git hosts' issues, other boards (Jira, Linear and similar), and derived from the first prompt. |
| Integration | Every task in its own worktree and branch. Finished tasks enter a merge queue: Hangar updates the branch, the agent resolves conflicts, tests run, the human confirms with one click; one at a time. |
| Shared memory | Every agent automatically gets decisions, running tasks and claimed areas before it starts; shared transcripts on demand. |
| Notifications | In Hangar and as a desktop notification, push to the phone, e-mail, chat (Slack, Teams, Discord). |
| Business model | Free for small teams, paid per person above that; self-hosting is always free. |
| Version 1 scope | Core first: account and team, task list with reservation, automatic file claims with warning and pause, live status of all agents, shared memory. Merge queue, issue import, transcripts, phone and chat follow. |

## Licensing and editions (owner's decision, 2026-11-08)
| Use | Account | Price | Where enforced |
| --- | --- | --- | --- |
| Alone, all local features | none | free | nothing to enforce |
| Team of up to 3 people | Hangar account | free | team server counts members |
| Team of 4 or more | Hangar account | paid package per seat | team server counts members |
| Self-hosted server, up to 3 people | none at Hangar | free | self-hosted server |
| Self-hosted server, 4 or more | none at Hangar | license key | self-hosted server checks a signed key |

- **Desktop app: MIT**, unchanged. Anyone may fork it, including removing account code; that is
  accepted. The app is never trusted to enforce limits and holds no service secrets.
- **Team server: separate repository under the Elastic License 2.0** (source available). Use,
  modification and self-hosting are allowed; offering it to third parties as a hosted or managed
  service and moving, changing, disabling or circumventing the license key functionality are not.
- **Enforcement on the server only:** joining a team is a server operation, so the seat limit holds
  for every client, including forks. End-to-end encryption does not conflict: the member list is
  metadata the server needs for key distribution anyway.
- **License keys for self-hosting:** an Ed25519-signed document (team id, seats, expiry) the server
  verifies offline with an embedded public key; issued by the website after purchase.
- **Brand:** "Hangar" and the logo are the protection for the official product; a trademark
  search and registration and a `TRADEMARKS.md` policy (forks must rename) follow.
- **Contributions:** CLA for the server repository (keeps relicensing and commercial licensing
  possible), DCO sign-off for the MIT app.
- **Residual risk:** someone can write their own compatible server from scratch; the protocol is
  not protected. The official service, the brand and the self-hosting license are the moat.
- Not legal advice: terms, license texts and trademark need review by an IT lawyer before sales.

## Options (to be decided after research)
1. **Coordination state in Git only** (files or a dedicated ref such as `refs/hangar/*`): no server,
   works offline, but not live and visible to everyone with repository access in plain text.
2. **Hangar service with end-to-end encryption** (chosen direction): live, works across Git hosters,
   accounts and teams; the server stores encrypted blobs and routes events.
3. **Peer to peer** (LAN, Tailscale): no server, but needs both peers online; later as an addition.

## Proposed decision
Option 2 as the backbone, with Git as the source of truth for code and as an offline fallback for
reservations, and peer to peer as a later transport. Version 1 scope as in the table.

## Open questions (research before building)
- **Pausing an agent:** which CLIs allow Hangar to block an edit before it happens (e.g. Claude Code
  hooks such as PreToolUse, MCP tools) and how Codex CLI, Gemini CLI and others can be paused.
  Without a hook Hangar can only warn and stop the process; the catalog needs a per-agent
  capability flag.
- **End-to-end encryption for groups:** key management when people join and leave (MLS, RFC 9420,
  versus simpler per-team keys with rotation), device keys, recovery.
- **Server stack and hosting:** Rust service, storage, push channels; cost per team for the free tier.
- **Change observation:** file watching per worktree (notify crate) with debounce, mapping hunks to
  areas, early conflict detection against other people's live changes.
- **Secret redaction** for shared transcripts: detection rules and false positives.
- **Shared memory format:** what is injected into each agent (instruction file, MCP resource) and
  how large it may get.
- **Project profile:** the current non-goal "remote multiplexing like tmux" stays (no remote
  control of sessions), but Hangar gains a network service; the project rules must say so.

## Consequences
- macOS and Linux become release blockers before collaboration ships.
- Hangar gains accounts, a server component and a security surface that needs its own threat
  model and review.
- The 0.3.0 worktree, diff, notification and status features are the local building blocks.
