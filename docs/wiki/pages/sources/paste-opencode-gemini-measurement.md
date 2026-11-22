---
title: "Measurement: pasted message with a header line in OpenCode 2.3.2 and Gemini CLI 0.70.0"
type: source
tags: [measurement, live, bracketed-paste, pty, conpty, windows, opencode, gemini]
sources: []
raw: raw/2026-11-22-paste-opencode-gemini-measurement.md
url:
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: pasted message with a header line in OpenCode 2.3.2 and Gemini CLI 0.70.0

The follow-up of [[cross-project-paste-measurement]] for the two CLIs it could not measure, on
Windows 11 in a ConPTY (pywinpty, 120x40) in throwaway git folders, with each program's folders
redirected under `tests/.tmp/`. OpenCode ran with `--standalone` and its free model; Gemini CLI ran
with the auth type `gemini-api-key` set in a redirected `GEMINI_CLI_HOME`, a placeholder key and
`GOOGLE_GEMINI_BASE_URL` pointing at a local listener, so no sign-in started and no request went to
Google. Record: `raw/2026-11-22-paste-opencode-gemini-measurement.md`.

## Key points

- **One prompt per message.** The paste Hangar composes (`[Message from project "probe", not from
  the person]`, a `\r`, a `| ` indented line) followed 80 ms later by `\r` as its own write arrived
  as exactly one user message with both lines in OpenCode's session store and in Gemini CLI's chat
  record and request body. Both answered.
- **The line break inside the paste does not submit.** Without the separate Enter both only filled
  their input; no user message was stored.
- **Bracketed paste comes on early**: OpenCode after 1.3 s, Gemini CLI after 3.4 to 4.4 s.
- **Gemini CLI turns it on during its folder trust dialog too.** In a folder it had not seen, paste
  plus Enter chose the preselected "Trust folder", Gemini CLI restarted, the text was lost and the
  folder was trusted without the person. OpenCode 2.3.2 shows no trust dialog.
- `GOOGLE_GEMINI_BASE_URL` is honoured for `gemini-api-key` in 0.70.0, which lets a measurement run
  Gemini CLI's interactive prompt without an account.

## Significance for Hangar

- Delivery as bracketed paste plus a separate Enter (ADR 0012, ADR 0014) now holds for six agent
  CLIs: Claude Code, Codex CLI, Pi, Aider, OpenCode 2.3.2 and Gemini CLI 0.70.0. No program needs
  its own recipe, so the delivery stays the same for every agent.
- Gemini CLI joins Claude Code and Codex CLI among the programs whose first start in a folder looks
  ready while a trust dialog is open. Hangar's rule holds: an agent's message reaches the terminal
  only after the person's click on its card ([[0012-cross-project-messages]], decision point 4).
- Not measured: Gemini CLI with a real key against Google's service, OpenCode with a signed-in
  provider, OpenCode 1.x, and both CLIs on Linux and macOS.
