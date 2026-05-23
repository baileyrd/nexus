# ADR 0010: No Plugin Signature Verification in M1

**Date:** 2026-04-11
**Status:** Accepted

## Context

PRD 02 §5 describes plugin signing (Ed25519 signatures, trusted authors).
Roadmap Section 3 cuts "plugin code review/approval workflows" from M1.
Trust levels (`core` vs `community`) stay in the manifest.

## Decision

M1 implements trust levels but not signature verification. Plugins declare
`trust_level = "core"` or `"community"` in their manifests; the kernel honors
the declaration without verifying it cryptographically. Community plugins
with HIGH-risk capabilities get an install-time CLI prompt.

Signing verification is a v0.2 feature, added if/when the personal-tool
becomes a multi-user concern.

## Alternatives considered

- Implement signing now: excess work for zero personal-tool benefit.
- Cut trust levels entirely: makes install-time prompts awkward.

## Consequences

- Trust is advisory, not enforced. Acceptable given single-user.
- The `ed25519-dalek` dep is deferred but the architecture leaves room
  for it to plug in later without breaking the manifest format.
