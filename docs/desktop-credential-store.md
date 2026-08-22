# Desktop credential store

Cipher's desktop core stores only refresh material and local-state wrapping keys in the operating system's credential store. The renderer has no command, handle, or fallback path for this data.

| Platform   | Native store                          | Access and persistence                                                                                                                                                                                             |
| ---------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| macOS 13+  | The current user's login Keychain     | Generic-password items remain in the user Keychain. The entry names contain a one-way scope fingerprint rather than an account reference.                                                                          |
| Windows 11 | The current user's Credential Manager | Generic credentials use local persistence so they remain on the current device and are not an enterprise-roaming credential. The entry names contain a one-way scope fingerprint rather than an account reference. |

The core accepts a stable, non-secret account or device reference only to derive the scope fingerprint. It never writes that reference into a Keychain service name or Credential Manager target. Each stored value has a versioned binary envelope containing its credential kind and exact length. Refresh material must be non-empty and at most 8 KiB; local-state wrapping keys must be exactly 32 bytes.

## Replacement, migration, corruption, and deletion

- `replace` validates a value, writes the current native record, then removes the matching legacy native record. If legacy cleanup fails, the current record remains and the operation reports the failure so it can be retried.
- `migrate` reads only the prior native schema for the same scope and credential kind. It validates the legacy value, writes the current record, and deletes the legacy item only after the write succeeds. A valid current item wins and causes any leftover legacy item to be removed.
- A malformed current record returns a `Corrupt` error. It is never treated as missing, read from a prior record, or copied to another store. A malformed legacy record is preserved for explicit recovery and also returns `Corrupt`.
- `delete` is idempotent and removes both current and legacy native items for one credential. `delete_scope` does the same for both credential kinds. Platform stores do not offer a transaction across independent records, so a deletion failure is returned and can be retried.

`SecretBytes` zeroizes its owned buffer when dropped and renders as `[redacted]` during debugging. Store errors use fixed categories and do not include native error text or protected values. There is deliberately no plaintext-file, browser-storage, WebView-storage, or in-memory production fallback.
