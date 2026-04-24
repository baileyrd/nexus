// shell/src/plugins/core/capabilityPrompt/consentLogic.ts
//
// WI-31 — Pure decision logic for the install-time consent flow.
//
// Given the current plugin manifest + the prior `granted_caps.json`
// snapshot, decide whether to prompt, auto-accept, or re-prompt. The
// semver rule: re-prompt only on major/minor bumps — patch bumps
// silently carry forward prior grants (Q4 decision). The kernel will
// re-verify on load (its version comparison is string-equal, so the
// shell's write of the new version string is what "upgrades" the grant
// to the patch release).
//
// Only HIGH-risk capabilities gate activation. Low/medium are auto-
// granted by the kernel at load time (see loader.rs build_capabilities);
// the shell still surfaces them in the banner so the user sees the full
// surface, but the UI path is non-blocking.

import type { Capability } from '@nexus/extension-api'
import { CAPABILITY_INFO, highestRisk } from '../../nexus/pluginsMgmt/capabilityInfo'
import { kernelStringsToCaps } from './capabilityMapping'

// ── Semver parsing ──────────────────────────────────────────────────────────
//
// We intentionally DON'T pull in a semver library. Plugin versions in
// this phase are simple "x.y.z" triples; pre-release / build-metadata
// suffixes are stripped. Anything that doesn't parse falls back to
// `null`, which the caller treats as "always re-prompt" (safer than
// accidentally silent-accepting an unparseable version).

export interface SemVer {
  major: number
  minor: number
  patch: number
}

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)/

export function parseSemVer(v: string | undefined | null): SemVer | null {
  if (!v) return null
  const m = SEMVER_RE.exec(v.trim())
  if (!m) return null
  return {
    major: Number(m[1]),
    minor: Number(m[2]),
    patch: Number(m[3]),
  }
}

/**
 * True when `current` and `prior` share the same `major.minor`. Patch
 * changes are considered "same family" and carry grants forward; a
 * bump of major or minor forces re-consent (Q4 decision).
 *
 * An unparseable version on EITHER side conservatively returns false
 * (forces prompt).
 */
export function isPatchOnlyBump(
  current: string | undefined,
  prior: string | undefined,
): boolean {
  const a = parseSemVer(current)
  const b = parseSemVer(prior)
  if (!a || !b) return false
  return a.major === b.major && a.minor === b.minor
}

// ── Prior grant snapshot ────────────────────────────────────────────────────

/**
 * Shape the Rust `get_plugin_granted_capabilities` command returns, per
 * plugin. `version: ""` means "no prior grant on disk" — the first-run
 * path.
 */
export interface PriorGrant {
  version: string
  /** Kernel-format dotted strings (as written to granted_caps.json). */
  capabilities: string[]
}

/** PriorGrant with the capability list already mapped back to PascalCase. */
export interface ParsedPriorGrant {
  version: string
  capabilities: Capability[]
}

export function parsePriorGrant(raw: PriorGrant | undefined): ParsedPriorGrant {
  if (!raw || !raw.version) {
    return { version: '', capabilities: [] }
  }
  return {
    version: raw.version,
    capabilities: kernelStringsToCaps(raw.capabilities),
  }
}

// ── Consent decision ────────────────────────────────────────────────────────

export type ConsentDecision =
  /** No declared capabilities — nothing to prompt about. */
  | { kind: 'auto-accept'; reason: 'no-capabilities' }
  /** Prior grant covers the exact requested set at same major.minor. */
  | { kind: 'auto-accept'; reason: 'patch-bump' }
  /** No high-risk caps declared — show a non-blocking banner. */
  | { kind: 'banner'; caps: Capability[] }
  /**
   * High-risk present OR prior version differs (major/minor) — show the
   * blocking modal. `previouslyGranted` is the caps the user OK'd on the
   * prior install (PascalCase); UI highlights newly-added caps against
   * this set. `reason` drives the header copy ("First install" vs "This
   * plugin updated — please review new capabilities").
   */
  | {
      kind: 'modal'
      caps: Capability[]
      previouslyGranted: Capability[]
      reason: 'first-install' | 'version-bump' | 'capability-change'
    }

export interface ConsentInput {
  declared: Capability[] | null
  currentVersion: string
  prior: ParsedPriorGrant
}

/**
 * Given a plugin's declared capabilities + its prior grant snapshot,
 * decide what UI (if any) to show. Pure function — no I/O, no React.
 */
export function decideConsent(input: ConsentInput): ConsentDecision {
  const declared = input.declared
  if (declared === null || declared.length === 0) {
    return { kind: 'auto-accept', reason: 'no-capabilities' }
  }

  const risk = highestRisk(declared)
  const hasHighRisk = risk === 'high'

  const priorHasGrant = input.prior.version !== ''
  const patchBump =
    priorHasGrant && isPatchOnlyBump(input.currentVersion, input.prior.version)

  // Same major.minor AND declared set subset-or-equal to prior grants
  // (plus auto-granted low/medium that the prior grant never stored) →
  // silent carry-forward. We treat high-risk as the gating signal: if
  // every declared high-risk cap was previously granted, no prompt.
  if (patchBump) {
    const declaredHighRisk = declared.filter(
      (c) => CAPABILITY_INFO[c]?.risk === 'high',
    )
    const priorSet = new Set(input.prior.capabilities)
    const allPreviouslyGranted = declaredHighRisk.every((c) => priorSet.has(c))
    if (allPreviouslyGranted) {
      return { kind: 'auto-accept', reason: 'patch-bump' }
    }
    // Patch bump but new high-risk cap appeared — still re-prompt.
    return {
      kind: 'modal',
      caps: declared,
      previouslyGranted: input.prior.capabilities,
      reason: 'capability-change',
    }
  }

  if (!hasHighRisk) {
    // Only low/medium caps — non-blocking banner. No prior grant needed
    // because the kernel auto-grants these from the manifest.
    return { kind: 'banner', caps: declared }
  }

  return {
    kind: 'modal',
    caps: declared,
    previouslyGranted: input.prior.capabilities,
    reason: priorHasGrant ? 'version-bump' : 'first-install',
  }
}
