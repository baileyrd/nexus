// Test-suite-wide DOM shim, registered via `node --import ./test-setup.ts`
// before any test file loads. `index.ts` calls `bootstrapSandboxedPlugin`
// at module scope, which expects real `window`/`postMessage` globals —
// happy-dom supplies those so the plugin can be imported and exercised
// under plain Node.

import { GlobalRegistrator } from '@happy-dom/global-registrator'

if (!(globalThis as { __pluginTestHappyDomRegistered?: boolean }).__pluginTestHappyDomRegistered) {
  GlobalRegistrator.register()
  ;(globalThis as { __pluginTestHappyDomRegistered?: boolean }).__pluginTestHappyDomRegistered = true
}
