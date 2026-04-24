// shell/src/plugins/core/capabilityPrompt/requestConsent.ts
//
// WI-31 — Orchestrator entry point, called from main.tsx between
// `scanCommunityPlugins` and `loadEnabledCommunityPlugins`.
//
// Semantics:
//   1. For every discovered manifest, read `granted_caps.json` from the
//      plugin dir (one Tauri call for the whole batch).
//   2. Run `decideConsent` per plugin. auto-accept → carry on;
//      banner → queue non-blocking; modal → enqueue blocking.
//   3. Await each modal sequentially. The FIFO queue in the store means
//      the second plugin doesn't show until the first is resolved.
//   4. Persist grants back to disk (kernel dotted form). Denied plugins
//      get an empty grant set written with the NEW version string so
//      the kernel's version-pin logic marks them deny-all on load.
//   5. Return a Map<pluginId, 'approved' | 'denied' | 'auto'> so
//      `loadEnabledCommunityPlugins` can skip denied plugins.

import { invoke } from '@tauri-apps/api/core'
import type { Capability } from '@nexus/extension-api'
import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'
import { parseManifestCapabilities } from '../../nexus/pluginsMgmt/capabilityInfo'
import {
  decideConsent,
  parsePriorGrant,
  type PriorGrant,
} from './consentLogic'
import { capsToKernelStrings } from './capabilityMapping'
import {
  useCapabilityPromptStore,
  type ModalPrompt,
} from './capabilityPromptStore'

export type ConsentOutcome = 'approved' | 'denied' | 'auto'

export interface ConsentResult {
  outcomes: Map<string, ConsentOutcome>
  /** Plugin ids the user denied in this run — should be filtered out
   *  of the enabled-community list before activation. */
  denied: Set<string>
}

export interface ConsentRunnerDeps {
  /** Injected for tests — the live path uses the Tauri bridge. */
  getGranted?: (
    plugin_dirs: Record<string, string>,
  ) => Promise<Record<string, PriorGrant>>
  setGranted?: (args: {
    plugin_dir: string
    version: string
    capabilities: string[]
  }) => Promise<void>
}

const defaultDeps: Required<ConsentRunnerDeps> = {
  getGranted: (plugin_dirs) =>
    invoke<Record<string, PriorGrant>>('get_plugin_granted_capabilities', {
      pluginDirs: plugin_dirs,
    }),
  setGranted: (args) =>
    invoke('set_plugin_granted_capabilities', {
      pluginDir: args.plugin_dir,
      version: args.version,
      capabilities: args.capabilities,
    }),
}

/**
 * Run the consent flow for every manifest with enabled=true. Manifests
 * that are disabled are ignored — consent happens at the moment the
 * plugin is about to activate, so a disabled plugin with declared caps
 * will only prompt when the user flips it on (that path routes through
 * the settings "Review capabilities" button; see plugin entry).
 *
 * Manifests with no declared capabilities auto-accept. Manifests the
 * user has already approved at the same major.minor version auto-accept
 * without UI flash.
 */
export async function runInstallTimeConsent(
  manifests: CommunityPluginManifest[],
  deps: ConsentRunnerDeps = {},
): Promise<ConsentResult> {
  const outcomes = new Map<string, ConsentOutcome>()
  const denied = new Set<string>()

  const enabled = manifests.filter((m) => m.enabled)
  if (enabled.length === 0) {
    return { outcomes, denied }
  }

  const getGranted = deps.getGranted ?? defaultDeps.getGranted
  const setGranted = deps.setGranted ?? defaultDeps.setGranted

  // Batch-load prior grants.
  const dirMap: Record<string, string> = {}
  for (const m of enabled) dirMap[m.id] = m.dir
  let priorMap: Record<string, PriorGrant> = {}
  try {
    priorMap = await getGranted(dirMap)
  } catch (err) {
    console.warn(
      '[core.capabilityPrompt] get_plugin_granted_capabilities failed; ' +
        'assuming no prior grants:',
      err,
    )
  }

  for (const m of enabled) {
    const declared = parseManifestCapabilities(m.capabilities)
    const prior = parsePriorGrant(priorMap[m.id])
    const decision = decideConsent({
      declared,
      currentVersion: m.version,
      prior,
    })

    if (decision.kind === 'auto-accept') {
      outcomes.set(m.id, 'auto')
      // Patch-bump path: rewrite the grants file with the new version
      // string so the kernel's version-equality check passes on the
      // next load. No caps change; we re-persist the prior set.
      if (decision.reason === 'patch-bump' && prior.version !== m.version) {
        try {
          await setGranted({
            plugin_dir: m.dir,
            version: m.version,
            capabilities: capsToKernelStrings(prior.capabilities),
          })
        } catch (err) {
          console.warn(
            `[core.capabilityPrompt] failed to refresh grants for ${m.id}:`,
            err,
          )
        }
      }
      continue
    }

    if (decision.kind === 'banner') {
      useCapabilityPromptStore.getState().pushBanner({
        pluginId: m.id,
        pluginName: m.name,
        caps: decision.caps,
      })
      outcomes.set(m.id, 'auto')
      continue
    }

    // Modal path — await one at a time. The store's FIFO queue means
    // we can fire-and-await these serially without extra plumbing; if
    // two prompts land concurrently the second enqueues behind.
    const grantedCaps = await requestModalConsent({
      pluginId: m.id,
      pluginName: m.name,
      version: m.version,
      pluginDir: m.dir,
      caps: decision.caps,
      previouslyGranted: decision.previouslyGranted,
      reason: decision.reason,
    })

    if (grantedCaps === null) {
      // Denied. Persist an empty grant pinned to the new version so
      // the kernel also sees deny-all and doesn't load with lingering
      // HIGH-risk caps from a previous accept.
      outcomes.set(m.id, 'denied')
      denied.add(m.id)
      try {
        await setGranted({
          plugin_dir: m.dir,
          version: m.version,
          capabilities: [],
        })
      } catch (err) {
        console.warn(
          `[core.capabilityPrompt] failed to persist denial for ${m.id}:`,
          err,
        )
      }
      continue
    }

    outcomes.set(m.id, 'approved')
    try {
      await setGranted({
        plugin_dir: m.dir,
        version: m.version,
        capabilities: capsToKernelStrings(grantedCaps),
      })
    } catch (err) {
      console.warn(
        `[core.capabilityPrompt] failed to persist grants for ${m.id}:`,
        err,
      )
    }
  }

  return { outcomes, denied }
}

/**
 * Await a single modal decision. Returns the granted capability list on
 * Approve, or `null` on Deny. Pulled out of the loop above so tests
 * and the Settings "Review capabilities" button can reuse it.
 */
export function requestModalConsent(
  prompt: Omit<ModalPrompt, 'resolve'>,
): Promise<Capability[] | null> {
  return new Promise((resolve) => {
    useCapabilityPromptStore.getState().enqueueModal({
      ...prompt,
      resolve: ({ ok, grantedCaps }) => {
        resolve(ok ? grantedCaps : null)
      },
    })
  })
}
