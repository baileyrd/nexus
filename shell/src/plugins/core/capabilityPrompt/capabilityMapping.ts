// shell/src/plugins/core/capabilityPrompt/capabilityMapping.ts
//
// WI-31 — Install-time capability prompt.
//
// The shell-side Capability union is PascalCase (ts-rs-generated from
// `crates/nexus-plugin-api/src/capability.rs`). The kernel's
// `Capability::from_str` parses dotted strings (`"fs.read"`,
// `"process.spawn"`). Plugin manifests on the JS side use PascalCase
// because that's what the TS union is; the kernel's `granted_caps.json`
// uses dotted because that's what `Capability::as_str` emits.
//
// This file owns the translation between the two forms. Source of truth
// mirrors `crates/nexus-plugin-api/src/capability.rs` — keep in sync when
// a new variant is added (the `Record<Capability, string>` annotation
// below is the exhaustiveness check at typecheck time).

import type { Capability } from '@nexus/extension-api'

/**
 * PascalCase → dotted kernel form. `Capability::from_str` at
 * `crates/nexus-plugin-api/src/capability.rs:115` is the canonical parser
 * on the Rust side; these strings must match exactly.
 */
export const CAPABILITY_TO_KERNEL_STRING: Record<Capability, string> = {
  FsRead:           'fs.read',
  FsWrite:          'fs.write',
  FsReadExternal:   'fs.read.external',
  FsWriteExternal:  'fs.write.external',
  NetHttp:          'net.http',
  NetHttpLocalhost: 'net.http.localhost',
  ProcessSpawn:     'process.spawn',
  KvRead:           'kv.read',
  KvWrite:          'kv.write',
  IpcCall:          'ipc.call',
  DbQuery:          'db.query',
  DbWrite:          'db.write',
  EventsPublish:    'events.publish',
  UiNotify:         'ui.notify',
  // ADR 0022 — per-handler ai.* capability surface.
  AiChat:           'ai.chat',
  AiIndex:          'ai.index',
  AiSessionRead:    'ai.session.read',
  AiSessionWrite:   'ai.session.write',
  AiConfigWrite:    'ai.config.write',
  AiActivityWrite:  'ai.activity.write',
  AiToolsWrite:     'ai.tools.write',
  AiToolsMcp:       'ai.tools.mcp',
  // BL-117 — audio subsystem caps.
  AudioRecord:      'audio.record',
  AudioSynthesize:  'audio.synthesize',
}

/** Dotted → PascalCase; inverse of the above. */
export const KERNEL_STRING_TO_CAPABILITY: Record<string, Capability> =
  Object.fromEntries(
    Object.entries(CAPABILITY_TO_KERNEL_STRING).map(
      ([k, v]) => [v, k as Capability],
    ),
  )

/**
 * Translate a PascalCase capability list (as declared in plugin.json) to
 * the dotted strings the kernel persists in `granted_caps.json`. Unknown
 * inputs are silently dropped — the capability info parser has already
 * filtered to known variants upstream, so this is defence-in-depth.
 */
export function capsToKernelStrings(caps: readonly Capability[]): string[] {
  return caps
    .map((c) => CAPABILITY_TO_KERNEL_STRING[c])
    .filter((s): s is string => typeof s === 'string')
}

/**
 * Translate dotted kernel strings (as persisted in `granted_caps.json`)
 * back into PascalCase `Capability`s. Unknown strings are dropped —
 * forward-compat with older shell versions reading a file written by a
 * newer one.
 */
export function kernelStringsToCaps(strs: readonly string[]): Capability[] {
  const out: Capability[] = []
  for (const s of strs) {
    const c = KERNEL_STRING_TO_CAPABILITY[s]
    if (c) out.push(c)
  }
  return out
}
