// shell/src/plugins/community/mermaid/index.js
//
// Stub bundle slot. The community-plugin loader (driven by plugin.json)
// would dynamic-import this file via a Blob URL on activation. The
// plugin's manifest is intentionally `enabled: false` — registration
// flows through `shell/src/plugins/catalog.ts` (DEFAULT_OFF_PLUGINS)
// because the Blob-URL loader can't resolve `import "mermaid"` without
// a community-plugin bundler. See README.md.
//
// The default export below satisfies the loader's contract in case a
// future change toggles `enabled: true` before the bundler lands —
// the plugin will load harmlessly and warn rather than crashing the
// host.

export default {
  manifest: {
    id: 'community.mermaid',
    name: 'Mermaid Diagrams',
    version: '1.0.0',
    core: false,
    activationEvents: ['*'],
    apiVersion: 1,
  },
  activate() {
    console.warn(
      '[community.mermaid] index.js stub activated — bare specifier ' +
        '`mermaid` cannot be resolved from a Blob URL. Register the plugin ' +
        'via shell catalog (DEFAULT_OFF_PLUGINS) until a community bundler lands.',
    )
  },
}
