# Cipher HTTP API v1

This document is the normative contract for Cipher's versioned HTTPS API. It
defines the wire format before the authenticated application endpoints are
implemented. `GET /v1`, `/healthz`, `/readyz`, and the WebSocket upgrade route
are the only routes currently served; the resource routes below are reserved
contracts, not an indication that durable messaging is available.

The realtime wire protocol is defined separately. `GET /v1/realtime` performs
only the HTTP-to-WebSocket upgrade described by that protocol.

## Versioning and transport

- Production API requests use HTTPS. Local development may use loopback HTTP.
- The major version is the first path component: `/v1/...`.
- Requests and responses use UTF-8 JSON with `Content-Type: application/json`.
  Multipart requests and untyped JSON are not supported.
- Every versioned JSON response includes `X-Cipher-Api-Version: v1`.
- An unknown major version returns `406` and `unsupported_version`; clients
  must not silently retry it against another major version.
- Within v1, Cipher may add optional fields, enum values, endpoints, and
  response metadata. Clients must ignore fields they do not understand and
  reject only enum values that affect a security decision. Existing fields,
  semantics, and error codes are never repurposed within the major version.

All authenticated endpoints require `Authorization: Bearer <Cognito access
token>`. The server validates the token issuer, audience, expiry, and
signature. A valid token does not by itself grant access to a conversation or
media object. `POST /v1/devices` is the only authenticated call that can
register a new device; all other state-changing calls require a registered,
non-revoked device identity in `X-Cipher-Device-Id`. The server verifies that
the device belongs to the token's user before authorizing the request.

Cipher does not expose whether a user, conversation, message, media object, or
invite exists to callers who lack access. Such requests return `404` rather
than a more revealing authorization error.

## Envelopes

Successful responses use this envelope:

```json
{
  "data": {},
  "meta": {
    "apiVersion": "v1",
    "requestId": "req_01f4c7c7dc9d4cf9a0d4d66a7fa8b24b"
  }
}
```

`requestId` is an opaque server-generated correlation identifier. Clients may
include it in support reports but must not derive ordering, identity, or
authorization from it. Collection responses additionally include `meta.page`:

```json
{
  "data": [],
  "meta": {
    "apiVersion": "v1",
    "requestId": "req_01f4c7c7dc9d4cf9a0d4d66a7fa8b24b",
    "page": { "nextCursor": "opaque-cursor" }
  }
}
```

The final page includes `"page": {}`. A cursor is opaque and is valid only
for the same authenticated principal, endpoint, filter set, and v1 API
contract. It must never be decoded, edited, or reused for another query.

Failed responses use this envelope:

```json
{
  "error": {
    "code": "invalid_request",
    "message": "The request is malformed.",
    "details": [
      {
        "field": "limit",
        "code": "out_of_range",
        "message": "limit must be between 1 and 100."
      }
    ]
  },
  "meta": {
    "apiVersion": "v1",
    "requestId": "req_01f4c7c7dc9d4cf9a0d4d66a7fa8b24b"
  }
}
```

`details` is present only for client-correctable validation failures. Error
messages are safe to display but are not a source of program logic; clients
branch on `error.code` and HTTP status.

## Endpoint contract

