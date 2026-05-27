// shell/src/plugins/nexus/aiSettings/index.ts
//
// AI provider settings — accessible even when the nexus.ai plugin is
// disabled. Extracts the 8 provider keys from nexus.ai's manifest and
// registers them independently so the Settings panel always shows them.
//
// On activate it reads the saved values out of the config store and
// pushes them to the kernel via pushUserConfig, then subscribes to
// config:changed events to re-push on every change — exactly the same
// pattern as nexus.ai did, but scoped to this always-on plugin.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useConfigStore } from '../../../stores/configStore'
import { pushUserConfig, type AiUserConfig } from '../ai/aiRuntime'

export const aiSettingsPlugin: Plugin = {
  manifest: {
    id: 'nexus.aiSettings',
    name: 'AI Settings',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      configuration: {
        pluginId: 'nexus.ai',
        title: 'AI',
        order: 50,
        category: 'ai',
        schema: [
          {
            key: 'ai.provider',
            title: 'Chat provider',
            description:
              'AI provider used for chat. Leave blank to fall back to environment variables (ANTHROPIC_API_KEY / OPENAI_API_KEY / OLLAMA_BASE_URL).',
            type: 'select' as const,
            default: '',
            options: ['', 'anthropic', 'openai', 'ollama'],
          },
          {
            key: 'ai.model',
            title: 'Chat model',
            description:
              'Optional model override (e.g. claude-sonnet-4-5, gpt-4o-mini, llama3.1). Leave blank for the provider default.',
            type: 'string' as const,
            default: '',
          },
          {
            key: 'ai.apiKey',
            title: 'API key',
            description:
              'Required for Anthropic and OpenAI. Stored locally in this workspace; never sent anywhere except the provider you choose.',
            type: 'password' as const,
            default: '',
          },
          {
            key: 'ai.baseUrl',
            title: 'Base URL',
            description:
              'Override the provider endpoint. For Ollama this points at your local server (default http://localhost:11434).',
            type: 'string' as const,
            default: '',
          },
          {
            key: 'ai.embedProvider',
            title: 'Embedding provider',
            description:
              'Provider for the RAG retrieval embeddings. OpenAI gives higher quality; Ollama runs locally with a model server; "local" uses the in-process fastembed-rs backend (no network, requires the local-embeddings build feature). Leave blank to share the chat provider where supported.',
            type: 'select' as const,
            default: '',
            options: ['', 'openai', 'ollama', 'local'],
          },
          {
            key: 'ai.embedModel',
            title: 'Embedding model',
            description:
              'For Ollama: the embedding model name (nomic-embed-text, mxbai-embed-large). For "local": the fastembed identifier (bge-small-en-v1.5-int8 default, also bge-small/base/large-en-v1.5, mxbai-embed-large-v1, nomic-embed-text-v1.5, all-mini-lm-l6-v2). First use of a local model downloads ~33–500 MB to ~/.cache/fastembed/. Leave blank for the provider default.',
            type: 'string' as const,
            default: '',
          },
          {
            key: 'ai.embedApiKey',
            title: 'Embedding API key',
            description:
              'Only required when the embedding provider differs from the chat provider. Otherwise the chat key is reused.',
            type: 'password' as const,
            default: '',
          },
          {
            key: 'ai.embedBaseUrl',
            title: 'Embedding base URL',
            description:
              'Override the embedding endpoint (used for self-hosted Ollama or OpenAI-compatible proxies).',
            type: 'string' as const,
            default: '',
          },
        ],
      },
    },
  },

  async activate(api: PluginAPI) {
    api.configuration.register(aiSettingsPlugin.manifest.contributes!.configuration!)

    const readUserConfig = (): AiUserConfig => {
      const cfg = useConfigStore.getState()
      return {
        provider: cfg.get<string>('ai.provider', ''),
        model: cfg.get<string>('ai.model', ''),
        apiKey: cfg.get<string>('ai.apiKey', ''),
        baseUrl: cfg.get<string>('ai.baseUrl', ''),
        embedProvider: cfg.get<string>('ai.embedProvider', ''),
        embedModel: cfg.get<string>('ai.embedModel', ''),
        embedApiKey: cfg.get<string>('ai.embedApiKey', ''),
        embedBaseUrl: cfg.get<string>('ai.embedBaseUrl', ''),
      }
    }

    // Re-push whenever any of the 8 provider keys change.
    const aiKeys = [
      'ai.provider',
      'ai.model',
      'ai.apiKey',
      'ai.baseUrl',
      'ai.embedProvider',
      'ai.embedModel',
      'ai.embedApiKey',
      'ai.embedBaseUrl',
    ]
    for (const key of aiKeys) {
      api.events.on(`config:changed:${key}`, () => {
        void pushUserConfig(api, readUserConfig())
      })
    }

    // Initial push of saved provider settings into the kernel.
    void pushUserConfig(api, readUserConfig())
  },
}
