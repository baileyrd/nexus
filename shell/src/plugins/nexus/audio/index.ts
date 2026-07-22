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
import { writeAttachment } from '../editor/attachments'
import { appendInbox } from '../memory/kernelClient'
import {
  getUseWebSpeech,
  probeSpeechSupport,
  recordVoiceMemo,
  setUseWebSpeech,
  synthesize,
  transcribe,
} from './runtime'

const COMMAND_TRANSCRIBE = 'nexus.audio.transcribe'
const COMMAND_SYNTHESIZE = 'nexus.audio.synthesize'
const COMMAND_STATUS = 'nexus.audio.status'
const COMMAND_RECORD_VOICE_MEMO = 'nexus.audio.recordVoiceMemo'

const CONFIG_USE_WEB_SPEECH = 'nexus.audio.useWebSpeech'
const CONFIG_DEFAULT_LANG = 'nexus.audio.defaultLanguage'
const CONFIG_DEFAULT_VOICE = 'nexus.audio.defaultVoice'
const CONFIG_DEFAULT_RATE = 'nexus.audio.defaultRate'

// C11 (#364) — same BL-043 quick-capture inbox the memory plugin's text
// capture writes to (`memory/index.ts`), read the same cross-plugin way
// `enrich`/`recall` already do, so a voice memo lands in the same file.
const CONFIG_INBOX_PATH = 'memory.inboxPath'
const DEFAULT_INBOX_PATH = 'Inbox.md'
/** Forge-relative folder voice recordings are saved under. Matches the
 *  extension-based `attachment` classification `infer_file_type` already
 *  applies to `.webm`/`.wav` regardless of directory, so any folder would
 *  index correctly — `attachments/` is used for consistency with the
 *  editor's paste/drop attachment pipeline. */
const VOICE_MEMO_DIR = 'attachments'

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
        category: 'system',
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
        {
          id: COMMAND_RECORD_VOICE_MEMO,
          title: 'Audio: Record voice memo',
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

    // C11 (#364) — record, transcribe, append the transcript to the
    // quick-capture inbox, and save the recording as a forge attachment.
    // Always uses the kernel MediaRecorder + com.nexus.audio::transcribe
    // path (via recordVoiceMemo) since the browser-only Web Speech API
    // never yields bytes to persist.
    api.commands.register(COMMAND_RECORD_VOICE_MEMO, async () => {
      const ready = await api.kernel.available()
      if (!ready) {
        api.notifications.show({
          type: 'warning',
          message: 'Open a forge before recording — voice capture needs an active workspace.',
        })
        return
      }
      try {
        api.notifications.show({ type: 'info', message: 'Recording voice memo (5s)…' })
        const lang = api.configuration.getValue<string>(CONFIG_DEFAULT_LANG, '')
        const memo = await recordVoiceMemo(api, lang ? { lang } : {})
        const relpath = await writeAttachment(
          api.kernel,
          VOICE_MEMO_DIR,
          voiceMemoName(memo.format, new Date()),
          memo.bytes,
        )
        const inboxPath = api.configuration.getValue<string>(CONFIG_INBOX_PATH, DEFAULT_INBOX_PATH)
        const stamp = new Date().toISOString()
        const transcript = memo.text.trim() || '_(no speech detected)_'
        const snippet = `## Voice memo — ${stamp}\n\n${transcript}\n\n![[${relpath}]]`
        await appendInbox(api.kernel, inboxPath, snippet)
        api.notifications.show({
          type: 'success',
          message: `Voice memo captured to ${inboxPath} (audio saved to ${relpath})`,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Voice memo failed: ${(err as Error).message}`,
        })
      }
    })
  },
}

/** Name for a recorded voice memo — mirrors `pastedImageName`'s
 *  timestamp-collision-avoidance shape (`writeAttachment`'s probe loop
 *  covers same-second repeats). */
function voiceMemoName(format: 'webm' | 'wav', now: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  const stamp =
    `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}` +
    `-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`
  return `voice-memo-${stamp}.${format}`
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
