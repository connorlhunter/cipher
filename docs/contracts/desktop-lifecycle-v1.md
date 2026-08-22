# Desktop lifecycle boundary v1

`cipher-desktop-lifecycle` keeps the desktop process in a small native state
machine. React does not decide whether to reconnect a transport, retain an
operation, or keep a prior screen visible after a safety event.

## Safety transitions

| Native event                                       | Required native action                                                                    |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Cold start                                         | Start in `active` with native transport ready.                                            |
| Single-instance launch                             | Focus the existing main window; ignore launch arguments.                                  |
| Sleep                                              | Cancel native work, pause transport, lock the native session, and purge renderer state.   |
| Wake                                               | Leave the application locked; network availability does not implicitly restore a session. |
| Offline                                            | Pause transport and retain only bounded reconnect state.                                  |
| Online                                             | Reconnect only from an active native session.                                             |
| Lock, logout, account change, or device revocation | Cancel work, pause transport, and increment the renderer purge generation.                |
| Shutdown                                           | Cancel work, pause transport, purge renderer state, finalize native cleanup, then stop.   |

The controller caps active operations at 32. Every safety transition has a
bounded action list and omits account identifiers, content, ciphertext,
credentials, key material, endpoint URLs, capability URLs, and screenshots.

The desktop process starts the controller only after the guarded main webview
exists. A second launch focuses that existing window and discards its command
line arguments. The native single-instance registration happens before other
desktop extensions. Shutdown begins on an exit request and records completion
only when the event loop exits. A resumed event never restores a locked session
or reconnects a transport without an explicit safe online transition.

Native authentication, messaging, reachability, and power-management code must
call `DesktopLifecycleService::handle_native_event` for lock, logout, account
change, device revocation, sleep, wake, offline, online, and interrupted work.
The webview has no command for these transitions.

## Diagnostics

`SafeDesktopDiagnostic` is the only lifecycle diagnostic export. It includes
only lifecycle state, transport state, renderer purge generation, active
operation count, cold-start count, and wake count. Native integrations must
not add free-form error strings or machine-specific state to this export.

`desktop_diagnostics` is allowlisted only for IPC protocol version 1. Its
fixture is at `contracts/ipc/v1/desktop-diagnostics.json`; version-zero clients
continue to use only the existing status command. The webview validates the
exact six-field diagnostic shape before React can inspect it.

The Tauri integration emits renderer purge notifications without a payload.
The renderer must treat each notification as an instruction to clear its
ephemeral view state, storage, clipboard cache, and notification preview cache.
Logout, device revocation, app lock (including sleep), and account change map
to the corresponding fixed event names reserved by the renderer lifetime
contract. No account reference, reason, event object, or diagnostic accompanies
the notification.
