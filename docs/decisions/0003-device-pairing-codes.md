# 0003. Sign a device in with a pairing code

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

A device needs credentials to sync and download. Every reader screen works with buttons alone (product.md, Display capabilities), so typing a username and password with arrow keys is the worst case. product.md locks local users first and leaves OIDC for later.

## Options considered

1. **Username and password on the device.** The device logs in and gets a token. Slow to type with buttons, and the device keeps a password or asks for it again.
2. **Pairing code.** The device shows a short code. The user enters it in shelf's admin UI on another screen. The device then receives its own token. This is the OAuth device authorization grant pattern (RFC 8628) used by TVs and consoles.

## Decision

Option 2.

1. The device calls `POST /pair/start`. Shelf returns a secret `device_code`, a short `user_code` (for example `K7Q-4MD`), the admin URL to visit, a poll interval, and an expiry of ten minutes.
2. The device shows the `user_code` and the URL.
3. A signed-in user enters the code in the admin UI and names the device.
4. The device polls `POST /pair/token` with the `device_code`. After approval it receives a long-lived bearer token, shown once.
5. The token authorizes `/sync` and downloads for that user. The admin UI lists devices and revokes any of them.

Shelf stores only hashes of `device_code` and the token. User codes avoid letters that look alike (0 and O, 1 and I).

The admin UI uses a username and password with a session cookie. OPDS reader apps use HTTP Basic auth, because that is what OPDS clients support.

## Consequences

- Pairing needs no typing on the device.
- Each device can be revoked alone. A lost device does not mean changing the user's password.
- The radio stays up for the pairing poll, once per device.
- Shelf has three ways to authenticate: session, Basic, and bearer token. Each route states which it accepts.
- When OIDC arrives, pairing stays the same. Only the admin UI sign-in changes.
