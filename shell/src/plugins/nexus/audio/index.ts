// shell/src/plugins/nexus/audio/index.ts
//
// BL-118 — Web Speech API shell integration. Companion to BL-117
// (the `com.nexus.audio` kernel plugin). Exposes two shell commands
// other plugins call to drive the runtime:
//
//   - `nexus.audio.transcribe` — capture one utterance, copy the
//     recognised text to the clipboard, and append a toast with
//     the transcript.
//   - `nexus.audio.synthesize` — speak text via the platform TTS
//     engine. Pass `text` in the command args.
//
// Programmatic callers (other plugins) import the runtime
// directly via `import { transcribe, synthesize } from './runtime'`
// rather than reaching through the command bus, since commands
// don't carry typed args today.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import {
  getUseWebSpeech,
  probeSpeechSupport,
  setUseWebSpeech,
  synthesize,
  transcribe,
} from './runtime'

const COMMAND_TRANSCRIBE = 'nexus.audio.transcribe'
const COMMAND_SYNTHESIZE = 'nexus.audio.synthesize'
const COMMAND_STATUS = 'nexus.audio.status'

const CONFIG_USE_WEB_SPEECH = 'nexus.audio.useWebSpeech'
const CONFIG_DEFAULT_LANG = 'nexus.audio.defaultLanguage'
const CONFIG_DEFAULT_VOICE = 'nexus.audio.defaultVoice'
const CONFIG_DEFAULT_RATE = 'nexus.audio.defaultRate'

export const audioPlugin: Plugin = {
  manifest: {
    id: 'nexus.audio',
    name: 'Audio (Web Speech)',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      configuration: {
        pluginId: 'nexus.audio',
        title: 'Audio',
        order: 72,
        schema: [
          {
            key: CONFIG_USE_WEB_SPEECH,
            title: 'Use Web Speech API',
            description:
              'When enabled (default), transcription and speech synthesis run in-browser via ' +
              'webkitSpeechRecognition / SpeechSynthesisUtterance. Disable to route every ' +
              'request through the kernel-side com.nexus.audio plugin (Whisper / Piper / ' +
              'OpenAI per your forge config).',
            type: 'boolean' as const,
            default: true,
          },
          {
            key: CONFIG_DEFAULT_LANG,
            title: 'Default language (BCP-47)',
            description:
              'Language tag passed to both STT and TTS by default — e.g. "en-US" / "fr-FR". ' +
              'Leave blank to use the browser\'s default locale.',
            type: 'string' as const,
            default: '',
          },
          {
            key: CONFIG_DEFAULT_VOICE,
            title: 'Default voice URI',
            description:
              'Platform voice identifier — typically picked from a dropdown surfaced by the ' +
              'plugin\'s settings UI. Leave blank to use the platform default.',
            type: 'string' as const,
            default: '',
          },
          {
            key: CONFIG_DEFAULT_RATE,
            title: 'Speaking rate',
            description: 'Synthesis rate, 0.1–10. Defaults to 1.0 (natural pace).',
            type: 'number' as const,
            default: 1.0,
          },
        ],
      },
      commands: [
        {
          id: COMMAND_TRANSCRIBE,
          title: 'Audio: Transcribe microphone',
          category: 'Audio',
        },
        {
          id: COMMAND_SYNTHESIZE,
          title: 'Audio: Speak text…',
          category: 'Audio',
        },
        {
          id: COMMAND_STATUS,
          title: 'Audio: Show backend status',
          category: 'Audio',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.configuration.register(audioPlugin.manifest.contributes!.configuration!)

    // Initial preference from persisted settings.
    const persisted = api.configuration.getValue<boolean>(CONFIG_USE_WEB_SPEECH, true)
    setUseWebSpeech(persisted !== false)
    api.configuration.onChange(CONFIG_USE_WEB_SPEECH, (val) => {
      setUseWebSpeech(val !== false)
    })

    api.commands.register(COMMAND_TRANSCRIBE, async () => {
      try {
        const lang = api.configuration.getValue<string>(CONFIG_DEFAULT_LANG, '')
        const result = await transcribe(api, lang ? { lang } : {})
        if (result.text) {
          try {
            await navigator.clipboard.writeText(result.text)
            api.notifications.show({
              type: 'info',
              message: `Transcribed (${result.backend}): "${truncate(result.text, 60)}" — copied to clipboard`,
            })
          } catch {
            api.notifications.show({
              type: 'info',
              message: `Transcribed (${result.backend}): ${truncate(result.text, 80)}`,
            })
          }
        } else {
          api.notifications.show({ type: 'warning', message: 'No speech detected.' })
        }
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Transcribe failed: ${(err as Error).message}`,
        })
      }
    })

    api.commands.register(COMMAND_SYNTHESIZE, async (args?: unknown) => {
      const text =
        typeof args === 'string'
          ? args
          : typeof (args as { text?: unknown } | undefined)?.text === 'string'
            ? ((args as { text: string }).text)
            : null
      if (!text) {
        // No text arg — prompt the user.
        const entry = await api.input.prompt('Audio: Speak text', 'Text to read aloud…')
        if (!entry || entry.trim().length === 0) return
        await speakWithDefaults(api, entry)
        return
      }
      await speakWithDefaults(api, text)
    })

    api.commands.register(COMMAND_STATUS, () => {
      const sup = probeSpeechSupport()
      const using = getUseWebSpeech()
      const stt = using && sup.stt ? 'web-speech' : 'ipc (com.nexus.audio)'
      const tts = using && sup.tts ? 'web-speech' : 'ipc (com.nexus.audio)'
      api.notifications.show({
        type: 'info',
        message: `Audio backends — STT: ${stt} · TTS: ${tts}`,
      })
    })
  },
}

async function speakWithDefaults(api: PluginAPI, text: string): Promise<void> {
  try {
    const lang = api.configuration.getValue<string>(CONFIG_DEFAULT_LANG, '')
    const voice = api.configuration.getValue<string>(CONFIG_DEFAULT_VOICE, '')
    const rate = api.configuration.getValue<number>(CONFIG_DEFAULT_RATE, 1.0)
    await synthesize(api, text, {
      lang: lang || undefined,
      voice: voice || undefined,
      rate: typeof rate === 'number' ? rate : undefined,
    })
  } catch (err) {
    api.notifications.show({
      type: 'error',
      message: `Speak failed: ${(err as Error).message}`,
    })
  }
}

function truncate(s: string, n: number): string {
  if (s.length <= n) return s
  return `${s.slice(0, n - 1)}…`
}

// Re-export the runtime so other plugins (e.g. quick-capture)
// can import `transcribe` / `synthesize` directly.
export { startContinuous, synthesize, transcribe } from './runtime'
export type {
  ContinuousHandle,
  SynthesizeOptions,
  TranscribeOptions,
  TranscribeResult,
} from './runtime'
