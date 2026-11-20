---
title: "ADR 0015: Secret guard"
type: decision
status: accepted
superseded_by:
tags: [security, secrets, messages, links, commit, paste, command-guard]
sources: []
created: 2026-11-20
updated: 2026-11-20
---

# ADR 0015: Secret guard

## Context
Agents and the person move text around in Hangar: link group messages go into another agent's
prompt on their own ([[0014-session-links]]), partner projects' messages wait as cards
([[0012-cross-project-messages]]), the commit box commits staged changes, and the person pastes
into sessions. Any of these can carry an API key, a token or a private key by mistake: an agent
that read `.env` and quotes it to a peer, a key staged with a config file, a token copied from a
browser. Once it reached another agent's context or a commit, it is hard to take back.

## Options
1. **A secret scanner dependency (gitleaks rules, a regex crate with a large rule set).** Hundreds
   of rules, many with high false positives on code; a new dependency on both sides.
2. **A few precise shapes, hand-written, in one module per side with shared test vectors.** Only
   shapes with a fixed prefix and a known body: few false positives, no dependency, easy to keep
   equal in Rust and TypeScript.
3. **Nothing; rely on the agents' own care.**
4. **Reuse the sub-agent view's masking (`src-tauri/src/subagents/redact.rs`).** It masks on purpose
   more than it must (any `*_KEY=value`, `password: ...`, a token-looking word after a secret
   name): right for hiding text in a view, wrong for holding a message or a commit, which would
   then stop on harmless code. It also names no kind.

## Decision
Option 2, kept small.

