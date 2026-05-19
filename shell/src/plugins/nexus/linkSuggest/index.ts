// shell/src/plugins/nexus/linkSuggest/index.ts
//
// BL-039 — AI-DIR auto-link suggestion plugin.
//
// The actual ghost-rendering CodeMirror extension lives at
// `editor/cm/linkSuggest.ts` (composed into the editor extension
// list alongside `ghostCompletionExt`). This plugin is the
// activation surface: it owns the configuration schema and the
// enabled-toggle so users can switch the feature on from
// Settings > Plugins (the catalog ships it default-off, mirroring
// `aiPlugin` / `semanticSearchPlugin`).
//
// Why no separate runtime here? The CM extension reads its settings
// lazily via `configStore` and reaches the AI handler through
// `getGhostApi()` — both already populated by the AI plugin's
// activate. As long as the user has the AI plugin enabled (the
// catalog comment on `semanticSearchPlugin` spells out the same
// pairing rule) link-suggest works automatically.

import type { Plugin, PluginAPI } from '../../../types/plugin'

export const linkSuggestPlugin: Plugin = {
  manifest: {
    id: 'nexus.linkSuggest',
    name: 'AI Link Suggestions',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      configuration: {
        pluginId: 'nexus.linkSuggest',
        title: 'AI Link Suggestions',
        order: 51,
        category: 'ai',
        schema: [
          {
            key: 'ai.linkSuggest.enabled',
            title: 'Inline link suggestions',
            description:
              'When typing a phrase that semantically matches an existing note, surface a [[wiki-link]] ghost suggestion. Tab accepts; Esc dismisses.',
            type: 'boolean' as const,
            default: true,
          },
          {
            key: 'ai.linkSuggest.debounceMs',
            title: 'Link suggestion debounce (ms)',
            description:
              'Quiet-period after a keystroke before querying the semantic index.',
            type: 'number' as const,
            default: 600,
          },
          {
            key: 'ai.linkSuggest.minChars',
            title: 'Link suggestion minimum phrase length',
            description: 'Skip suggestions when the trailing phrase is shorter.',
            type: 'number' as const,
            default: 4,
          },
          {
            key: 'ai.linkSuggest.maxChars',
            title: 'Link suggestion maximum phrase length',
            description: 'Cap on the trailing phrase length sent to the index.',
            type: 'number' as const,
            default: 80,
          },
          {
            key: 'ai.linkSuggest.scoreGate',
            title: 'Link suggestion score threshold',
            description:
              'Minimum top-match similarity score (0.0–1.0) required to surface a suggestion. Higher = fewer, more confident suggestions.',
            type: 'number' as const,
            default: 0.55,
          },
        ],
      },
    },
  },

  activate(api: PluginAPI) {
    api.configuration.register(linkSuggestPlugin.manifest.contributes!.configuration!)
  },
}
