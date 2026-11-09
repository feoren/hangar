---
title: "Hangar account API (sign-in, account, projects)"
type: component
tags: [collaboration, teams, account, oauth, device-flow, keychain, api]
sources: []
version: "API v1 (base /api/v1), as of 2026-11-09"
license: "service API; client code in this repository is MIT"
repo: "website of the official service (not public); client in src-tauri/src/account*.rs"
created: 2026-11-09
updated: 2026-11-09
---

# Hangar account API (sign-in, account, projects)

The website of the official Hangar service is identity and billing for teams. The app signs in
through the OAuth device flow, reads the account and its projects, and later fetches short-lived
tickets for the relay (step D). A self-hosted server offers the same API under its own address.
Built in step F, part 1 ([[team-service-v1]]); licensing in [[0005-team-collaboration]].

> [!question] Unverified
> The endpoint behavior below is the service's own documentation as handed to the app team on
> 2026-11-09, not a measurement against the live service. The client was tested only against an
> in-test mock (`src-tauri/tests/account.rs`).

## Endpoints

Base `/api/v1`, default `https://hangar.feoren.com/api/v1`. All answers JSON with
`Cache-Control: no-store`; errors are RFC 9457 `application/problem+json` with `type`
(`urn:hangar:api:problem:<code>`), `title`, `status`, `detail`, `code`; the OAuth endpoints add
`error` and `error_description` (RFC 6749 5.2). Ids are opaque strings. Bodies are
`application/x-www-form-urlencoded` or a flat JSON object (the app sends JSON), at most 16 KiB.

| Method, path | Auth | Purpose |
| --- | --- | --- |
| `POST /oauth/device` | none | RFC 8628 3.1: `client_id=hangar-desktop`, `scope=team`, optional `device_name` (100), `platform` (40); answers `device_code`, `user_code` (`XXXX-XXXX`), `verification_uri`, `verification_uri_complete`, `expires_in` 600, `interval` 5 |
| `POST /oauth/token` | none | `grant_type=urn:ietf:params:oauth:grant-type:device_code` or `refresh_token`; access token `hgat_...` (900 s), refresh token `hgrt_...` |
| `GET /me` | Bearer | `id`, `email`, `name`, `locale`, `seats_account_total`, `seats_account_used` |
| `GET /projects` | Bearer | `{"projects":[...]}`: `id`, `name`, `role` (owner, admin, member), `owner_id`, `plan`, `seats_account_total`, `seats_account_used` (the owner's account), `pending_invitations` (owner and admin only), `invite_url` |
| `POST /relay-ticket` | Bearer | `{"project_id"}` -> `{"ticket","expires_at","relay_url"}`; 404 for non-members and unknown projects |
| `GET /keys` | none | public Ed25519 keys (not used by the app yet) |
| `GET/POST /devices` | none | reserved, 501 |

A missing, wrong, expired or revoked bearer token, or a locked account: 401 with
`WWW-Authenticate: Bearer realm="hangar", error="invalid_token"`.

## Device flow and tokens

- Polling errors: `authorization_pending` (ask again after `interval`), `slow_down` (add 5 s for
  all further polls), `access_denied`, `expired_token`, `invalid_grant` (unknown or already
  redeemed), `invalid_client`, `invalid_scope`.
- **Refresh tokens rotate.** Every refresh returns a new pair and spends the old refresh token.
  Presenting a spent one revokes the whole device (all its tokens); two concurrent refreshes of
  the same app count as such a reuse. Refreshes must therefore be serialized.
- There is no revoke endpoint. Connected devices are listed and revoked on the account page
  `/account/devices` of the server's origin.
- Seats count per account of a project's owner (ADR 0005, licensing).

## How Hangar uses it

- **Modules:** `account.rs` (pure: server address and origin checks, wire types, problem
  documents, polling rules, the window's view), `account_api.rs` (reqwest client),
  `account_service.rs` (state machine, tokens, single-flight refresh, keychain), commands in
  `commands.rs`. Types mirror in `src/account.ts`.
- **Server address:** https, or http only on `localhost`, `127.0.0.1` or `[::1]`; no user info,
  query or fragment; at most 512 characters. Stored in `account.json` in the config folder. The
  official address is the default; Settings can change it for self-hosting.
- **State machine** (`AccountState`): `idle` (optional notice `signedOut` or `sessionEnded`),
  `waitingForUser` (`userCode`, `verificationUri`, `verificationUriComplete`, `expiresAt`,
  `interval`), `signedIn` (`user` once `/me` answered), `failed` (`denied`, `expired`,
  `refused`, `network`, `server`, `foreignPage`, `keychain`, `badResponse`, with the server's
  cleaned text). A refresh token in the keychain at start means `signedIn`.
- **Polling:** waits `interval` seconds (default 5, clamped to 1 to 120), adds 5 s on
  `slow_down` for good, backs off after network errors and 5xx or 429 (doubling, at most 60 s),
  stops before the code expires and when cancelled (a raised flow id plus a wake-up, so a waiting
  poll ends at once).
- **Pages:** the confirmation page opens only when it and `verification_uri` have the server's
  own scheme, host and port; otherwise the flow fails with `foreignPage` and nothing opens. The
  same rule holds for `invite_url` (owners and admins only) and the devices page. The window names
  the page (`verification`, `devices`, `invite` with a project id), never a URL; the backend opens
  it through `desktop::open_url` (explorer.exe, `open`, `xdg-open`; on Windows an address with a
  comma or quote is refused because explorer.exe splits its command line at commas).
- **Tokens:** the refresh token lives only in the OS keychain (service `hangar-account`, account
  = the server address, so each server keeps its own sign-in) and in memory as `Zeroizing`; the
  access token only in memory. It is treated as expired 30 s early. No token appears in
  `Debug`, an error, a log line or an event; the device code stays in the backend.
- **Single flight:** one async mutex is held across every refresh, sign-in, sign-out and server
  change. A caller that saw an expired or refused token refreshes only when no newer token set
  exists (a generation counter); otherwise it uses the new one. A 401 is answered with one
  refresh and one retry; a second 401 or `invalid_grant` on refresh signs out locally with
  `sessionEnded` and deletes the keychain entry.
- **Rotation and the keychain:** the rotated refresh token is written to the keychain right after
  the refresh. Should that write fail, the running app keeps working, but the next start presents
  the spent token and the server revokes the device (logged, not shown).
- **Server text:** problem texts, project names, plans and the person's name reach the window
  without control characters and bidirectional marks (they could reorder what is read), at most
  300 characters.
- **Limits:** 15 s per request (connect and total), answers at most 512 KiB, no redirects,
  https-only client unless the base is loopback http.
- **Interface:** commands `account_status`, `account_sign_in`, `account_cancel_sign_in`,
  `account_sign_out`, `account_projects`, `account_set_base_url(baseUrl | null)`,
  `account_open(page)` (main window only); event `account-changed` with `AccountView { baseUrl,
  official, devicesUrl, state }`; errors as `{ kind, detail }`. `relay_ticket` is a typed client
  function, not a command yet.
