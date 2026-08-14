# ADR-0001: Closed-alpha release scope

- Status: Proposed
- Date: 2026-08-14
- Owners: Cipher maintainers

## Decision

The first release is a closed-alpha desktop messaging client. It is a learning
release for the encrypted messaging path, not a general-purpose social client
or a payments product. We will not start implementation of a feature until its
scope and acceptance test are covered by this document or a later ADR.

The recommendations below are the smallest coherent release built around the
current Cipher design. Items marked **confirm** need an explicit product
decision before this ADR can become Accepted.

## Recommended alpha baseline

### In the release

- Invite-only account enrollment through Cognito-hosted authentication.
- One active device per account. A reinstall is a new device enrollment.
- A Rust-owned native core for authentication, key custody, encryption,
  persistence, synchronization, and network access.
- A React renderer that receives only bounded, ephemeral display view models
  over typed IPC; it does not call backend services directly.
- Direct and small group conversations with encrypted message bodies, ordered
  history, offline outbox, reconnect/resume, and member removal.
- The existing server/channel/membership primitives and default-deny role
  checks, kept to the minimum needed to create, join, and moderate a test
  conversation. **Confirm whether these primitives are required for the first
  cohort or should move behind a later milestone.**
- A single-device OpenMLS profile: device identity, key-package publication,
  Welcome/Commit processing, removal, encrypted local state, and replay and
  rollback tests.
- Private avatar, server-icon, and group-icon media only. These objects are
  server-readable, quarantined and re-encoded before publication, and stored
  with SSE-KMS. Message attachments, thumbnails, and arbitrary file uploads
  are not part of this release.
- One modular backend task exposing HTTP and WSS, with a stop-before-start
  deployment during the alpha. There is no multi-gateway routing or shared
  presence bus in this release.

### Outside the release

- Calls or WebRTC media.
- General attachments, thumbnails, and server-generated previews.
- Multi-device sessions, device recovery, or account-level key recovery.
- Multiple gateway tasks, seamless rolling gateway deployments, or global
  presence fan-out.
- Cipher Pay, payment providers, wallets, signing, payment tables, payment
  events, or payment UI. A future integration may use a versioned backend
  boundary keyed by the opaque Cipher user id; no payment dependency is added
  now.
- Rich text, message search, reactions, edits, read receipts, typing
  indicators, and other interaction polish unless separately promoted.

## Product choices to confirm

These are the inputs required to replace `Proposed` with `Accepted`:

1. **Platform:** recommended macOS-first for the alpha. Confirm the minimum
   macOS version and whether both Apple Silicon and Intel builds are required.
   Windows should remain deferred unless there is a firm test and signing plan.
2. **Conversation shape:** confirm whether servers/channels/basic roles are in
   the first cohort, or whether the alpha is direct and group conversations
   only.
3. **Media privacy:** recommended avatar/icon-only media with server-readable
   objects. If attachments are a launch promise, choose client encryption and
   define key, size, type, quarantine, and thumbnail rules before implementation.
4. **Recovery:** recommended invite-only enrollment and one active device, with
   no server-side recovery of message keys. Confirm the user-facing behavior
   after reinstall, lost device, logout, and account revocation.
5. **Release size:** name the initial cohort and test-device count. This controls
   the amount of staging capacity, support, telemetry, and rollback preparation.

## Enrollment and recovery contract

- A maintainer issues an invite; Cognito performs identity verification and
  sign-in; the application maps the stable Cognito `sub` to an app-owned
  Cipher user id.
- The first device creates its device record and publishes a key package.
- A replacement install is a new device. A current member must deliver a
  device-addressed Welcome/rejoin message; without a valid sponsor, old
  ciphertext remains unreadable.
- Logout and revocation clear native credentials, renderer caches, and pending
  plaintext. Application sessions are revoked independently of the short-lived
  access-token expiry window.
- No “forgotten password restores message history” promise is made for this
  alpha.

## Acceptance journey

The release candidate must pass this journey on every supported target:

1. An invited tester signs in, completes device enrollment, and sees only the
   conversations they are authorized to see.
2. Two enrolled testers create or join a permitted conversation, exchange
   encrypted text, restart the app, and recover ordered history.
3. A member is removed; subsequent sends, history reads, and reconnects are
   denied for that member.
4. The network is interrupted; the client resumes with bounded queues and no
   duplicate or silently missing messages.
5. A tester uploads an allowed avatar/icon; the service verifies, quarantines,
   re-encodes, and serves only the canonical object.
6. Logout, account revocation, and device lock clear local display state and
   prevent new authenticated work.

## Rollback and exit criteria

The alpha does not proceed if any of these are false:

- The threat model, OpenMLS profile, token handling, media boundary, and
  default-deny authorization have received review and have no open launch-
  blocking finding.
- Format, lint, type, unit, integration, security, and staging smoke checks
  pass on each supported target; signed installers can be reproduced.
- A staging deployment can be stopped before starting a replacement task,
  preserving history and documenting the expected brief outage.
- Operators can disable enrollment, stop the cohort, revoke a device, and
  restore the last known-good backend/data version without deleting ciphertext.

Rollback means stopping enrollment and the affected release, revoking exposed
devices or sessions, and returning to the last known-good build. A schema or
key migration is not shipped until its rollback or forward-fix path has been
tested; a blind database downgrade is not an option.

## Work unlocked after acceptance

Once the choices above are recorded, implementation proceeds in this order:

1. Freeze this ADR and product copy.
2. Write the threat model and security invariants.
3. Scaffold the Rust workspace, desktop shell, typed IPC, CI, and release
   profiles.
4. Prove native OAuth/PKCE callback handling and credential storage.
5. Implement identity, device, conversation, membership, and authorization
   contracts.
6. Implement the scoped OpenMLS and encrypted message history path.
7. Implement the single-task realtime gateway and recovery semantics.
8. Implement icon/avatar media and its quarantine/finalization path.
9. Run the end-to-end desktop journey in staging, then package a private alpha.

Issue #2 remains open until this document is Accepted and the four scope
sub-issues (#33–#36) have their decisions and acceptance checks recorded.
