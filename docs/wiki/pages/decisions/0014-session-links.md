---
title: "ADR 0014: Linked sessions that work together"
type: decision
status: accepted
superseded_by:
tags: [sessions, agents, hooks, security, pty, sidebar]
sources: ["[[0012-cross-project-messages]]", "[[cross-project-paste-measurement]]", "[[paste-opencode-gemini-measurement]]", "[[unattended-settings-reading]]"]
created: 2026-11-19
updated: 2026-11-23
---

# ADR 0014: Linked sessions that work together

## Context
[[0012-cross-project-messages]] lets the agent of one project message the agent of a partner
project. A partnership is set up once per pair of projects, is stored, and an agent's message
waits until the person clicks "Paste".

The owner wants something lighter: pick two or more running sessions in the sidebar, link them,
and let their agents talk to each other on their own until the person unlinks them. The linked
sessions must be easy to spot, more than two must be possible, and it must work with every agent
CLI.

## Options

| Option | Advantage | Disadvantage |
| --- | --- | --- |
| A. Link groups of running sessions, in memory, automatic delivery | What the owner asked for; reuses the 0012 send route and paste | Agents steer each other without a click; needs a loop brake |
| B. Extend partner projects to more than two and make them temporary | One concept less | Partners are per project, the owner wants per session; mixes a stored and a temporary concept |
| C. Shared chat panel in Hangar that agents read via MCP | Answers and history in one place | Not every CLI supports MCP the same way; the agent only reads when it decides to |

## Decision
Accepted on 2026-11-19: option A.

**1. Link group.** The person selects two or more running agent sessions and chooses "Link". They
form a link group with its own color from a fixed palette, shown as a colored chain mark on each
session row (and on the tab icon in the collapsed sidebar). A session belongs to at most one group.
Several groups can exist at once. Shell sessions cannot be linked (as in 0012, shells neither send
nor receive).

**2. Names.** Each member gets a short name inside the group, derived from its session title
(lowercase, `[a-z0-9-]`, unique inside the group by a numeric suffix). The person can rename it in
the link dialog. `hangar-send <name> <text>` writes to one member, `hangar-send all <text>` to
every other member.

