// shell/src/plugins/core/capabilityPrompt/applyCapabilityChange.ts
//
// BL-096 follow-up — single entry point for "the user's chosen
// capability set for this plugin just changed". Persists the new set
// to disk via the existing `set_plugin_granted_capabilities` Tauri
// command AND, for any cap that was removed from the prior grant,
// calls the new `revoke_plugin_capability` verb so the running
// plugin loses access immediately.
//
// Pre-BL-096-follow-up the persisted-write was the only side effect
// of the consent UI — the plugin's wired capability set wouldn't
// drop the now-revoked cap until the next boot. This module closes
// that gap.

import type { Capability } from '@nexus/extension-api'
import { capsToKernelStrings } from './capabilityMapping'

/** Surface the dependency on tauri's `invoke` as a parameter so the
 *  helper is unit-testable without running inside Tauri. The default
 *  export below builds a closure that uses the real `invoke`. */
export interface ApplyCapabilityInvoker {
  invoke<T = unknown>(command: string, args?: unknown): Promise<T>
}

export interface ApplyCapabilityChangeArgs {
  /** Plugin's manifest id — passed to `revoke_plugin_capability`. */
  pluginId: string
  /** Plugin's directory on disk — passed to
   *  `set_plugin_granted_capabilities` so the right
   *  `granted_caps.json` is overwritten. */
  pluginDir: string
  /** Version stamped onto the persisted entry. The kernel resets
   *  grants on a version mismatch, which forces a re-prompt the
   *  next time the plugin loads. */
  version: string
  /** Capability set the plugin had granted before the user opened
   *  the consent modal. */
  prior: ReadonlyArray<Capability>
  /** Capability set the user chose in the modal. `null` semantically
   *  means "deny everything" — the persisted set becomes empty. */
  next: ReadonlyArray<Capability> | null
}

export interface ApplyCapabilityChangeResult {
  /** Caps that were in `prior` but not in the new set. The helper
   *  fired one `revoke_plugin_capability` per entry. */
  revoked: Capability[]
  /** Caps that the helper tried to revoke but the kernel rejected
   *  (e.g. plugin not loaded). The persist-to-disk step still
   *  succeeded; these are surfaced so the caller can warn the user
   *  that the live mutation didn't take. */
  revokeErrors: Array<{ capability: Capability; error: unknown }>
}

/** Set difference `a \ b` over capability values. Both sides are
 *  treated as deduplicated string-keyed sets — equality is by the
 *  kernel-string form so the helper doesn't depend on object
 *  identity. */
export function diffRevokedCapabilities(
  prior: ReadonlyArray<Capability>,
  next: ReadonlyArray<Capability>,
): Capability[] {
  if (prior.length === 0) return []
  const nextKeys = new Set(capsToKernelStrings(next as Capability[]))
  const out: Capability[] = []
  for (const cap of prior) {
    const key = capsToKernelStrings([cap])[0]
    if (!key) continue
    if (!nextKeys.has(key)) out.push(cap)
  }
  return out
}

/**
 * Persist the new grant set to disk and live-revoke any cap that was
 * removed. Returns the list of revoked caps + any kernel rejections.
 * The disk write is best-effort — a failure surfaces as a thrown
 * error from this function (matching the pre-existing call sites'
 * try/catch shape); a revoke failure is captured in `revokeErrors`
 * and the function returns normally because the file is already
 * authoritative for the next boot.
 */
export async function applyCapabilityChange(
  invoker: ApplyCapabilityInvoker,
  args: ApplyCapabilityChangeArgs,
): Promise<ApplyCapabilityChangeResult> {
  const { pluginId, pluginDir, version, prior, next } = args
  const nextOrEmpty: ReadonlyArray<Capability> = next ?? []
  // Persist first — `set_plugin_granted_capabilities` is the
  // authoritative store. If the user is offline-revoking (no kernel
  // booted) the file write is the only thing that survives.
  await invoker.invoke('set_plugin_granted_capabilities', {
    pluginDir,
    version,
    capabilities: capsToKernelStrings(nextOrEmpty as Capability[]),
  })
  const revoked = diffRevokedCapabilities(prior, nextOrEmpty)
  const revokeErrors: Array<{ capability: Capability; error: unknown }> = []
  for (const cap of revoked) {
    const kernelString = capsToKernelStrings([cap])[0]
    if (!kernelString) continue
    try {
      await invoker.invoke('revoke_plugin_capability', {
        pluginId,
        capability: kernelString,
      })
    } catch (err) {
      revokeErrors.push({ capability: cap, error: err })
    }
  }
  return { revoked, revokeErrors }
}
