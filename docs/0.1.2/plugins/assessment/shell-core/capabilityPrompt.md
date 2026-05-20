# capabilityPrompt

- **Path:** `shell/src/plugins/core/capabilityPrompt/`
- **Tier:** Shell Core
- **Plugin id:** `core.capabilityPrompt`

## Architecture
- Entry point: `shell/src/plugins/core/capabilityPrompt/index.ts:22`
- Activation: `onStartup`; `popoutCompatible: false`
- Modules:
  - `index.ts` — registers the modal + banner overlay views, re-exports the consent API for `main.tsx`
  - `requestConsent.ts:80` — `runInstallTimeConsent(manifests, deps)` orchestrator called by `main.tsx` between `scanCommunityPlugins` and `loadEnabledCommunityPlugins`
  - `requestConsent.ts:207` — `requestModalConsent(prompt)` standalone modal awaiter, reused by the Settings "Review capabilities" button
  - `consentLogic.ts` — pure decision function `decideConsent({declared, currentVersion, prior})` → `auto-accept | banner | modal`; SemVer parsing and patch-bump detection
  - `capabilityPromptStore.ts` — FIFO Zustand store holding modal queue + active banners
  - `applyCapabilityChange.ts` — diff + IPC apply path used when a user revokes caps from a running plugin
  - `capabilityMapping.ts` — bidirectional `Capability` ↔ kernel dotted-string conversion (`capsToKernelStrings`, `kernelStringsToCaps`)
  - `CapabilityModalView.tsx`, `CapabilityBannerView.tsx` — the rendered overlays
- Persistence: writes `granted_caps.json` per community plugin via Tauri commands `get_plugin_granted_capabilities` / `set_plugin_granted_capabilities`; in-memory queue otherwise
- Settings owned: none
- External deps: `@tauri-apps/api/core` (invoke); `@nexus/extension-api` for the `Capability` type

## Surface
- **Views:** `core.capabilityPrompt.modal` and `core.capabilityPrompt.banner` registered into the `overlay` slot at priorities 95 and 10
- **Commands / keybindings / settings:** none contributed via manifest
- **Consumes from `@nexus/extension-api`:** `Capability` type; consumes `Plugin`, `PluginAPI` from local types
- **External entry points (not via PluginAPI):** `runInstallTimeConsent`, `requestModalConsent`, `applyCapabilityChange`, `diffRevokedCapabilities` — imported directly by `main.tsx` and `nexus.pluginsMgmt`

## Necessity
- **Verdict:** Useful
- **Required for basic capabilities?** No — opening a forge, browsing/editing markdown, search, and git commit do not exercise this plugin. It only fires when a community plugin has been scanned and declares non-trivial capabilities.
- **Depended on by:** `shell/src/main.tsx` (calls `runInstallTimeConsent` at boot); `shell/src/plugins/nexus/pluginsMgmt/` (uses `requestModalConsent` + `applyCapabilityChange` from Settings → Plugins → Review capabilities)
- **Depends on:** Tauri host commands `get_plugin_granted_capabilities`, `set_plugin_granted_capabilities`; the community plugin manifest scanner in `host/communityPluginLoader`
- **What breaks if removed:** No prompt for high-risk caps → either community plugins always boot with whatever they declare (security regression) or fail because grants are never persisted. The Settings UI loses its capability review affordance.

## Notes
- Security-critical: this is the user's single touch point before a community plugin gains `fs.write`, `ipc.call`, etc. Removing it without a replacement would silently grant or silently deny — both bad.
- The plugin is UI-only; the decision and persistence logic are deliberately importable as plain functions so `main.tsx` can run consent before the extension host exists.
- Modal priority (95) is set just above `nexus.confirm` (90) for explicit stacking — see comment at `index.ts:38`.
- Test coverage: `applyCapabilityChange.test.ts` exercises the diff + revoke path.
