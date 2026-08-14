# Cipher

Secure realtime messaging for private conversations, roles, and permission-gated media.

The first milestone is a closed-alpha desktop messaging vertical slice. Message bodies are end-to-end encrypted; calls, general attachments, and Cipher Pay integration are outside the MVP.

Project docs and diagrams: <https://connorhunter.me/projects/cipher?viewer=docs#project-viewer>

## Development

Run `bun run verify` before committing or pushing. The local gate requires the pinned CodeQL CLI as the literal `codeql` executable on `PATH`; `bun run codeql:scan` scans JavaScript/TypeScript, Rust, and GitHub Actions and keeps its ignored database, cache, and SARIF output under `.codeql/`.

Branches use `<type>/<kebab-case-name>` and commits use `<type>[(scope)][!]: <imperative summary>`, where `<type>` is `feat`, `fix`, `chore`, `docs`, `test`, or `refactor`. Issue and pull request subjects use the same commit format; the issue forms supply the appropriate prefix.

Release branches use `release/<version>`, release-preparation commits use `chore(release): prepare <version>`, and release tags use `v<version>`.

Dependabot branches are accepted as `dependabot/*`. These rules apply to new work; existing Git history remains unchanged.

## Releases

`package.json` is the Cipher release-version source. Run `bun run version:sync` after changing it to update the Cargo workspace and Tauri metadata, then run `bun run version:check` before committing.
