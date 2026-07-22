// shell/src/plugins/nexus/audio/runtime.ts
//
// BL-118 — combined STT / TTS runtime. Picks the Web Speech API
// when available (zero latency, no audio capture round-trip), falls
// back to the kernel-side `com.nexus.audio` plugin (BL-117) when the
// browser lacks support OR the user has opted out via
// `nexus.audio.useWebSpeech = false`.
//
// Two public flows:
//
//   transcribe(opts?) -> Promise<string>
//     Records the next utterance. Web-speech path is one-shot
//     (microphone → text in-browser). IPC path uses MediaRecorder to
//     capture WebM/Opus bytes, then routes through `com.nexus.audio
//     ::transcribe`.
//
//   synthesize(text, opts?) -> Promise<void>
//     Speaks the text. Web-speech path uses SpeechSynthesisUtterance.
//     IPC path posts to `com.nexus.audio::synthesize` and plays the
//     returned base64 WAV / MP3 through an `<audio>` element.
//
// Continuous-mode STT (DoD bullet) is supported via
// `startContinuous(handler)` which fires `handler(text, isFinal)` on
// every interim + final result until the returned `stop()` runs.

import type { PluginAPI } from '../../../types/plugin'
import {
  type RecognitionHandle,
  createRecognition,
  listVoices,
  probeSpeechSupport,
  speak,
  startRecognition,
} from './speechApi'

const AUDIO_PLUGIN_ID = 'com.nexus.audio'
const HANDLER_TRANSCRIBE = 'transcribe'
const HANDLER_SYNTHESIZE = 'synthesize'

/**
 * Options for [`transcribe`]. Mirrors the BL-117 wire-level
 * [`AudioTranscribeArgs`] shape so a caller passing the same options
 * object to either path observes identical behaviour.
 */
export interface TranscribeOptions {
  lang?: string
  /** Force the kernel-side path even when Web Speech API is available. */
  forceIpc?: boolean
}

/**
 * Result of [`transcribe`]. `backend` matches the BL-117 reply field
 * — `"webspeech"` is shell-side; `"provider"` / `"local"` / `"platform"`
 * are kernel-side.
 */
export interface TranscribeResult {
  text: string
  backend: 'webspeech' | string
  language?: string
}

/** Options for [`synthesize`]. */
export interface SynthesizeOptions {
  voice?: string
  rate?: number
  pitch?: number
  lang?: string
  /** Force the kernel-side path even when Web Speech API is available. */
  forceIpc?: boolean
}

/** Handle returned by [`startContinuous`]. */
export interface ContinuousHandle {
  stop: () => void
}

let useWebSpeechFlag = true

/** Override the Web Speech preference. Plugin's `activate` calls this
 *  from the live `nexus.audio.useWebSpeech` setting. */
export function setUseWebSpeech(use: boolean): void {
  useWebSpeechFlag = use
}

/** Read the current Web Speech preference (for tests + status pane). */
export function getUseWebSpeech(): boolean {
  return useWebSpeechFlag
}

// ── transcribe ─────────────────────────────────────────────────────────────

/**
 * Capture one utterance from the microphone and return the
 * recognised text. Web Speech API path runs entirely in-browser;
 * MediaRecorder fallback path records WebM/Opus and routes through
 * `com.nexus.audio::transcribe`.
 */
export async function transcribe(
  api: PluginAPI,
  opts: TranscribeOptions = {},
): Promise<TranscribeResult> {
  const support = probeSpeechSupport()
  if (!opts.forceIpc && useWebSpeechFlag && support.stt) {
    const handle = startRecognition({ lang: opts.lang })
    if (handle) {
      const text = await handle.done
      return { text, backend: 'webspeech', language: opts.lang }
    }
  }
  return ipcTranscribe(api, opts)
}

/**
 * Start a continuous recognition session. The handler fires on
 * every result chunk; pass `isFinal=true` when the chunk is a
 * stable transcript (the next chunk begins a fresh utterance).
 * Throws when continuous mode isn't available — caller can degrade
 * to a single-shot [`transcribe`] in that case.
 */
