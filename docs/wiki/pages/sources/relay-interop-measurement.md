---
title: "Measurement: the app against a self-hosted team relay (interop run)"
type: source
tags: [collaboration, teams, relay, protocol, account, interop, measurement]
sources: []
raw: raw/2026-11-09-relay-interop-run.md
url:
retrieved: 2026-11-09
created: 2026-11-09
updated: 2026-11-09
---

# Measurement: the app against a self-hosted team relay

Own interop run on Windows 11 (build 26300) with the ignored test
`src-tauri/tests/relay_interop.rs` against a relay in self-host mode on the loopback.

## Key points
- **Live protocol compatible:** `hello` and `welcome`, six `ticket.renew` on one connection,
  presence opened with `open_verified`, claims with an overlap refined to lines, a membership log
  that rotated to epoch 2 after a removal, close 4403 after the member's removal and 4404 after
  the project's deletion.
- **Self-host account API differs** from the website API the app speaks: OAuth at the root and
  the API under `/v1` (not one base `/api/v1`), `/oauth/device_authorization` instead of
  `/oauth/device`, form bodies only (415 for JSON), client id `hangar` instead of
  `hangar-desktop`, no `/projects`, `/v1/tickets` `{team}` -> `{ticket, expiresIn, live}` instead
  of `/relay-ticket` `{project_id}` -> `{ticket, expires_at, relay_url}`, no `/oauth/revoke`,
  errors not RFC 9457, `/me` without `name`, no `invite_url`, no `/account/devices`. Reported to
  the relay's maintainers.
- Reservations were not exercised (the app did not send `task.*` at the time).

## Significance for Hangar
- The relay side of steps C to F holds: the app needs no protocol change for a self-hosted
  relay.
- Self-hosting works only once the self-host server serves the account API of
  [[hangar-account-api]] (or the app gains a second client); until then the test's adapter
  bridges it. The test fails while part 1 finds a mismatch.
- Next run: include reservations ([[team-service-v1]], part 2b, tasks).