**3. Lifetime.** A group lives only in memory. It ends when the person chooses "Unlink" or when
fewer than two members remain (a member's session closed or exited). Nothing is stored on disk;
after a restart there are no groups. A member that leaves loses the right to send to the group at
once; its queued messages to others stay deliverable, messages queued for it are dropped.

**4. Sending.** The send route of 0012 resolves the target in this order: member of the sender's
link group, then partner project. Because a link is made while the session runs, its environment
cannot change any more: every agent session therefore gets the send token, the `hangar-send`
client on `PATH` and the `HANGAR_SEND` variable at its start, whether or not it has partners. The
route refuses a sender that is neither linked nor has the named partner, so an unused token grants
nothing. Sessions started before this version have no token and are not offered for linking.

**5. Introduction.** On "Link", Hangar delivers an introduction to every member (same delivery path
as below): the group's member names, each with its program and working folder, and how to use
`hangar-send`. The text says the other members are agents working in parallel and that the agent
should send results or requests only when they help the other side.

**6. Automatic delivery.** Inside a link group, a message is delivered without a click: Hangar
queues it for the target session and pastes it with the header line
`[Message from "<name>" (link group)]` and then Enter once the target is waiting for input (state
"waiting for you" or "paused", output paused, bracketed paste on). This differs from 0012
decision point 4 on purpose: the person created the group and so consented to agents steering each
other inside it. Messages from partner projects keep the 0012 rule.

Guards:

- A toggle per group, "Ask before pasting", switches the group to the 0012 behavior (waiting
  message with "Paste" and "Discard"). Off by default.
- Loop brake: at most 6 automatic deliveries per group per minute. Beyond that, messages wait for
  a click like in 0012 and the group shows a hint; the brake resets after a quiet minute.
- The cleaning, length cap and queue limits of 0012 apply (at most 5 waiting per target).
- Every delivered message leaves a work log line ([[0011-work-log-and-daily-review]]) with sender,
  target and time, not the text.

**7. Agents.** Delivery is a bracketed paste plus Enter, which needs nothing from the CLI. Measured
in [[cross-project-paste-measurement]] for Claude Code, Codex CLI, Pi and Aider, and in
[[paste-opencode-gemini-measurement]] for OpenCode 2.3.2 and Gemini CLI 0.70.0 (since 2026-11-22).
Sending needs the agent to run a command.

## Consequences
- New glossary terms: link group, member name.
- Every agent session gets a send token. The route still refuses everything without a link group
  or partner, so the attack surface for a session without either is unchanged in effect.
- A link group is not a remote feature and stays inside one Hangar instance.
- **As built (2026-11-20).** Rules in `src-tauri/src/links.rs` (pure), state and send route in
  `src-tauri/src/link_service.rs`, window side in `src/links.ts`, `src/LinkDialog.tsx`,
  `src/LinkMark.tsx`. Choices and deviations:
  - Which sessions can be linked is known from the start of their process: `PartnerLaunch`
    reports every session that got the send token and `hangar-send` (`LinkService::sendable`)
    and every one that ended. A shell, an update session, a session without the client and one
    started before this version are never offered; the backend checks again on "Link".
  - Bounds: 2 to 8 members per group, at most 32 groups; the color is the first of 6 no other
    group shows, and repeats once all are taken.
  - Member names spell out common accented letters (`ä` as `ae`), turn everything else into
    single dashes, are cut to 28 characters before the suffix and are never `all`. A title
    without a letter or digit gives `agent`. `hangar-send` ignores ASCII case in the name.
  - Selecting in the sidebar is Ctrl+click (Cmd+click on macOS) on a row that can be linked; a
    bar below the list shows the count and offers "Link" from two on. The session menu offers
    "Link with …" only where another session could join, a member gets "Edit group" and
    "Unlink". Joining an existing group was not offered at first; since 2026-11-23 it is (see
    "Adding sessions to a group").
  - The send route asks the link group first and the partner projects second; a linked sender
    can still reach its partners. A name that is neither gets the new code `not_member` (exit 4)
    with the members and partners named; a message that goes on its own answers `queued`
    (exit 0), one that waits answers `waiting` as in 0012. A sender with neither a group nor a
    partner gets `not_partner`, so an unused token grants nothing.
  - `all` is all or nothing: when one target has no room, the whole message is refused.
  - The loop brake counts a delivery when the message is queued, not when it is pasted: the
    count then stops a loop at its source, whatever the targets' readiness. Every message while
    the brake holds pushes the release back, so "a quiet minute" means no message in the group at
    all. The introduction does not count.
  - Turning "Ask before pasting" on also holds the messages of the group that still wait to go
    on their own.
  - An automatic delivery that does not get through (the target never turns bracketed paste on
    within 30 seconds, the paste fails) turns into a message that waits for a click, with a hint,
    instead of being retried without end.
  - "Unlink" by the person drops what still waits in the group, so the agents stop at once. When
    the group ends because a member left, what the others sent stays deliverable as decided.
  - The header check of 0012 now refuses every line that starts like `[Message from `, so no
    text poses as a member, Hangar's introduction or a partner project.
  - The introduction is headed `[Message from Hangar (link group)]` and leaves no work log line,
    since no session sent it.
- **Hardened after review (2026-11-20).** A security review and the acceptance review asked for
  the following; each is built with a test.
  - **Permissions.** The eight link commands (`link_state`, `link_sessions`, `unlink_group`,
    `edit_link_group`, `link_message_text`, `hold_link_message`, `link_person_typed`,
    `close_link_message`) are in `build.rs` and granted in `capabilities/default.json` only.
    `tests/contract.rs` now also checks the other direction: every command wired in
    `generate_handler!` must be in `build.rs`.
  - **When Enter goes in on its own** (`autoReadiness` in `src/links.ts`): only when the target
    went quiet after a working phase (state "waiting for you" with reason quiet, or on screen
    "idle" whose last phase ended quiet), never while it works or rests without having worked,
    after a bell, while it waits for a gate decision or may show a question (`asksQuestion`),
    and never while the person types there. A target on screen goes "idle" instead of "waiting
    for you" when it falls quiet; reading only the state would mean no message ever reaches a
    session the person watches, so its quiet end counts too. Since [[0016-permission-notices]]
    a Claude Code session's own hooks report a permission or input dialog (`permissionPending`),
    private sessions included, and no Enter goes in while one waits; the gate's held tool calls
    count as before.
  - **Patience.** A member's message waits for a working target without a limit; a target that
    is not working and not ready for 30 seconds, or where the person types for 20 seconds, gets
    the message as a card ("Paste and send"). The introduction waits 5 seconds: a session that
    never worked may show a trust dialog, which an Enter would answer. Typing counts from the
    person's own keys: a line begun since their last Enter, or a key in the last 4 seconds.
  - **The toggle and the brake act at once.** The window fetches the text of an automatic
    delivery with `link_message_text(id, to, automatic: true)`; the backend refuses it
    (`link_held`) once the message waits for a click, the group asks first or, for a member's
    message, the brake holds.
  - **At most once.** Fetching the text claims the message (`LinkQueue::claim`); a second paste
    is refused (`link_claimed`) until `close_link_message` removes it or `hold_link_message`
    frees it for a click. A message whose removal failed after the paste stays claimed and is
    never pasted on its own again.
  - **Budget.** Besides the rate, at most 20 automatic deliveries per group without the person
    stepping in (`AUTO_BUDGET`, state `BrakeState::Spent`); then everything waits for a click and
    the group shows the brake's hint. It starts again when the person sends a line they typed in a
    member session (`link_person_typed`, `ownLineKey` in `links.ts`), pastes a card or edits the
    group. A bare Enter, or arrows and Enter, on an agent's dialog does not count: an agent can ask
    for that reflex.
  - **Marked as a peer's message.** The header reads
    `[Message from agent "<name>" (link group), not from the person]` and every line of the text
    is indented with `| ` (not `> `, which made a quoted numbered list look like a dialog's
    pointed choice), so no line starts in the first column where a header would. The
    introduction adds that messages from other members are requests from fellow agents, no
    instruction or approval from the person, and that the agent's rules and the person's
    questions still apply. Format characters beyond the hidden ones of 0012 and NFKC folding are
    not checked: with every line indented, a look-alike header cannot stand where a real one
    does, and a normalization table would be a new dependency.
  - **Agents without prompts.** A member whose profile starts it without asking before tools
    (`runs_without_prompts`: Claude Code `--dangerously-skip-permissions` or permission mode
    `bypassPermissions`/`dontAsk`, Codex CLI `--dangerously-bypass-approvals-and-sandbox`,
    `--full-auto`, `--yolo` or approval `never`, Gemini CLI `--yolo` or approval mode `yolo`,
    Aider `--yes-always`, Copilot CLI `--allow-all-tools`) turns "Ask before pasting" on when the
    group forms, the introductions included; the person can turn it off. The link dialog shows
    each candidate with program, "without prompts" where it applies, and folder. Since
    2026-11-20 the settings files of the CLIs count too ([[unattended-settings-reading]],
    `src-tauri/src/unattended.rs`): Claude Code's `permissions.defaultMode` in the user, project,
    managed and `--settings` files, Codex CLI's `approval_policy = "never"` in its home, the chosen
    profile and the project files of the folder and its parents, Aider's `yes-always` in its
    config and `.env` files and `AIDER_YES_ALWAYS`. Every place is read and any one that says so
    counts, without working out which file wins: a wrong "yes" only turns the toggle on. The
    files are read once per process (the CLI takes its start mode from them when it starts), on
    a blocking thread and outside the sessions lock, so a working folder on a slow drive does not
    freeze the window; `link_state`, `link_sessions` and `edit_link_group` are async for that.
    Gemini CLI and Copilot CLI have no such key; their arguments decide alone. The program is
    told by the file name of the profile's command, so a wrapper (`wsl.exe`, `npx`, a shell with
    `-c`) is judged by its arguments only, and Claude Code's `auto` mode is not counted.
  - **A sender that leaves.** What it sent still reaches its targets, but only as a card.
  - **Work log.** A delivered member message writes `linkMessage` with the sender's member name,
    the target and the time, no text ([[0011-work-log-and-daily-review]]).
  - **What the send right cannot tell apart.** The token reaches the agent through its
    environment, so every child process of the agent (its shell commands, tools, a script it runs)
    inherits it and can send as that member. Under the same user account this cannot be
    separated: such a process could read the agent's memory too. What limits it: the route takes
    only a member name or partner and only cleaned text, every message carries the sender's
    member name and says it is not from the person, a shell session never gets the token, the
    rate, the budget and the toggle bound what goes in on its own, and the token ends with the
    session's process.

