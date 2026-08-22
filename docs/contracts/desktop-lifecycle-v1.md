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

## Diagnostics

`SafeDesktopDiagnostic` is the only lifecycle diagnostic export. It includes
only lifecycle state, transport state, renderer purge generation, active
operation count, cold-start count, and wake count. Native integrations must
not add free-form error strings or machine-specific state to this export.

The Tauri integration emits renderer purge notifications without a payload.
The renderer must treat each notification as an instruction to clear its
ephemeral view state, storage, clipboard cache, and notification preview cache.
