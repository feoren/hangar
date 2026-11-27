---
title: Output rate of a Claude Code session from its transcript
type: source
tags: [agents, claude-code, tokens, transcripts, sidebar]
sources: ["[[token-usage-measurement]]", "[[context-window-measurement]]"]
raw: raw/2026-11-27-output-rate-measurement.md
url: ""
retrieved: 2026-11-27
created: 2026-11-27
updated: 2026-11-27
---

# Output rate of a Claude Code session from its transcript

Own measurement on Windows 11 with main transcripts of Claude Code 2.1.319, 2.1.330 and 2.1.332
for the output rate on the session rows (`src-tauri/src/context_usage.rs` `OutputRate`,
`src/SessionRow.tsx` `RowSpeed`). Counts, type names and times only.

## Key points (2.1.319 to 2.1.332)
- **Final count on every record:** all records of one `message.id` carry the same
  `usage.output_tokens`, the first one included, though their `timestamp`s lie seconds apart
  (as in [[token-usage-measurement]] for 2.1.322). Conclusion: a response's records land in the
  file when the response is complete; its output is known then, never while it streams.
- **Adjacency:** records of one id nearly always follow each other; a few times another record
  came between, so a reader dedupes by id, not by neighbour.
- **Start of a response:** the record directly before it is mostly `api-request` (2.1.319 and
  later), about 0.05 s after the `user` record (prompt or tool result) it answers. `attachment`
  records directly before a response often carry a time at or after its first record and give
  spans of 0 s or less; `user` records are a sound start.
- **Numbers:** with the `user` record as start, a response takes a median of 4.5 to 5.9 s, 95 %
  within 22 to 26 s, and writes a median of 85 to 97 output tokens per second (95 % between 44
  and 125).

## Significance for Hangar
- The row's rate is the output tokens of the responses that ended in the last 30 s over the time
  they took together, each from the prompt or tool result it answers (or the end of the response
  before, if later) to its last record. Attachments and other records do not start a span; a
  response whose start the read did not see does not count.
- 30 s covers 95 % of the measured responses, so the number stays live while an agent works
  through tool calls. 30 s after the last response it is no longer live, but it stays: the row
  keeps the rate of the last window that had a response (`Rate { value, live: false }`), dimmed,
  and the peek says "Last response: 84 tokens per second". Before a session's first measured
  response the row shows nothing, never a 0. The row shows the rate in every state, not only
  while the session works.
- The kept rate survives a fresh read of the transcript and a switch to another conversation id
  (restart, resume fallback): the row is still the same session. It shows again once the new
  conversation's context use is known and is replaced by the first response measured there.
  Claude Code's `/clear` writes to a new transcript Hangar does not follow
  ([[context-window-measurement]]), so the row keeps the old numbers until then.
- A response longer than 30 s leaves the rate "not live" while it runs: the transcript only
  shows a response once it is complete, so no live number exists during it.
- On the title line the rate stays down to the default sidebar width (320 px, a title line of
  239 px); below 260 px the state word gives up its fixed cell first, below 225 px the rate steps
  back (measured in Chromium with the README mock).
- The transcript only grows when a response or tool result lands, so the poll runs every second
  while a transcript grew in the last 30 s and every second second otherwise (the idle cost of
  [[context-window-measurement]]'s 2 s poll).
- Codex rollouts still cannot be tied to a session ([[context-window-measurement]]); other
  programs show no rate, and none is estimated from terminal bytes.

> [!question] Unverified
> - That the records are appended at completion rather than written earlier and rewritten: the
>   files are append-only JSONL and every record already has the final count, but the write
>   moment itself was not observed.
> - Whether `output_tokens` counts thinking: Hangar assumes the API's usage does, so a thinking
>   phase counts as output.