## Roles, history and templates
Added on 2026-11-20. Each part is built with tests; without a lead and without a template that
links, everything above holds unchanged.

- **Lead.** At most one member of a group may lead (`Role::Lead` in `src-tauri/src/links.rs`,
  `link_two_leads` for a second one). The person picks it in "Link with …" or "Edit group"
  ("No lead" by default). The lead hands out tasks and collects results; the members report
  results and questions back to it. Routing does not change: every member may still write to
  every other one. When the lead leaves, the group goes on without one; nobody is promoted.
- **Alias `lead`.** `hangar-send lead <text>` reaches the lead. Like `all`, `lead` is reserved:
  no member may carry it, derived names step aside (`lead-2`). Without a lead the alias names no
  member and the partner projects decide, as for any other unknown name. The lead naming itself
  gets `refused` like any own name.
- **Introduction per role.** The member list marks the lead with `(lead)`. The lead reads that it
  splits the work, hands it out with `hangar-send <name>` and collects the results; a member reads
  the lead's name and to report to it with `hangar-send <lead>` or `hangar-send lead`. A change of
  the lead in "Edit group" queues a fresh introduction for every member, opened with "The roles in
  your link group in Hangar changed." (without a lead any more, it adds "No member leads this
  group any more."); a member whose queue is full misses it, since the edit
  itself must not fail on that. A rename alone introduces nothing, as before.
