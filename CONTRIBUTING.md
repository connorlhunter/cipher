# Contributing

Create a branch from `main`, keep the change focused, and open a pull request.

Every pull request except one opened by Dependabot must link a Cipher issue in its description. Use a recognized phrase such as `Closes #<issue-number>` or `Related to #<issue-number>`.

Run `bun run bootstrap` once after cloning. Run `bun run verify` before opening or updating a pull request. The required GitHub checks build the frontend, test the Rust workspace, and package Cipher on Apple Silicon macOS and Windows x64. GitHub also runs CodeQL.

- `deny.toml` tells `cargo deny` to check the resolved Cargo graph for advisories, approved licenses, duplicate or wildcard versions, and trusted sources. A denied finding makes the audit fail; documented temporary advisory exceptions are reviewed with each Tauri update.

Do not commit credentials, tokens, local environment files, plaintext user content, or generated build output. Open an architecture decision issue before changing a security boundary, privacy claim, data model, deployment shape, or supported platform.

Connor Hunter owns reviews and releases during the MVP. A passing build is not permission to publish a release.
