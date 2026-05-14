// shell/src/plugins/nexus/audio/speechApi.ts
//
// BL-118 — thin wrappers around the browser's Web Speech API.
//
// `webkitSpeechRecognition` (STT) and `SpeechSynthesisUtterance` /
// `speechSynthesis` (TTS) are the legacy-prefixed but widely-shipped
// surfaces every Chromium-derived webview ships (Chrome, Edge,
// modern Safari, Tauri's WebKit). The unprefixed
// `SpeechRecognition` is a draft alias — we feature-detect both so
// non-Chromium runtimes that ship the unprefixed name still work.
//
// Everything here is browser-only. The plugin loads only inside the
// Tauri webview; pure-Node tests stub the globals manually (see
// `speechApi.test.ts`).

/// Polyfill type for non-Chromium platforms — TypeScript's lib.dom
/// only declares the `webkitSpeechRecognition` form.
type MinimalSpeechRecognition = EventTarget & {
  continuous: boolean
  interimResults: boolean
  lang: string
  start: () => void
  stop: () => void
  abort: () => void
  onresult: ((e: unknown) => void) | null
  onerror: ((e: unknown) => void) | null
  onend: ((e: unknown) => void) | null
  onstart: ((e: unknown) => void) | null
}

type SpeechRecognitionCtor = new () => MinimalSpeechRecognition

declare global {
  interface Window {
    webkitSpeechRecognition?: SpeechRecognitionCtor
    SpeechRecognition?: SpeechRecognitionCtor
  }
}

/**
 * Browser-feature snapshot taken at plugin activate. Both flags can
 * be false on platforms without speech APIs (e.g. headless test
 * runners, embedded webviews without speech-dispatcher); callers
 * fall back to the kernel-side `com.nexus.audio::transcribe` /
 * `synthesize` path when so.
 */
export interface SpeechSupport {
  stt: boolean
  tts: boolean
}

/** Probe the live `window` for STT + TTS support. */
export function probeSpeechSupport(): SpeechSupport {
  const w: Window | undefined = typeof window === 'undefined' ? undefined : window
  if (!w) return { stt: false, tts: false }
  const recogCtor = w.SpeechRecognition ?? w.webkitSpeechRecognition
  const synth = typeof w.speechSynthesis === 'object' && w.speechSynthesis !== null
  return { stt: !!recogCtor, tts: synth }
}

/** Construct a fresh SpeechRecognition instance, or `null` when unsupported. */
export function createRecognition(): MinimalSpeechRecognition | null {
  if (typeof window === 'undefined') return null
  const Ctor = window.SpeechRecognition ?? window.webkitSpeechRecognition
  if (!Ctor) return null
  return new Ctor()
}

// ── STT ────────────────────────────────────────────────────────────────────

/** Options for [`recognizeOnce`]. */
export interface RecognizeOptions {
  /** BCP-47 language tag (e.g. `"en-US"`). Defaults to the browser's locale. */
  lang?: string
  /**
   * Emit interim (live, low-confidence) transcripts to `onInterim`
   * while the user is still speaking. Final result is always passed
   * back through the resolved Promise once the user pauses.
   */
  interim?: boolean
  /**
   * Per-utterance interim chunk callback. Fires only when `interim`
   * is true; safe to omit even then.
   */
  onInterim?: (text: string) => void
  /**
   * Manual cancel hook. Resolving the returned object's `cancel()`
   * stops recognition; the promise resolves with the partial text
   * gathered so far.
   */
}

/** Handle returned by `startRecognition` — call `cancel()` to stop early. */
export interface RecognitionHandle {
  /** Resolves to the final transcript when the speaker pauses or
   *  `cancel()` runs. Rejects on a SpeechRecognition `error` event. */
  done: Promise<string>
  /** Stop the recognition session early. Idempotent. */
  cancel(): void
}

/**
 * One-shot speech recognition: start, capture the next utterance,
 * resolve when the user pauses. Returns `null` immediately when the
 * browser has no SpeechRecognition implementation — caller should
 * fall back to MediaRecorder + IPC in that case.
 */