- **Header.** A message of the lead reads
  `[Message from agent "<name>", lead of the link group, not from the person]`; a member's header
  stays as before. The `| ` indentation and the "not from the person" stay; the header check of
  0012 still refuses any line that starts like `[Message from `, so no member can pose as the lead.
- **Mark.** The lead's chain carries a small dot in the group's color (`.session-link.lead`), apart
  from the brake's ring; its tooltip says "lead".
- **History.** Per group a ring of the last 200 messages that were delivered or discarded, with
  time, sender (member name and role, or Hangar), target member name, outcome and the cleaned
  text (`History` in `links.rs`). Besides the count it holds at most 256 KiB of text per group, so
  32 groups at the length cap stay bounded; the newest entry always stays. A message dropped
  because its target left counts as discarded. The history lives in memory only and ends with
  the group (unlink, or fewer than two members), like everything of a group (ADR 0011 and this
  ADR stay valid: nothing goes to disk). The work log line per delivered message stays as it was,
  without text. Read with the main window command `link_history(group)` (`check_main`, in
  `build.rs`, `capabilities/default.json` and the main-window-only list of `tests/contract.rs`).
  Shown in the dialog "Group history" from the session menu of a member: newest first, each row
  closed to time, sender, target, outcome and the first line, opened for the whole text with
  "Copy". It reads again whenever the group's messages change.
