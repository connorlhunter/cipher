# Changelog

## [0.1.0-prealpha.3] - 2026-08-20

### Added

- Publish downloadable overview and TypeScript coverage PDFs alongside their HTML reports.

### Changed

- Stamp every Cipher coverage page with one project-owned UTC publication timestamp.
- Render both PDFs from the exact stamped HTML before syncing only Cipher's coverage prefix.

### Known limits

- Control, Network, and Runtime stacks, authenticated application flows, durable messaging, and live operational drills remain incomplete.

## [0.1.0-prealpha.2] - 2026-08-18

### Added

- Production State-stack foundations for invite-only Cognito, four DynamoDB tables, and private encrypted media storage.
- Exact State-stack readiness checks, stable resource names, and a non-root Rust server container foundation.
- Dedicated Rust test modules and a published TypeScript coverage report.

### Changed

- Local readiness now validates the State stack's required resources and runtime outputs before deployment steps continue.
- Project documentation and diagrams distinguish deployed State foundations from the remaining Control, Network, Runtime, and application work.

### Known limits

- Control, Network, and Runtime stacks, authenticated application flows, durable messaging, and live operational drills remain incomplete.

## [0.1.0-prealpha.1] - 2026-08-14

### Added

- A Tauri desktop shell with a React renderer and Rust workspace.
- An Axum service foundation with validated environment configuration and health and realtime route shells.
- Protected state, control, network, and runtime CDK stack boundaries with guarded readiness, pause, resume, and full-destruction commands.
- Cross-platform verification, dependency policy, code documentation, synchronized release versions, local CodeQL scans, and semantic change naming.

### Changed

- Runtime configuration now comes from environment variables.
- `package.json` is the release-version source for Cargo workspace metadata, the lockfile, internal versioned path dependencies, and Tauri.

### Known limits

- Production Cognito, DynamoDB, S3, ECR, VPC, ALB, ECS, DNS, and TLS resources are not provisioned.
- The realtime endpoint is an unauthenticated route shell without durable messaging.
- Live readiness, fixture cleanup, pause and resume, and full-destruction drills are incomplete.
- There is no supported production deployment or signed installer.
