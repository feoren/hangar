---
title: Token usage in Claude Code transcripts
type: source
tags: [claude-code, costs, transcripts]
sources: []
raw: raw/2026-11-13-token-usage-measurement.md
url: https://platform.claude.com/docs/en/about-claude/pricing
retrieved: 2026-11-13
created: 2026-11-13
updated: 2026-11-13
---

# Token usage in Claude Code transcripts

Own measurement with Claude Code 2.1.322 on Windows 11, keys and counts only.

## Key points

- Every `assistant` record has `message.usage` with `input_tokens`, `output_tokens`,
  `cache_creation_input_tokens`, `cache_read_input_tokens`, the split `cache_creation`
  (`ephemeral_5m_input_tokens`, `ephemeral_1h_input_tokens`) and `speed`.
- Claude Code writes one record per content block: 1345 records held 771 `message.id` values, and
  all records of one id had identical counts. A response counts once per id.
- `timestamp` is RFC 3339 UTC with milliseconds, so the local day needs the user's time zone.
- `message.model` names the model of the response (`<synthetic>` for Claude Code's own notes with
  zero usage).
- Prices: the built-in table in `src-tauri/src/model_prices.toml` follows the model table cached
  2026-11-03 (see the raw file for the numbers).

## Significance for Hangar

The cost overview ([[costs-and-handover]]) sums usage per `message.id`, buckets it by local day
and prices it with the table. Main and sub-agent transcripts both count.

> [!question] Unverified
> - Cache read and write prices of Haiku 5.5 above 100K prompt tokens: Hangar applies the usual
>   factors (0.1x, 1.25x, 2x) to the long input price.
> - Fast mode cache prices: Hangar multiplies every price by the fast factor.
> - `SystemTimeToTzSpecificLocalTime` with no zone uses the current zone's rules for past dates;
>   historical rule changes are not applied.
