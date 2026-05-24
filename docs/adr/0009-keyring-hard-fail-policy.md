# ADR 0009: Keyring Hard-Fail Policy

**Date:** 2026-04-11
**Status:** Accepted

## Context

`keyring-rs` may fail to access the OS keychain (no D-Bus on Linux, locked
macOS Keychain, etc.). We need a policy for what happens when credentials
can't be stored or retrieved.

## Decision

Hard fail. Nexus refuses to start if the keyring is unavailable. The error
message points to platform-specific setup docs. `NEXUS_NO_KEYRING=1` is an
escape hatch that disables credential operations entirely (not a fallback).

## Alternatives considered

- Encrypted on-disk fallback with passphrase: adds a UX surface (prompts)
  for the 99% case where the keychain works.
- Plaintext fallback: bad — secrets on disk.

## Consequences

- Personal-tool framing assumes a daily-driver machine where the keychain
  works; this is the right default.
- Users running Nexus in unusual environments (remote SSH, container) must
  set up keyring access or use `NEXUS_NO_KEYRING=1`.
- Not yet enforced in PRD 01 (keyring is `nexus-security` concern).
