---
title: "GitHub REST: conditional requests and rate limits"
type: source
tags: [updates, github, network]
sources: []
raw:
url: https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# GitHub REST: conditional requests and rate limits

Official docs, read 2026-11-12:
[Best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
and [Rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api),
plus our own measurement on the same day.

## Key points
- "Most endpoints return an `etag` header"; send it back in `if-none-match`, and an unchanged
  resource answers `304 Not Modified`.
- A conditional request does not count against the primary rate limit "if a `304` response is
  returned and the request was made while correctly authorized with an `Authorization` header".
  Hangar sends no `Authorization`, so its `304` answers count.
- Measured unauthenticated against `/repos/PowerShell/PowerShell/releases/latest`:
  `X-RateLimit-Remaining` went 53 (200), 52 (304), 51 (304). The `304` saves the body only.
- "The primary rate limit for unauthenticated requests is 60 requests per hour", bound to the
  originating IP address.
- Exceeding the primary or a secondary limit yields "a `403` or `429` response".
- Waiting: if `retry-after` is present, wait that many seconds; if `x-ratelimit-remaining` is `0`,
  wait until `x-ratelimit-reset` ("UTC epoch seconds"); "Otherwise, wait for at least one minute".
- Same day, also measured: PyPI `/pypi/<package>/json` sends an `ETag` and answers `304`;
  npm `/<package>/latest` sends no `ETag`.

## Significance for Hangar
Applied in [[program-updates]] (`src-tauri/src/updates.rs`: `settle`, `rate_limited_until`).