| Method   | Path                                                  | Authentication        | Contract                                                                           |
| -------- | ----------------------------------------------------- | --------------------- | ---------------------------------------------------------------------------------- |
| `GET`    | `/v1`                                                 | None                  | Return the active API major version and media type.                                |
| `GET`    | `/v1/me`                                              | Access token          | Return the caller's user and device-safe profile.                                  |
| `POST`   | `/v1/devices`                                         | Access token          | Register one public device identity and its public key material.                   |
| `DELETE` | `/v1/devices/{deviceId}`                              | Access token + device | Revoke the named caller-owned device.                                              |
| `GET`    | `/v1/conversations`                                   | Access token + device | List conversations visible to the device.                                          |
| `POST`   | `/v1/conversations`                                   | Access token + device | Create a conversation and its initial membership.                                  |
| `GET`    | `/v1/conversations/{conversationId}`                  | Access token + device | Return a visible conversation's metadata and membership.                           |
| `PATCH`  | `/v1/conversations/{conversationId}`                  | Access token + device | Change mutable conversation metadata, subject to role checks.                      |
| `POST`   | `/v1/conversations/{conversationId}/members`          | Access token + device | Add or change a member according to the conversation role policy.                  |
| `DELETE` | `/v1/conversations/{conversationId}/members/{userId}` | Access token + device | Remove a member according to the conversation role policy.                         |
| `GET`    | `/v1/conversations/{conversationId}/messages`         | Access token + device | List encrypted message envelopes in cursor order.                                  |
| `POST`   | `/v1/conversations/{conversationId}/messages`         | Access token + device | Submit one encrypted message envelope. The server never accepts message plaintext. |
| `POST`   | `/v1/media/uploads`                                   | Access token + device | Create a bounded upload capability for encrypted media ciphertext.                 |
| `GET`    | `/v1/media/{mediaId}`                                 | Access token + device | Return metadata and a time-limited download capability for visible ciphertext.     |

Path identifiers, timestamps, cursors, idempotency keys, and size limits use
the canonical serialization rules from the application primitives contract. A
resource response contains only the fields required by its endpoint; private
keys, access tokens, refresh tokens, MLS state, plaintext, and raw object-store
credentials are never returned by this API.

## Pagination, limits, and writes

List routes accept these query parameters:

- `limit`: optional integer from `1` through `100`; default `50`.
- `cursor`: optional opaque cursor returned by the preceding page.

The maximum JSON request body is 64 KiB. Media ciphertext is transferred only
through the bounded upload capability, never through a JSON HTTP request.
Endpoints may impose smaller documented field limits. A response body larger
than a client's available memory must be handled as a bounded page, not a
single unbounded collection.

Every `POST`, `PATCH`, and `DELETE` request requires an `Idempotency-Key`
header. A key is scoped to the authenticated user, registered device, HTTP
method, and canonical path. Repeating an identical request during the replay
window returns the original status and envelope. Reusing a key for a different
request returns `409 conflict`. Reusing a key after its retained replay
window returns `409 expired`. The client must create a new key after confirming
the current resource state. The replay window is at least 24 hours.

State-changing requests may use a resource version precondition where the
endpoint defines one. A stale precondition returns `409 conflict`; the
client must fetch current state and deliberately decide whether to retry. The
server never automatically merges membership, role, or encrypted-message
changes.

## Errors

| Status | Code                  | Meaning                                                             |
| ------ | --------------------- | ------------------------------------------------------------------- |
| `400`  | `invalid_request`     | Malformed JSON, query, or envelope.                                 |
| `401`  | `unauthenticated`     | A valid access token is missing, invalid, or expired.               |
| `403`  | `forbidden`           | The caller is known but lacks a non-sensitive capability.           |
| `404`  | `not_found`           | The resource or v1 route is unavailable to the caller.              |
| `406`  | `unsupported_version` | The requested path major version is unsupported.                    |
| `409`  | `conflict`            | A state precondition or idempotency key conflicts with the request. |
| `409`  | `duplicate`           | The request duplicates an existing operation.                       |
| `409`  | `stale`               | The request refers to state that has been superseded.               |
| `409`  | `expired`             | The request or idempotency key is outside its accepted window.      |
| `413`  | `too_large`           | The request body exceeds the endpoint limit.                        |
| `415`  | `invalid_request`     | The request content type is not accepted.                           |
| `422`  | `invalid_request`     | Well-formed fields violate endpoint constraints.                    |
| `429`  | `rate_limited`        | The caller must slow down; a `Retry-After` header is included.      |
| `503`  | `unavailable`         | The service is temporarily unavailable.                             |
| `500`  | `internal`            | The server could not safely complete the request.                   |

The golden response examples in
[`apps/cipher-server/src/tests/fixtures/http-v1`](../../apps/cipher-server/src/tests/fixtures/http-v1)
are serialized directly by the server contract tests. They cover a successful
response, authorization denial, idempotency conflict and expiry, validation,
and an unsupported API version.
