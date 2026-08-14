# Contributing

Create a branch from `main`, keep the change focused, and open a pull request.

Run `bun run bootstrap` once after cloning. Run `bun run verify` before opening or updating a pull request. The required GitHub checks build the frontend, test the Rust workspace, and package Cipher on Apple Silicon macOS and Windows x64. GitHub also runs CodeQL.

Do not commit credentials, tokens, local environment files, plaintext user content, or generated build output. Open an architecture decision issue before changing a security boundary, privacy claim, data model, deployment shape, or supported platform.

Connor Hunter owns reviews and releases during the MVP. A passing build is not permission to publish a release.
