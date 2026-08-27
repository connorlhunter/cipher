# Cipher

Secure realtime messaging for private conversations, roles, and permission-gated media.

The first milestone is a closed-alpha desktop messaging vertical slice. Message bodies are end-to-end encrypted; calls, general attachments, and Cipher Pay integration are outside the MVP.

Project docs and diagrams: <https://connorhunter.me/projects/cipher/docs>

## Development

Run `bun run verify` before committing or pushing. Web formatting, linting, type checks, builds, and local development use Vite+ through the `bun run` commands. Use `bun run check:frontend` for the web-only gate or `bun run check` to include Rust and infrastructure checks. The local gate requires the pinned CodeQL CLI as the literal `codeql` executable on `PATH`; `bun run codeql:scan` scans JavaScript/TypeScript, Rust, and GitHub Actions and keeps its ignored database, cache, and SARIF output under `.codeql/`.

Branches use `<type>/<kebab-case-name>` and commits use `<type>[(scope)][!]: <imperative summary>`, where `<type>` is `feat`, `fix`, `chore`, `docs`, `test`, or `refactor`. Issue and pull request subjects use the same commit format; the issue forms supply the appropriate prefix.

Release branches use `release/<version>`, release-preparation commits use `chore(release): prepare <version>`, and release tags use `v<version>`.

Dependabot branches are accepted as `dependabot/*`. These rules apply to new work; existing Git history remains unchanged.

## Server container

- Build the server image with `bun run server:image`. The script reads the pinned Rust version from `rust-toolchain.toml`; the multi-stage build produces only the release `cipher-server` binary and runs it as an unprivileged user.
- The image defaults `CIPHER_SERVER_BIND` to `0.0.0.0:3000`. Local runs use the loopback value in `.env.example`; ECS task definitions must set `CIPHER_SERVER_BIND=0.0.0.0:3000`.
- Supply the remaining required `CIPHER_*` settings at runtime from the deployment configuration. Do not bake `.env` files, production endpoints, AWS identifiers, or secrets into the image; deployed resource values come from the production stack outputs.

## Releases

`package.json` is the Cipher release-version source. Run `bun run version:sync` after changing it to update the Cargo workspace, lockfile, internal versioned path dependencies, and Tauri metadata, then run `bun run version:check` before committing.

`bun run release:publish` validates the release metadata and changelog before publishing coverage JSON/PDF and the canonical `CHANGELOG.md` with its PDF.

## Coverage publication

`bun run coverage:publish` runs the TypeScript and Rust coverage suites, enforcing at least 95% line and function coverage for each independently. It writes one timestamped JSON artifact and one PDF to `projects/cipher/coverage/`; the portfolio renders the JSON itself. Rust coverage uses the pinned `cargo-llvm-cov` tool and Rust's `llvm-tools-preview` component. Set `ARTIFACTS_BUCKET` for the live artifact bucket, `SOURCE_ARTIFACTS_BUCKET` for a durable copy, and `ARTIFACTS_CLOUDFRONT_DISTRIBUTION_ID` when the published path needs invalidation.

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

The state policy enforces TLS, a signed SHA-256 payload, SSE-S3, and those key roots; it grants no public access, so writes also require an authorized caller. The live media fixture binds an S3 SHA-256 checksum on its exact upload and verifies the checksum, content length, and SSE-S3 metadata with `HeadObject`. S3 does not expose the additional-checksum header as a bucket-policy condition key, so application uploads must keep both the SigV4 payload signature and the explicit checksum boundary.

`bun run infra:readiness` requires exactly one Cognito pool, four DynamoDB tables, one S3 bucket, and the seven configuration outputs above in the synthesized state stack. It retains presence-only checks for the control, network, and runtime stacks while those foundations are completed.

State resources use stable `cipher-production-*` names. The media bucket includes the account and region because S3 bucket names are global; the stack outputs remain the source for runtime configuration.

## Production network

`CipherProductionNetwork` is the one two-AZ, public-subnet production VPC. It
has no NAT gateway or parallel development, preview, or staging stack. Its
security groups reserve public TCP 443 for the later load balancer and allow
the service port only from that boundary. The published
[Cipher documentation](https://connorhunter.me/projects/cipher/docs)
covers its CIDR, cost tags, deletion model, and change-control procedure.

Run `bun --env-file=.env run infra:readiness` before a production change.
`infra:resume` checks the configured account, reruns that preflight, requires
the exact retained immutable image tag, and requires an interactive, explicit
CDK approval for every change; do not apply network changes through the AWS
console.

## Production runtime

`CipherProductionControl` retains the immutable server image repository, and
`CipherProductionRuntime` runs one TLS load balancer plus one Fargate backend
and realtime gateway task. Before the first deployment, use the guarded
certificate and bootstrap procedure in the published
[Cipher documentation](https://connorhunter.me/projects/cipher/docs),
then use the protected GitHub workflow for every reviewed production change.
That documentation records the exact image, health-check, drain, and recovery
sequence.
