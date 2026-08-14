# Cipher

Secure realtime messaging for private conversations, roles, and permission-gated media.

The first milestone is a closed-alpha desktop messaging vertical slice. Message bodies are end-to-end encrypted; calls, general attachments, and Cipher Pay integration are outside the MVP.

Project docs and diagrams: <https://connorhunter.me/projects/cipher?viewer=docs#project-viewer>

## Releases

`package.json` is the Cipher release-version source. Run `bun run version:sync` after changing it to update the Cargo workspace and Tauri metadata, then run `bun run version:check` before committing.
