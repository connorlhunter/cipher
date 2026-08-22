# Renderer data lifetime

The Cipher webview is a display surface, not a data store. Rust owns
authentication, transport, encrypted records, decryption, protocol state, and
durable retention. The renderer may retain one bounded conversation view for
the currently visible screen only.

## Allowed view data

`RendererConversationView` accepts only these fields:

- a canonical `usr_` account ID and `cnv_` conversation ID;
- a short conversation title;
- at most 50 messages, each with a canonical `msg_` ID, short author label,
  short plaintext preview, and canonical UTC timestamp.

Every object is copied, frozen, and validated before it enters the cache.
Unknown fields are rejected. Tokens, credentials, private keys, serialized MLS
state, ciphertext, transport responses, and arbitrary message history have no
place in this view shape.

The cache holds one screen for at most 60 seconds. Reading an expired view or a
view after a backwards clock change removes it immediately. Replacing a view
for a different account clears the former account before the new one is held.

## Retention and disclosure policy

The renderer does not read from persistent browser storage. On startup,
logout, device revocation, app lock, account change, and shell teardown it
clears local storage, session storage, enumerated IndexedDB databases, and
CacheStorage without reading their values. It also clears the clipboard on the
same transitions.

Copying is an explicit UI action and accepts only a validated bounded preview;
there is no automatic clipboard write. Notifications use the fixed title
`Cipher` and body `You have a new message.` so neither sender nor message
content reaches an operating-system notification history. Renderer failures
are discarded locally; a later diagnostics exporter may receive only the fixed
`renderer_failure` code and never an error object, component stack, or display
model.

## Lifecycle handoff

The renderer reserves these no-payload native event names:

| Transition        | Event                                    |
| ----------------- | ---------------------------------------- |
| Logout            | `cipher://renderer-data/logout`          |
| Device revocation | `cipher://renderer-data/device-revoked`  |
| App lock          | `cipher://renderer-data/app-locked`      |
| Account change    | `cipher://renderer-data/account-changed` |

Each event maps directly to a full renderer purge. The current desktop IPC
foundation has only the `desktop_status` command and does not yet emit these
events. The listener is intentionally ready now, while the native lifecycle
producer remains compatible work for the desktop lifecycle boundary. Until
that producer is present, startup, account replacement, and shell teardown
still clear the renderer state.

Some older WebView implementations cannot enumerate IndexedDB databases. The
renderer creates none, never reads one, and treats enumeration as best-effort
cleanup for pre-existing data. Selected desktop WebViews should be verified
when native lifecycle emission is introduced.