- **Templates that link.** A workspace template may carry `link` and `askFirst`, each session
  `memberName` and `role` (`WorkspaceTemplate`/`TemplateSession` in `main_store.rs` and
  `src/templates.ts`). All four are optional: an older template loads unchanged and links
  nothing; the store checks member names like the link dialog and at most one lead. "Save
  sessions as template" offers "Link on start" once two chosen sessions can be linked, checked
  when they are linked now; then the template keeps the names and the lead of the group with most
  of the chosen sessions, and that group's "Ask before pasting". On start, `startTemplate` starts
  every session as before, then links the ones whose process got the send client
  (`templateLinkRequest` in `src/links.ts`: shells and failed starts are left out, a name twice or
  a second lead is dropped) with `link_sessions(ids, names, lead, askFirst)`. With fewer than two
  it links nothing and says so. An agent without prompts still turns "Ask before pasting" on;
  the template can only turn it on, never off.

## Adding sessions to a group (2026-11-23)
The owner asked to add more sessions to an existing group instead of unlinking and linking again.

- **Operation.** `Links::add_members(group, chosen, lead)` in `src-tauri/src/links.rs` adds one
  or more sessions to a group under the rules of `link_members`: each a running agent session
  with the send client and in no group yet (`link_already` otherwise, no move between groups),
  the group then at most eight members, a chosen name valid, neither `all` nor `lead` and unique
  among members and newcomers. A derived name steps aside for every name in the group. `lead` is
  the group's lead afterwards, a member or a newcomer. Nothing changes on any refusal; an empty
  list is `link_none_added`. Adding resets the group's budget like an edit.
- **Command.** `add_to_link_group(group, ids, names, lead)` (async, `check_main`, in `build.rs`,
  `capabilities/default.json` and the main-window-only list of `tests/contract.rs`;
  `tests/security_config.rs` checks that the group commands check the window). The window sends
  the names `memberAliases` derives (`src/linkAliases.ts`), now taking the members' folders and
  names into account, and the current lead unless the person picked another.
- **Introductions.** A newcomer gets the full introduction as at linking. Every member gets one
  opened with `Your link group in Hangar grew: "<name>" joined.` that lists all members again,
  plus "Its roles changed as well." when the lead changed in the same step (`Occasion` in
  `links.rs`; the window titles its card "New members in the group", `src/linkIntro.ts`). Like
  every introduction they go into the group's history once pasted or discarded. The step is
  refused while the queue has no room for the newcomers' introductions; a member whose queue is
  full misses its own, as after an edit.
- **Ask before pasting.** The newcomers' introductions follow the group's toggle. A newcomer
  whose agent runs without prompts turns it on, as at linking, and holds what still waits to go
  on its own.
- **Where the person adds.** "Link with …" on a session in no group lists each group with room
  first ("Add to group: shop, cart, docs", one click), then the check boxes for a new group;
  the menu entry now also shows when only a group could take the session. "Edit group" has an
  "Add session …" list of the running agent sessions in no group; a chosen one shows with its
  default name, editable, can be taken out again and can be made the lead; Save renames first
  (so a newcomer may take a name a member gives up), then adds. Ctrl+click now also selects
  members: with the members of exactly one group among the selection, "Link" adds the others to
  that group; with two groups it is not offered.

## Simpler cards (2026-11-20)
The owner reported that the card below a member row ("Paste", "Paste and send", "Discard")
sometimes did not go away, and asked for it to be simpler. Causes found in the code, each built
with a test (`src/partnerMessage.test.ts`, `src/messageCard.test.ts`, `link_service.rs`):

- **A send to a working target waited without end and without a word.** `deliverMessage` holds
  while the target works (`holdUntilPaused`, no time limit, as decided). For link cards `onHeld`
  did nothing, so the card stood with greyed-out buttons; a session whose attention wrongly
  stayed "working" kept it forever. Now the card shows "Sends once {session} waits" with
  "Cancel" (`HoldProbe.cancelled`, result `cancelled`); the cancel counts until the paste begins
  (`onSending`), and the card goes by itself once the message is in.
