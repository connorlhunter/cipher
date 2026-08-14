# Changelog

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