export function startRecognition(opts: RecognizeOptions = {}): RecognitionHandle | null {
  const recog = createRecognition()
  if (!recog) return null
  recog.continuous = false
  recog.interimResults = !!opts.interim
  if (opts.lang) recog.lang = opts.lang

  let cancelled = false
  let finalText = ''

  const done = new Promise<string>((resolve, reject) => {
    recog.onresult = (e: unknown) => {
      const ev = e as { results: ArrayLike<ArrayLike<{ transcript: string }> & { isFinal: boolean }> }
      for (let i = 0; i < ev.results.length; i++) {
        const group = ev.results[i]
        const top = group[0]
        if (!top) continue
        if (group.isFinal) {
          finalText += top.transcript
        } else if (opts.interim && opts.onInterim) {
          opts.onInterim(top.transcript)
        }
      }
    }
    recog.onerror = (e: unknown) => {
      const errEv = e as { error?: string; message?: string }
      // 'no-speech' / 'aborted' are normal user-driven outcomes —
      // resolve with whatever final text we caught rather than
      // raising. 'not-allowed' (mic permission) and 'audio-capture'
      // (no mic) are real failures.
      const code = errEv.error ?? ''
      if (cancelled || code === 'aborted' || code === 'no-speech') {
        resolve(finalText.trim())
        return
      }
      reject(new Error(`speech recognition error: ${code || errEv.message || 'unknown'}`))
    }
    recog.onend = () => {
      // `end` fires after `result(isFinal=true)` for a clean utterance,
      // or after `error` / `abort`. Resolve here unconditionally with
      // the accumulated text — the error path has already rejected if
      // it ran first.
      resolve(finalText.trim())
    }
  })

  try {
    recog.start()
  } catch (err) {
    return {
      done: Promise.reject(
        new Error(`speech recognition failed to start: ${(err as Error).message}`),
      ),
      cancel: () => {},
    }
  }

  return {
    done,
    cancel: () => {
      if (cancelled) return
      cancelled = true
      try {
        recog.stop()
      } catch {
        // Some implementations throw when `stop` is called before
        // `start` lands; the `onend` handler will still fire.
      }
    },
  }
}

// ── TTS ────────────────────────────────────────────────────────────────────

/** Options for [`speak`]. */
export interface SpeakOptions {
  /** Voice URI (from `listVoices()`); defaults to the system default. */
  voice?: string
  /** Speaking rate, 0.1–10. Defaults to 1.0. */
  rate?: number
  /** Pitch, 0–2. Defaults to 1.0. */
  pitch?: number
  /** BCP-47 language tag override. */
  lang?: string
}

/** Snapshot of one platform-provided voice. */
export interface VoiceInfo {
  voiceURI: string
  name: string
  lang: string
  default: boolean
}

/** Enumerate the platform's available TTS voices, or `[]` when TTS is unsupported. */
export function listVoices(): VoiceInfo[] {
  if (typeof window === 'undefined' || !window.speechSynthesis) return []
  const voices = window.speechSynthesis.getVoices() ?? []
  return voices.map((v) => ({
    voiceURI: v.voiceURI,
    name: v.name,
    lang: v.lang,
    default: v.default,
  }))
}

/**
 * Speak `text` via the platform TTS engine. Resolves when the
 * utterance finishes (or `cancel()` runs); rejects on an `error`
 * event. Returns `null` when the platform has no `speechSynthesis`
 * — caller should fall back to the kernel-side `synthesize` IPC.
 */
export function speak(text: string, opts: SpeakOptions = {}): {
  done: Promise<void>
  cancel: () => void
} | null {
  if (typeof window === 'undefined' || !window.speechSynthesis) return null
  const synth = window.speechSynthesis
  const u = new SpeechSynthesisUtterance(text)
  if (opts.lang) u.lang = opts.lang
  if (typeof opts.rate === 'number') u.rate = clamp(opts.rate, 0.1, 10)
  if (typeof opts.pitch === 'number') u.pitch = clamp(opts.pitch, 0, 2)
  if (opts.voice) {
    const v = synth.getVoices().find((vv) => vv.voiceURI === opts.voice)
    if (v) u.voice = v
  }

  const done = new Promise<void>((resolve, reject) => {
    u.onend = () => resolve()
    u.onerror = (e: SpeechSynthesisErrorEvent) => {
      // 'interrupted' / 'canceled' fire when the user dismisses;
      // resolve cleanly in that case so the caller's await doesn't
      // raise on the happy-path cancel.
      if (e.error === 'interrupted' || e.error === 'canceled') {
        resolve()
        return
      }
      reject(new Error(`speech synthesis error: ${e.error}`))
    }
  })

  synth.speak(u)

  return {
    done,
    cancel: () => {
      try {
        synth.cancel()
      } catch {
        // ignore — some engines reject cancel after speak completes
      }
    },
  }
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, n))
}