- **Shapes** (`src-tauri/src/secret_scan.rs`, `src/secretScan.ts`): a PEM private key block
  (`-----BEGIN ... PRIVATE KEY-----` with at least 40 base64 characters within 512 characters
  after it), an AWS access key id (`AKIA`, `ASIA`, `ABIA`, `ACCA` and exactly 16 upper case
  letters or digits), GitHub tokens (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_` with 36 or more letters
  or digits, `github_pat_` with 60 or more), Anthropic keys (`sk-ant-` and 40 or more), OpenAI keys
  (`sk-proj-`, `sk-svcacct-`, `sk-admin-` and 40 or more, or a legacy `sk-` key with `T3BlbkFJ`
  inside), Slack tokens (`xoxb-` and the other `xox?-` and `xapp-` prefixes, 20 or more with a digit
  and a dash) and Stripe live secret or restricted keys (`sk_live_`, `rk_live_`, 24 or more).
  Every shape needs a word boundary in front and a body that looks random: at least ten different
  characters (eight for AWS), none of `xxxx`, `example`, `placeholder`, `redacted`; the AWS
  documentation key (ending in `EXAMPLE`) does not count. Stripe test keys do not count.
- **Result:** only the kinds (`SecretKind`: `privateKey`, `awsAccessKey`, `githubToken`,
  `anthropicKey`, `openaiKey`, `slackToken`, `stripeKey`, and `secretFile` for a staged file
  name), never the matching text or its place. Nothing logs or shows the match; a failed git call
  logs git's stderr only, never the diff.
- **Bounded and linear:** a token body counts up to 512 characters (`MAX_TOKEN_BODY`) on both
  sides. Every shape checks its word boundary before it reads a run, so runs never overlap and the
  scan stays linear (8 MiB of one prefix repeated: under 1 s per prefix in a debug build, tested);
  the window's regular expressions carry the same upper bound, since an open-ended repetition over
  megabytes ran out of stack, and a check that throws still warns ("kind not known").
- **Both sides check the same vectors:** `src-tauri/tests/data/secret-scan-vectors.json`, each text
  made of parts joined at run time, so no complete key shape is stored in the repository and no
  secret scanner reports the test data. Every value is made up.
- **Link group messages:** a message whose text holds a secret is never delivered on its own,
  whatever the group's "Ask before pasting" says; it waits as a card with "Probably contains a
  secret key (kind)" (the existing hold of ADR 0014: `auto` off, `link_message_text` with
  `automatic` refused). It does not count against the loop brake. The sender gets `waiting` with a
  sentence that names the kind and asks not to send keys in messages.
- **Partner project messages** always wait for the person already; their card shows the same hint,
  and the sender's answer names the kind. The kinds are found anew on every read
  (`WaitingView`), so the stored file keeps its shape.
- **Command guard:** a built-in rule `hangar-secret-file` ([[0010-command-guard]]) for `git add`
  and `git stage` naming a file that usually holds a secret: `.env` and `.env.<name>` except
  `.env.example`, `.env.sample`, `.env.template`; `*.pem`, `*.key`, `*.p12`, `*.pfx`; `id_rsa`,
  `id_dsa`, `id_ecdsa`, `id_ed25519`; `credentials.json`, `.netrc`, `_netrc`, `.pgpass`. Lexical
  like every rule; like Hangar's other built-in rules it cannot be answered with "always".
- **Commit box:** before a commit from the app, the added lines of the staged diff are scanned
  (`git diff --cached --text --unified=0`, filters, external diff drivers and text conversions off,
  at most 8 MiB, git and the scan together within 10 seconds), and the staged file names are
  checked like the command guard's (`secretFile`, a `.env` staged, say). With a finding nothing is
  committed; the box lists file and kind with "Commit anyway" and "Cancel". The answer carries the
  index's tree id (`git write-tree`); "Commit anyway" hands it back (`allow_tree`) and the scan is
  skipped only when the index is still exactly that tree, otherwise it asks again. A check that
  fails or runs out of time lets the commit go ahead (a helper, not a gate), and the answer says
  so: `Committed { checked: full | partial | skipped }`, with a short note in the box when not
  full. More than 20 files are counted, not listed; a cut diff says so.
- **Paste:** the person's paste (Ctrl+V, Shift+Insert) and "Send to" from the app ask first with
  "The text probably contains a secret key (kind)", also when the line and character questions
  are switched off in Settings. Broadcasts, queued messages and agent commands stay as they were:
  the person wrote or chose that text.

## Consequences
- New glossary terms: secret guard, secret kind.
- **What passes unseen.** A key of another kind; a key split across lines, joined from parts,
  encoded (base64, hex, URL escapes) or put into a string concatenation; a key longer than the
  bound; anything past the first 8 MiB of a staged diff or the 10 seconds; `git add .` or `-A`
  with an unignored `.env` in the command guard (the commit box still names the staged `.env`);
  a secret file under a name that is not on the list. Whoever wants to get a key past the guard
  can; it catches the common mistakes only.
- False positives keep a message as a card or ask once before a commit or paste; that is the
  safe direction.

## Evidence
- Code: `src-tauri/src/secret_scan.rs`, `links.rs` (`LinkMessage::secrets`), `link_service.rs`
  (`HeldBy::Secret`), `partner_service.rs` (`WaitingView`, `Accepted::secrets`),
  `command_guard.rs` (`RULE_SECRET_FILE`, `git_adds_secret_file`), `git_actions.rs`
  (`commit_checked`, `added_secrets`), `src/secretScan.ts`, `src/appPaste.ts`,
  `src/clipboardPaste.ts`, `src/ChangesPanel.tsx`, `src/WaitingMessages.tsx`.
- Tests: `secret_scan::tests`, `link_service::tests::a_message_with_a_secret_waits_for_a_click_and_never_counts_against_the_brake`,
  `partner_service::tests::a_message_with_a_secret_still_waits_and_both_answer_and_card_name_the_kind`,
  `command_guard::tests::git_add_of_a_file_that_usually_holds_a_secret_is_asked_about`,
  `git_actions::tests::a_commit_with_a_staged_secret_waits_for_the_person`,
  `git_actions::tests::added_lines_with_a_secret_name_their_file_and_kind_only`,
  `src/secretScan.test.ts`, `src/appPaste.test.ts`, `src/clipboardPaste.test.ts`,
  `src/gitActions.test.ts`, `src/ChangesPanel.test.tsx` (`CommitSecretAsk`, `CommitCheckNote`),
  `src/WaitingMessages.test.tsx`.

> [!question] Unverified
> The token formats are taken from the vendors' public prefixes as known on 2026-11-20 and not
> re-read from each vendor's documentation for this ADR; a vendor that changes its prefix needs a
> new shape and vectors.
