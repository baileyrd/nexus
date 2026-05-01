// shell/src/plugins/core/capabilityPrompt/index.ts
//
// WI-31 — Install-time capability consent prompt.
//
// Registers two overlay views:
//   - CapabilityModalView  (blocking; high-risk consent)
//   - CapabilityBannerView (non-blocking; low/medium-only)
//
// The actual consent orchestration (`runInstallTimeConsent`) is a
// plain async function called from main.tsx between
// `scanCommunityPlugins` and `loadEnabledCommunityPlugins` — the plugin
// itself is UI-only, the decision + persistence logic is imported
// directly so it can run before any extension host exists.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { CapabilityModalView } from './CapabilityModalView'
import { CapabilityBannerView } from './CapabilityBannerView'

const MODAL_VIEW_ID = 'core.capabilityPrompt.modal'
const BANNER_VIEW_ID = 'core.capabilityPrompt.banner'

export const capabilityPromptPlugin: Plugin = {
  manifest: {
    id: 'core.capabilityPrompt',
    name: 'Capability Prompt',
    version: '0.1.0',
    core: true,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.views.register(MODAL_VIEW_ID, {
      slot: 'overlay',
      component: CapabilityModalView,
      // Above nexus.confirm (90) so a confirm raised from the modal
      // (e.g. "Are you sure you want to deny?") wouldn't stack below.
      // Today we don't raise confirms from here, but keeping modal
      // ordering explicit.
      priority: 95,
    })
    api.views.register(BANNER_VIEW_ID, {
      slot: 'overlay',
      component: CapabilityBannerView,
      // Below modal priority; banners sit in the corner and never
      // overlap the modal visually.
      priority: 10,
    })
  },
}

// Re-exports for main.tsx + tests.
export { runInstallTimeConsent, requestModalConsent } from './requestConsent'
export type { ConsentOutcome, ConsentResult } from './requestConsent'
export {
  decideConsent,
  parseSemVer,
  isPatchOnlyBump,
  parsePriorGrant,
} from './consentLogic'
export type {
  ConsentDecision,
  ConsentInput,
  PriorGrant,
  ParsedPriorGrant,
  SemVer,
} from './consentLogic'
export {
  capsToKernelStrings,
  kernelStringsToCaps,
  CAPABILITY_TO_KERNEL_STRING,
} from './capabilityMapping'
