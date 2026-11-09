# Interop run: Hangar app against a self-hosted team relay (2026-11-09)

Platform: Windows 11 Pro, build 26300. The relay ran by hand in self-host mode (`IDENTITY=local`)
on the loopback, the app side was the ignored test `src-tauri/tests/relay_interop.rs`, run as

```
HANGAR_INTEROP_BASE=http://127.0.0.1:<port> \
HANGAR_INTEROP_LINK_A=<one-time login link, owner> \
HANGAR_INTEROP_LINK_B=<one-time login link, member> \
  cargo test --manifest-path src-tauri/Cargo.toml --test relay_interop -- --ignored --nocapture
```

Part 1 pointed the app's own account client (`AccountApi`) at the relay's self-host API. Part 2
bridged the account API through the test's adapter on 127.0.0.1 and ran the real
`AccountService` and `TeamService` of two accounts against the real relay.

## Live protocol (part 2): compatible

- `hello` answered with `welcome`; the relay accepted the app's protocol range.
- 6 `ticket.renew` on one connection, each answered with `ack`.
- `presence.set` reached the other member as `presence.update` and opened with
  `open_verified`.
- `claim.set` of both members produced `claim.overlap` with the sealed detail; the app refined
  it to overlapping lines.
- The membership log grew to a rotation: the owner removed a member, the relay admitted the
  remove entry and the rotation to epoch 2.
- After the member's removal the relay closed the member's connection with 4403.
- After the project was deleted the relay closed with 4404.

Reservations (`task.*`) were not part of this run: the app did not send them yet.

## Self-host account API (part 1): does not match the app's account API

| Topic | The app expects (website API v1) | The self-hosted relay serves |
| --- | --- | --- |
| Base paths | OAuth and the API under one base `/api/v1` | OAuth at the root, the API under `/v1` |
| Device authorization | `POST /oauth/device` | `POST /oauth/device_authorization` |
| Request bodies | form or flat JSON | form only; JSON is answered with 415 |
| Client id | `hangar-desktop` | `hangar` |
| Projects | `GET /projects` | none |
| Relay ticket | `POST /relay-ticket` `{project_id}` -> `{ticket, expires_at, relay_url}` | `POST /v1/tickets` `{team}` -> `{ticket, expiresIn, live}` |
| Token revocation | `POST /oauth/revoke` (RFC 7009) | none |
| Errors | RFC 9457 problem documents | not RFC 9457 |
| `GET /me` | `id`, `email`, `name`, ... | without `name` |
| Invitations | `invite_url` per project | none |
| Device list | page `/account/devices` | none |

Reported to the relay's maintainers on 2026-11-09.
