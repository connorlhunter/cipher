# Cipher

Secure realtime messaging for private conversations, roles, and permission-gated media.

The first milestone is a closed-alpha desktop messaging vertical slice. Message bodies are end-to-end encrypted; calls, general attachments, and Cipher Pay integration are outside the MVP.

Project docs and diagrams: <https://connorhunter.me/projects/cipher?viewer=docs#project-viewer>

## Development

Run `bun run verify` before committing or pushing. The local gate requires the pinned CodeQL CLI as the literal `codeql` executable on `PATH`; `bun run codeql:scan` scans JavaScript/TypeScript, Rust, and GitHub Actions and keeps its ignored database, cache, and SARIF output under `.codeql/`.

Branches use `<type>/<kebab-case-name>` and commits use `<type>[(scope)][!]: <imperative summary>`, where `<type>` is `feat`, `fix`, `chore`, `docs`, `test`, or `refactor`. Issue and pull request subjects use the same commit format; the issue forms supply the appropriate prefix.

Release branches use `release/<version>`, release-preparation commits use `chore(release): prepare <version>`, and release tags use `v<version>`.

Dependabot branches are accepted as `dependabot/*`. These rules apply to new work; existing Git history remains unchanged.

## Server container

- Build the server image with `bun run server:image`. The script reads the pinned Rust version from `rust-toolchain.toml`; the multi-stage build produces only the release `cipher-server` binary and runs it as an unprivileged user.
- The image defaults `CIPHER_SERVER_BIND` to `0.0.0.0:3000`. Local runs use the loopback value in `.env.example`; ECS task definitions must set `CIPHER_SERVER_BIND=0.0.0.0:3000`.
- Supply the remaining required `CIPHER_*` settings at runtime from the deployment configuration. Do not bake `.env` files, production endpoints, AWS identifiers, or secrets into the image; deployed resource values come from the production stack outputs.

## Releases

`package.json` is the Cipher release-version source. Run `bun run version:sync` after changing it to update the Cargo workspace, lockfile, internal versioned path dependencies, and Tauri metadata, then run `bun run version:check` before committing.

## Coverage publication

`bun run coverage:publish` runs the TypeScript and Rust coverage suites, enforcing at least 95% line and function coverage for each independently. It creates one UTC publication timestamp, stamps the overview plus both surface pages, renders matching PDF downloads, and syncs only `projects/cipher/coverage/`. Rust coverage uses the pinned `cargo-llvm-cov` tool and Rust's `llvm-tools-preview` component. Set `ARTIFACTS_BUCKET` for the live artifact bucket, `SOURCE_ARTIFACTS_BUCKET` for a durable copy, and `ARTIFACTS_CLOUDFRONT_DISTRIBUTION_ID` when the published path needs invalidation.

## State-stack configuration

After `CipherProductionState` is deployed, use its outputs to replace these ignored `.env` placeholders:

- `CognitoUserPoolId` → `CIPHER_COGNITO_USER_POOL_ID`
- `CognitoUserPoolClientId` → `CIPHER_COGNITO_CLIENT_ID`
- `UsersTableName` → `CIPHER_USERS_TABLE`
- `ConversationsTableName` → `CIPHER_CONVERSATIONS_TABLE`
- `MessagesTableName` → `CIPHER_MESSAGES_TABLE`
- `MediaTableName` → `CIPHER_MEDIA_TABLE`
- `MediaBucketName` → `CIPHER_MEDIA_BUCKET`

`MediaPendingPrefix`, `MediaReadyPrefix`, and `MediaFixturePrefix` describe the S3 key roots enforced by the state stack. Run `bun --env-file=.env run live:fixtures` to validate the deployed Cognito, DynamoDB, and S3 settings with a fresh UUID-scoped fixture run. It refuses an incorrect production account or State output, requires DynamoDB and S3 ownership markers before cleanup, and verifies that unmarked same-prefix sentinels remain untouched.

The state policy enforces TLS, SSE-S3, and those key roots. A later signed upload command must bind the SHA-256 checksum header, then verify it with `HeadObject`; S3 does not expose that checksum header as a bucket-policy condition key.

`bun run infra:readiness` requires exactly one Cognito pool, four DynamoDB tables, one S3 bucket, and the seven configuration outputs above in the synthesized state stack. It retains presence-only checks for the control, network, and runtime stacks while those foundations are completed.

State resources use stable `cipher-production-*` names. The media bucket includes the account and region because S3 bucket names are global; the stack outputs remain the source for runtime configuration.
