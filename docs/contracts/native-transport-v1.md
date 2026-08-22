# Native transport boundary v1

Cipher's React renderer does not authenticate HTTP requests, open WebSocket
connections, retain reconnect state, or receive access tokens. Those operations
are owned by Rust through the `cipher-native-transport` crate.

## Native ownership

| Concern       | Native boundary                                                                                                                                                                                 |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Access tokens | `SessionAuthenticator` returns a non-serializable `AccessToken` to Rust only.                                                                                                                   |
| HTTP          | `NativeHttpClient` adds authentication immediately before a `NativeHttpTransport` implementation sends a bounded v1 request.                                                                    |
| Realtime      | `NativeRealtimeClient` opens the versioned `/v1/realtime` connection, completes the hello/welcome handshake, and validates every server frame.                                                  |
| Reconnect     | Native state retains only the bounded subscription set, opaque cursor, attempt count, and stable failure category.                                                                              |
| Cancellation  | Every authenticator, HTTP transport, connection, and receive operation receives the shared `OperationCancellation` handle.                                                                      |
| Diagnostics   | `NativeTransportDiagnostic` contains only lifecycle state, attempt count, and error category. It omits endpoints, capability URLs, credentials, ciphertext, plaintext, and account identifiers. |

Production origins must use `https` and `wss`. Native clients permit `http` and
`ws` only for explicit loopback development origins. Requests are confined to
relative `/v1` paths and HTTP bodies, responses, and realtime frames are capped
by the shared protocol limits.

## Renderer contract

Tauri commands may return only bounded display view models defined by the IPC
contract. A Rust feature using this transport may trigger an explicit renderer
state update after native validation or decryption, but it must never serialize
an `AccessToken`, authenticated request, native response body, connection,
cursor, or transport diagnostic with endpoint material across IPC.

The platform credential store supplies persisted refresh material and local
wrapping keys to native authentication code. It is not a WebView storage API;
the renderer cannot read or replace entries directly.
