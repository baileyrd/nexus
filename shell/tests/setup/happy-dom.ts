// Test-suite-wide DOM shim. Registered globally so any `node --test`
// run that imports this file gets `window`, `document`, and friends as
// real globals before user test files load.
//
// We use happy-dom (not jsdom) for install/startup cost — see
// `src/plugins/nexus/editor/cm/livePreview.runtime.test.ts` for the
// motivating CM6 mount-time check.

import { GlobalRegistrator } from '@happy-dom/global-registrator'

if (!(globalThis as { __nexusHappyDomRegistered?: boolean }).__nexusHappyDomRegistered) {
  GlobalRegistrator.register()
  ;(globalThis as { __nexusHappyDomRegistered?: boolean }).__nexusHappyDomRegistered = true
}
