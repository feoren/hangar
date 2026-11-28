# Measurement: response times and output tokens in Claude Code transcripts (2026-11-27)

Platform: Windows 11 Pro 10.0.26300. Claude Code 2.1.332 installed; the three main transcripts
read were written by 2.1.332, 2.1.330 and mostly 2.1.319 (some records 2.1.321).

Method: three small Python scripts read main transcripts under `%USERPROFILE%\.claude\projects\`
(the newest one of a running session and the largest of two other projects) and printed only
counts, type names and times, never message text (`py -3 -I <script> <transcript>...`).

1. `rate_stats.py`: per `message.id` of an `assistant` record (synthetic ones skipped): the number
   of records, whether all carry the same `usage.output_tokens`, whether they follow each other,
   the type of the record before the first one, and the span from that record's `timestamp` to
   the response's last record.
2. `rate_hangar.py`: Hangar's first rule: lines that start with `{"type":"` and are neither `user`
   nor `assistant` are skipped unparsed (like `skipped_cheaply`), every other timestamped record
   of the main chain starts the next response's span.
3. `rate_hangar2.py`: the rule Hangar uses: only `user` records (prompts, tool results) start a
   span; a response record moves the start to its own time if that is later, so two responses
   in a row do not overlap. `attachment` and other parsed records are ignored.

Output (verbatim):

```
version 2.1.332 responses 112 records 217 ids with differing output_tokens 0 records of an id not adjacent 1
record type before a response {'api-request': 103, 'file-history-delta': 2, 'attachment': 4, 'queue-operation': 3}
span s: median 4.1 p5 -0.0 p95 17.7 max 57.7
tokens/s: median 89 p5 53 p95 120
version 2.1.330 responses 227 records 520 ids with differing output_tokens 0 records of an id not adjacent 18
record type before a response {'api-request': 135, 'attachment': 81, 'queue-operation': 7, 'file-history-delta': 3, 'user': 1}
span s: median 3.0 p5 -0.0 p95 17.2 max 99.3
tokens/s: median 103 p5 53 p95 389
version 2.1.321 responses 718 records 1395 ids with differing output_tokens 0 records of an id not adjacent 22
record type before a response {'api-request': 547, 'queue-operation': 35, 'attachment': 107, 'user': 4, 'file-history-delta': 25}
span s: median 4.3 p5 -0.0 p95 22.4 max 106.7
tokens/s: median 101 p5 66 p95 232
---
responses 112 without a start 0 record before {'attachment': 26, 'user': 86}
span s: median 4.7 p5 2.0 p95 25.9
tokens/s per response: median 87 p5 36 p95 139
responses 227 without a start 38 record before {'attachment': 136, 'user': 91}
span s: median 4.3 p5 0.0 p95 26.7
tokens/s per response: median 99 p5 45 p95 608
responses 718 without a start 40 record before {'attachment': 229, 'user': 489}
span s: median 5.7 p5 0.0 p95 26.6
tokens/s per response: median 97 p5 49 p95 274
---
responses 112 without a start 0 record before {'attachment': 26, 'user': 86}
span s: median 4.5 p5 2.2 p95 22.1
tokens/s per response: median 85 p5 49 p95 119
responses 227 without a start 0 record before {'attachment': 136, 'user': 91}
span s: median 4.5 p5 1.6 p95 22.5
tokens/s per response: median 92 p5 44 p95 120
responses 718 without a start 0 record before {'attachment': 229, 'user': 489}
span s: median 5.9 p5 1.8 p95 25.6
tokens/s per response: median 97 p5 53 p95 125
```

("version" in the first block is the highest version string in the file. "record before" in the
third block still names the record directly before, not the one that starts the span.)

Second observation, live, 2.1.332: the tail of a running sub-agent transcript, read while its
tool call ran (`date -u` printed 11:20:47), ended with two `assistant` records of one
`message.id`: a `thinking` block stamped 11:20:42.836 and a `tool_use` block stamped
11:20:47.203, both with `output_tokens` 1323, after an `api-request` record stamped 11:20:30.853
and the `tool_result` `user` record before it at 11:20:30.801. Earlier responses of that file
looked the same (records seconds apart, the same final count on each).

Reading:

- Every record of a response carries the response's final `usage.output_tokens`, also the first
  one, whose `timestamp` lies seconds before the last. Conclusion (not observed byte by byte):
  Claude Code appends a response's records when the response is complete, so a reader learns its
  output only then, not while it streams.
- Records of one id are nearly always adjacent; 1, 18 and 22 times another record came between.
- The record directly before a response is mostly `api-request` (2.1.319 and later), which comes
  about 0.05 s after the `user` record it answers. `attachment` records directly before a
  response often carry a time at or after the response's first record (spans of 0 s or below,
  rates up to 608 tokens per second), so they cannot start a span; `user` records can.
- With `user` records as start, responses take a median of 4.5 to 5.9 s, 95 % within 22 to 26 s;
  output runs at a median of 85 to 97 tokens per second, 95 % between 44 and 125.