export function startContinuous(
  handler: (text: string, isFinal: boolean) => void,
  opts: { lang?: string } = {},
): ContinuousHandle {
  const recog = createRecognition()
  if (!recog) {
    throw new Error(
      'continuous-mode transcription requires Web Speech API; not available in this webview',
    )
  }
  recog.continuous = true
  recog.interimResults = true
  if (opts.lang) recog.lang = opts.lang
  recog.onresult = (e: unknown) => {
    const ev = e as { results: ArrayLike<ArrayLike<{ transcript: string }> & { isFinal: boolean }> }
    for (let i = 0; i < ev.results.length; i++) {
      const group = ev.results[i]
      const top = group[0]
      if (!top) continue
      handler(top.transcript, group.isFinal)
    }
  }
  recog.onerror = (e: unknown) => {
    const code = (e as { error?: string }).error
    // Continuous sessions occasionally raise 'no-speech' between
    // utterances. Auto-restart for that case; bail for permission /
    // capture errors.
    if (code === 'no-speech') {
      try {
        recog.start()
      } catch {
        // already restarting or stopped — let onend land
      }
    }
  }
  recog.start()
  return {
    stop: () => {
      try {
        recog.stop()
      } catch {
        // ignore
      }
    },
  }
}

async function ipcTranscribe(
  api: PluginAPI,
  opts: TranscribeOptions,
): Promise<TranscribeResult> {
  const { text, backend, language } = await ipcTranscribeWithAudio(api, opts)
  return { text, backend, language }
}

/** Result of [`recordVoiceMemo`] — the transcript plus the raw recorded
 *  bytes, so a caller can save the audio (e.g. as a forge attachment)
 *  alongside the transcript. */
export interface VoiceMemoResult extends TranscribeResult {
  bytes: Uint8Array
  format: 'webm' | 'wav'
}

/**
 * C11 (#364) — capture one utterance AND keep the raw audio bytes,
 * unlike [`transcribe`] which discards them after the IPC round-trip.
 * Always uses the kernel `com.nexus.audio::transcribe` path (never
 * Web Speech) since only the MediaRecorder capture produces bytes a
 * caller can persist as an attachment.
 */
export async function recordVoiceMemo(
  api: PluginAPI,
  opts: TranscribeOptions = {},
): Promise<VoiceMemoResult> {
  const { text, backend, language, bytes, format } = await ipcTranscribeWithAudio(api, opts)
  return { text, backend, language, bytes, format }
}

async function ipcTranscribeWithAudio(
  api: PluginAPI,
  opts: TranscribeOptions,
): Promise<TranscribeResult & { bytes: Uint8Array; format: 'webm' | 'wav' }> {
  const { bytes, format } = await captureAudio()
  const audio_b64 = bytesToBase64(bytes)
  const reply = await api.kernel.invoke<{
    text: string
    language?: string
    backend: string
  }>(AUDIO_PLUGIN_ID, HANDLER_TRANSCRIBE, {
    audio_b64,
    format,
    language: opts.lang ?? null,
  })
  return {
    text: reply.text,
    backend: reply.backend,
    language: reply.language,
    bytes,
    format,
  }
}

/**
 * Capture a single MediaRecorder utterance. Records until the user
 * stops speaking for ~1.5s (silence-detect via VolumeMeter would be
 * ideal; v1 ships a max-duration cap and `stop()` API). Caller stops
 * the capture by calling the returned `stop()`; for `transcribe`,
 * a 30s ceiling caps long recordings so an idle mic doesn't run
 * forever.
 */
