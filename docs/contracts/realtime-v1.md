# Realtime protocol v1

Cipher realtime traffic uses UTF-8 JSON text frames over the authenticated
`wss://cipher.connorhunter.me/v1/realtime` connection. Authentication is
established by the WebSocket upgrade; credentials, recovery material, private
keys, MLS state, message plaintext, and media capability URLs are never valid
realtime-frame fields.

The server and native client enforce the frame rules in
`cipher-realtime-protocol`. The checked-in fixtures in `contracts/realtime/`
are the compatibility baseline, including the retained v0 handshake.

## Version negotiation

The first client frame **must** be `hello`. `supportedVersions` is a
non-empty, descending, duplicate-free list with at most the current and
previous wire versions. The server selects the highest mutually supported
version and returns `welcome`. Every subsequent client and server frame
carries that selected `protocolVersion`.

During the v1 rollout, versions 1 and 0 are accepted. New frame kinds and
fields are introduced only in the current version; a client that offers no
supported version receives the fatal `unsupported_version` error frame and
the connection closes. Unknown frame kinds or fields are rejected rather than
silently ignored.

```json
{
  "type": "hello",
  "supportedVersions": [1, 0],
  "resumeCursor": "cur_AQIDBA",
  "lastAcknowledgedServerSequence": 42
}
```

```json
{
  "type": "welcome",
  "protocolVersion": 1,
  "sessionId": "ses_0198b1dc-0000-7000-8000-000000000001",
  "nextServerSequence": 43,
  "heartbeatIntervalMs": 30000,
  "resumed": true
}
```

## Client frames

After `hello`, client frames are strictly ordered by a non-zero `sequence`.
The server rejects a repeated sequence as `duplicate` and a lower sequence as
`stale`. Commands also carry a stable `idempotencyKey`;
repeating that key never re-executes the command, even if it arrives with a
different sequence.

Once a post-handshake frame has a selected version and a new sequence, that
sequence is consumed even when its command or acknowledgement receives a typed
error. Retries therefore use a new sequence and the same idempotency key.

`command` is intentionally a narrow subscription control plane for v1:

```json
{
  "type": "command",
  "protocolVersion": 1,
  "sequence": 1,
  "idempotencyKey": "idem_0198b1dc-0000-7000-8000-000000000002",
  "command": {
    "type": "subscribe",
    "conversationIds": ["cnv_0198b1dc-0000-7000-8000-000000000003"]
  }
}
```

`unsubscribe` has the same `conversationIds` shape. The list is non-empty
and contains at most 100 distinct conversation IDs. A message body, encrypted
payload, or arbitrary command payload is not part of this control contract.

Clients acknowledge delivered server events with `ack`:

```json
{
  "type": "ack",
  "protocolVersion": 1,
  "sequence": 2,
  "acknowledgedServerSequence": 43
}
```

An acknowledgement may not move backwards, repeat, or exceed the highest
server sequence the connection has received. `heartbeat` has the same
`protocolVersion` and client `sequence` fields plus a 1–64-character ASCII
nonce; the peer echoes the nonce.

## Server frames

After `welcome`, server events are ordered by their non-zero `sequence`. Each
event has an opaque resume `cursor`, a typed `eventId`, and metadata only:

```json
{
  "type": "event",
  "protocolVersion": 1,
  "sequence": 43,
  "eventId": "evt_0198b1dc-0000-7000-8000-000000000004",
  "cursor": "cur_AQIDBQ",
  "event": {
    "type": "message_available",
    "conversationId": "cnv_0198b1dc-0000-7000-8000-000000000003",
    "messageId": "msg_0198b1dc-0000-7000-8000-000000000005"
  }
}
```

The v1 event catalog is `message_available`, `conversation_changed`, and
`device_revoked`. The client fetches any encrypted record through the versioned
HTTP contract; it never accepts message plaintext from a realtime event.

The server confirms a command with `ack`, which repeats the accepted client
sequence and idempotency key. It may return a typed `error` frame instead:

```json
{
  "type": "error",
  "protocolVersion": 1,
  "error": {
    "code": "duplicate",
    "message": "The command was already accepted.",
    "retryable": false
  },
  "fatal": false,
  "idempotencyKey": "idem_0198b1dc-0000-7000-8000-000000000002"
}
```

Error messages are fixed, safe diagnostics of at most 160 characters. They
must not expose account existence, authorization details, tokens, keys,
plaintext, ciphertext, or server internals. Fatal errors are followed by a
close. The server also emits and expects heartbeats every 30 seconds; after
three missed intervals it closes the connection and the native client begins
its bounded reconnect flow.

## Resume and ordering

The client supplies the last committed opaque cursor and its last acknowledged
server sequence only in `hello`. If the cursor is retained and authorized, the
server sets `resumed` to `true` and replays strictly increasing events after
that cursor. If it is expired, unavailable, or no longer authorized, the
server sets `resumed` to `false`; the client reconciles through HTTP before
using fresh realtime events. A cursor is opaque: clients must not construct,
compare, or infer data from it.

At-least-once delivery is expected around reconnects. Clients de-duplicate
events by `eventId`, preserve sequence order, and acknowledge only committed
events. A server never treats a transport write as delivery until it receives
the corresponding acknowledgement.

## Limits and compatibility

- A JSON text frame is at most 65,536 UTF-8 bytes. Binary frames are rejected.
- `supportedVersions` contains at most two entries, and supported versions are
  the current version plus exactly one previous version.
- Client and server sequences are unsigned 64-bit integers greater than zero.
- Subscription commands contain 1–100 unique IDs; heartbeat nonces are 1–64
  printable ASCII characters.
- An unknown version, frame kind, command, event, or field is rejected with a
  typed error. The wire error codes reuse the shared contract catalog:
  `invalid_request`, `duplicate`, `stale`, `unsupported_version`, and
  `too_large` cover protocol validation. A later protocol version must not
  depend on a v1 client ignoring new semantics.

The validation suite covers accepted, malformed, duplicate, stale, oversized,
and unsupported frames so a compatibility change must update both this
document and its golden fixtures deliberately.
