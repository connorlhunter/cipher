# Closed-alpha scope

- Status: Proposed
- Date: 2026-08-14

## Release

The first release is a small, invite-only desktop messaging alpha.

It includes:

- sign-in and account setup;
- direct and small-group conversations;
- encrypted text messages and message history;
- one active device per account;
- basic membership and permission checks;
- private avatars and group/server icons.

It does not include calls, general file attachments, multi-device recovery,
multi-gateway hosting, or Cipher Pay.

## Decisions still needed

- Which desktop platforms and minimum versions are supported?
- Are servers, channels, and basic roles part of this first test?
- Are avatars and icons enough for the first release?
- What should happen when a user reinstalls or loses a device?
- How many testers and test machines are in the first group?

The suggested starting point is macOS first, one device per account, icons and
avatars only, and a small invite-only group.

## Acceptance

The alpha is ready when invited testers can sign in, enroll a device, join a
permitted conversation, send and reload encrypted messages, reconnect after a
network interruption, and be removed from a conversation. Logout and revocation
must clear local credentials and displayed message data.

Uploads must be checked and quarantined before an avatar or icon is published.

## Release safety

The release needs passing tests, a security review, signed builds, a staging
smoke test, and a documented way to stop enrollment and return to the last good
build. The first backend deployment uses one task, with a brief outage during a
replacement if necessary.

This document becomes final after the decisions above are recorded. Issue #2
and its scope sub-issues remain open until then.