async function captureAudio(): Promise<{ bytes: Uint8Array; format: 'webm' | 'wav' }> {
  if (typeof navigator === 'undefined' || !navigator.mediaDevices?.getUserMedia) {
    throw new Error('audio capture requires getUserMedia; not available in this webview')
  }
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
  const mime = pickRecorderMime()
  const rec = new MediaRecorder(stream, mime ? { mimeType: mime } : undefined)
  const chunks: Blob[] = []
  rec.ondataavailable = (e: BlobEvent) => {
    if (e.data && e.data.size > 0) chunks.push(e.data)
  }
  const done = new Promise<Blob>((resolve) => {
    rec.onstop = () => resolve(new Blob(chunks, { type: rec.mimeType || 'audio/webm' }))
  })
  rec.start()
  // Hard cap at 30s so a stuck recorder can't leak the mic stream.
  const cap = setTimeout(() => {
    try {
      rec.stop()
    } catch {
      // ignore
    }
  }, 30_000)
  // Wait for an external stop. The plugin's `transcribe` command
  // sets a 5-second floor by default (controlled by the prompt UI);
  // v1 ships fixed 5s so the flow demos cleanly. Future settings:
  // a configurable recording length + silence-detect.
  await new Promise<void>((resolve) => setTimeout(resolve, 5_000))
  try {
    rec.stop()
  } catch {
    // ignore — onstop will resolve `done`
  }
  clearTimeout(cap)
  const blob = await done
  stream.getTracks().forEach((t) => t.stop())
  const bytes = new Uint8Array(await blob.arrayBuffer())
  const format = blob.type.includes('wav') ? 'wav' : 'webm'
  return { bytes, format }
}

function pickRecorderMime(): string | null {
  if (typeof MediaRecorder === 'undefined') return null
  const candidates = ['audio/webm;codecs=opus', 'audio/webm', 'audio/ogg;codecs=opus']
  for (const m of candidates) {
    if (MediaRecorder.isTypeSupported(m)) return m
  }
  return null
}

// ── synthesize ─────────────────────────────────────────────────────────────

/**
 * Speak `text`. Web Speech path uses SpeechSynthesisUtterance and
 * resolves when the utterance ends. IPC path posts to
 * `com.nexus.audio::synthesize` and plays the returned bytes
 * through an `<audio>` element.
 */
export async function synthesize(
  api: PluginAPI,
  text: string,
  opts: SynthesizeOptions = {},
): Promise<void> {
  const support = probeSpeechSupport()
  if (!opts.forceIpc && useWebSpeechFlag && support.tts) {
    const handle = speak(text, opts)
    if (handle) {
      await handle.done
      return
    }
  }
  await ipcSynthesize(api, text, opts)
}

async function ipcSynthesize(
  api: PluginAPI,
  text: string,
  opts: SynthesizeOptions,
): Promise<void> {
  const reply = await api.kernel.invoke<{
    audio_b64: string
    format: string
    backend: string
  }>(AUDIO_PLUGIN_ID, HANDLER_SYNTHESIZE, {
    text,
    voice: opts.voice ?? null,
    format: 'mp3',
  })
  const bytes = base64ToBytes(reply.audio_b64)
  await playAudioBytes(bytes, reply.format)
}

async function playAudioBytes(bytes: Uint8Array, format: string): Promise<void> {
  if (typeof Audio === 'undefined') {
    throw new Error('audio playback not available in this runtime')
  }
  const mime = format === 'wav' ? 'audio/wav' : format === 'mp3' ? 'audio/mpeg' : `audio/${format}`
  // Copy into a fresh ArrayBuffer-backed Uint8Array so the BlobPart
  // type-check is satisfied across the Uint8Array<ArrayBufferLike>
  // declarations TypeScript ships.
  const buf = new ArrayBuffer(bytes.byteLength)
  new Uint8Array(buf).set(bytes)
  const blob = new Blob([buf], { type: mime })
  const url = URL.createObjectURL(blob)
  try {
    const audio = new Audio(url)
    await new Promise<void>((resolve, reject) => {
      audio.onended = () => resolve()
      audio.onerror = () => reject(new Error('audio playback failed'))
      void audio.play().catch(reject)
    })
  } finally {
    URL.revokeObjectURL(url)
  }
}

// ── helpers ────────────────────────────────────────────────────────────────

function bytesToBase64(bytes: Uint8Array): string {
  let s = ''
  for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]!)
  // btoa is available in Tauri's webview + jsdom + node 18+; the
  // shell only runs in those environments, so no Node-only Buffer
  // fallback is needed here.
  return btoa(s)
}

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

/** Re-export the voice enumeration so settings UIs can list voices. */
export { listVoices }
/** Re-export the support probe so the status indicator can show
 *  whether the plugin is using the in-browser path. */
export { probeSpeechSupport }
export type { RecognitionHandle }
