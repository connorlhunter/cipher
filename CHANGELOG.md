# Changelog

## [0.1.0-prealpha.10] - 2026-08-27

### Added

- Publish project coverage as structured JSON with a direct PDF download.
- Publish the canonical changelog as Markdown with a direct PDF download.

### Fixed

- Restore the enforced TypeScript coverage gate for release publication.

## [0.1.0-prealpha.9] - 2026-08-26

### Added

- Native Cognito sign-in, verification challenges, password recovery, session refresh, and authenticated HTTP and realtime requests.
- The desktop design system, account and utility routes, and device-removal settings.

### Changed

- Replaced separate web formatting, linting, and type checks with Vite+, including a 15-path complexity limit for application source.
- Streamlined native CI validation and stabilized the Tauri build target.

### Fixed

- Accepted additive SRP challenge metadata and refined authentication, welcome, overview, and sign-in feedback.
- Kept desktop bootstrap out of coverage and preserved native CI check contexts.

## [0.1.0-prealpha.8] - 2026-08-22

### Added

- Production network, gateway, deployment-control, ingress-verification, and private-media integrity foundations.

### Fixed

- Require and verify signed media payloads, immutable GitHub deployment subjects, scoped deployment image tags, and image builds across every workspace target.
- Authorize production smoke checks and verify signed fixture uploads with checksum sentinels.

### Changed

- Move published project documentation and diagrams to the artifact generator.

## [0.1.0-prealpha.7] - 2026-08-21

### Added

- Native desktop trust-boundary foundations: platform credential storage, Rust-owned HTTP and realtime client boundaries, bounded renderer data lifetime, and lifecycle cancellation with safe diagnostics.

### Changed

- Restrict the desktop webview to its fixed navigation, new-window, download, and IPC policies.
- Synchronize every internal versioned path dependency from the release version.

### Known limits

- Control, Network, and Runtime stacks, authenticated application flows, durable messaging, and live operational drills remain incomplete.

## [0.1.0-prealpha.6] - 2026-08-21

### Added

- Versioned protocol contracts, a bounded desktop IPC boundary, and pull-request issue-link enforcement.

## [0.1.0-prealpha.5] - 2026-08-21

### Fixed

- Publish the Rust coverage report alongside the overview and TypeScript reports, including its navigation control.

### Known limits

- Control, Network, and Runtime stacks, authenticated application flows, durable messaging, and live operational drills remain incomplete.

## [0.1.0-prealpha.4] - 2026-08-20

### Changed

- Simplified the published coverage update label to display the project publication date without a time or UTC suffix.

### Known limits

- Control, Network, and Runtime stacks, authenticated application flows, durable messaging, and live operational drills remain incomplete.

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