- **No reload after an action.** Pasting and discarding relied on the backend's event alone; a
  removal that failed (`link_message_gone`, the message went meanwhile) left a stale card and an
  error for a message that had arrived. Now every card action reads the list again afterwards
  (`sendFromCard`), a message that is gone already counts as done without an error, and a
  discard hides the card at once (`discardFromCard`) and shows it again only when the backend
  kept the message.
- **A message that left the list during the wait** (group unlinked, target left) kept the wait
  running and later showed `link_message_gone`. The wait now ends when the backend no longer
  lists the message, without a word (`cardNotice`).
- **A claim nobody closes.** A window reloaded mid-paste or a lost answer left a message claimed
  for good: invisible (claimed messages show no card) and taking one of the five places. A claim
  now runs out after `CLAIM_TIMEOUT_MS` (120 s, `LinkQueue::release_stale`, applied on every
  `link_state` and `link_message_text`); the message then waits as a card, never on its own
  again. The window reads again once after that time while any message is claimed. A paste has
  no hard upper bound: the window waits up to 30 s for each stuck piece of input
  (`BUSY_LIMIT_MS` in `src/sessionInput.ts`, restarted after any progress), behind the input
  queued before it. So a slow automatic paste can outlast the claim; the window keeps the ids of
  the automatic deliveries it runs and shows no card for them (`heldBySession(messages,
  delivering)`), and a click on such a message is refused, so it is never pasted twice. A message
  whose paste went in but whose close was lost can still come back as a card, which the person
  sees and can discard.
- **Overlapping reads.** Each event reads the whole list; reads can overlap (`link_state` waits
  for the settings readers first), so an older answer could bring back a card a newer one had
  dropped. Only an answer newer than the last applied one counts now (`latestOnly`). Not seen in
  practice; the guard costs nothing.
- **Arm delay.** Link cards armed with the partner messages' generation only, so a new link card
  did not arm the others. All cards now arm anew whenever any card in the sidebar appears or goes
  (`cardsKey`).
- **Two clicks.** The busy flag lived in React state, read from the render; a send now starts
  through `CardSends`, which refuses a second start for the same message.

The card itself: sender and time in one line with a close mark (discard, tooltip and
`aria-label`), the text cut to 4 lines or 240 characters with "More", one primary "Send"
(paste and Enter) and "Paste only" as a text button. The same card serves partner messages;
their rule (nothing pasted before a click) is unchanged.

Why the window waits and the backend does not deliver on its own after the click: the wait
claims nothing (the claim comes only with the paste), so a reloaded window or a cancel leaves
the message as it was; the same path serves partner messages, which have no automatic delivery;
and no new state or command is needed in the backend. A window reload ends the wait, and the card
simply offers "Send" again.

## Evidence
- [[cross-project-paste-measurement]]: a bracketed paste with a header line arrives as one prompt,
  Enter as a separate write submits it (Claude Code, Codex CLI, Pi, Aider on Windows).
- [[paste-opencode-gemini-measurement]]: the same for OpenCode 2.3.2 and Gemini CLI 0.70.0 on
  Windows; Gemini CLI's trust dialog also takes the paste.
- The send route, token scopes and queue limits as built in [[0012-cross-project-messages]].

> [!question] Unverified
> Automatic Enter while an agent shows a permission or trust prompt: the target state must be
> "waiting for you" at the prompt, not at a dialog. The readiness signals of 0012 cannot always tell
> these apart; the loop brake and the per-group toggle are the fallback. For Claude Code from
> 2.1.268 the notice hooks of [[0016-permission-notices]] close this gap for permission and MCP
> input dialogs; a trust dialog at the first start has no hook and keeps the fallback.

> [!note] Secret guard
> A member's message whose text seems to hold a key or token waits as a card with a hint and never
> goes in on its own ([[0015-secret-guard]]).
