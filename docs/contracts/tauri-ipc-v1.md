# Tauri IPC contract v1

The React webview may invoke only the commands registered by the Rust core.
For this foundation release, that allowlist contains `desktop_status` only.
Calls carry a `protocolVersion`; versions 0 and 1 are accepted during the v1
rollout, while new commands require v1.

`desktop_status` returns a bounded display view:

```json
{ "message": "Desktop core is ready." }
```

The message is limited to 160 Unicode code units. Native responses and events
must never contain credentials, access or refresh tokens, private keys, MLS
state, recovery phrases, or unbounded plaintext payloads.

Native failures use `{ "code", "message" }`, where `code` is one of
`cancelled`, `invalid_request`, `unsupported_version`, or `unavailable`.
Long-running commands must support cancellation through the typed `cancelled`
error, rather than exposing native handles to the webview. Rust may emit only
the `ready` and `shutdown` lifecycle notifications; each includes the protocol
version and no account or secret data.

The checked-in v0 and v1 fixtures are the compatibility baseline for Rust and
TypeScript tests.
