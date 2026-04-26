// src/host/shellHost.ts
// Singleton ExtensionHost reference — mirrors shellRegistry. main.tsx
// sets this after boot; mid-session hot-activation paths read it to
// register + activate dormant default-off built-ins without a reload.

import type { ExtensionHost } from './ExtensionHost'

let _host: ExtensionHost | null = null

export function setHost(host: ExtensionHost) {
  _host = host
}

export function getHost(): ExtensionHost | null {
  return _host
}
