# Cipher protocol primitives v1

This contract defines the shared wire values used by Cipher's HTTP, realtime,
and desktop contracts. The checked-in [golden fixture](../../contracts/primitives/v1.json)
is the compatibility baseline for every implementation.

## Application identifiers

Cipher provisions an application-owned `UserId` and records a private one-to-one
mapping from the verified Cognito subject to that ID. A Cognito subject is
authentication-provider data; it is not a Cipher protocol identifier and is never
returned in an API, realtime frame, or IPC view.

Every identifier is a lower-case RFC 9562 UUID version 7 preceded by its exact
prefix. The prefix and strong native type make resource identifiers visibly and
programmatically distinct.

| Value           | Prefix | Wire example                               |
| --------------- | ------ | ------------------------------------------ |
| User ID         | `usr_` | `usr_018f9a76-4c00-7a12-8b0c-4d5e6f708192` |
| Device ID       | `dev_` | `dev_018f9a76-4c01-7a12-8b0c-4d5e6f708192` |
| Session ID      | `ses_` | `ses_018f9a76-4c02-7a12-8b0c-4d5e6f708192` |
| Conversation ID | `cnv_` | `cnv_018f9a76-4c03-7a12-8b0c-4d5e6f708192` |
| Server ID       | `srv_` | `srv_018f9a76-4c04-7a12-8b0c-4d5e6f708192` |
| Channel ID      | `chn_` | `chn_018f9a76-4c05-7a12-8b0c-4d5e6f708192` |
| Message ID      | `msg_` | `msg_018f9a76-4c06-7a12-8b0c-4d5e6f708192` |
| Event ID        | `evt_` | `evt_018f9a76-4c07-7a12-8b0c-4d5e6f708192` |
| Media ID        | `med_` | `med_018f9a76-4c08-7a12-8b0c-4d5e6f708192` |

## Time, cursors, idempotency, and versions

- Timestamps use `YYYY-MM-DDTHH:MM:SS.mmmZ`: UTC only, exactly three fractional
  digits, and a real calendar date.
- A cursor is an opaque server-issued, unpadded base64url value prefixed with
  `cur_`. Clients must not construct, order, or inspect it.
- An idempotency key is a client-generated lower-case UUIDv7 prefixed with
  `idem_`. Reuse it only when retrying the same authenticated mutation. The
  server retains a matching result for 24 hours.
- A protocol version is a JSON number. `0` and `1` are valid values, but no
  version is globally accepted: each HTTP, realtime, or IPC contract publishes
  its own compatibility window.

## Error envelope

Every shared protocol error serializes as:

```json
{
  "code": "unsupported_version",
  "message": "This Cipher version is no longer supported.",
  "retryable": false
}
```

`code` is one of `invalid_request`, `unauthenticated`, `forbidden`, `not_found`,
`conflict`, `duplicate`, `stale`, `expired`, `unsupported_version`, `too_large`,
`rate_limited`, `unavailable`, or `internal`. Messages are safe for display and
logs, have no control characters or surrounding whitespace, and are limited to
256 UTF-8 bytes. An `internal` message must not reveal sensitive implementation
details.

## Size limits

| Value                                                 |     Limit |
| ----------------------------------------------------- | --------: |
| HTTP request or response body (not direct S3 uploads) |    64 KiB |
| Realtime frame                                        |    64 KiB |
| Encrypted message body before transport encoding      |    32 KiB |
| Photo source                                          |     5 MiB |
| Photo width or height                                 |   2048 px |
| Cursor                                                | 512 bytes |
