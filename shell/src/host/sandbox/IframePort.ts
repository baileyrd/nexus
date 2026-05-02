// shell/src/host/sandbox/IframePort.ts
//
// WI-30d — SandboxPort adapter around an HTMLIFrameElement.
//
// Wave 1 (WI-30b) abstracted the transport behind `SandboxPort` so the
// router could be unit-tested with an in-memory port pair. This file
// closes the loop for production: the iframe spawned by
// `SandboxOrchestrator` communicates with the host via `postMessage`,
// and `IframePort` is the thin adapter that funnels those frames into
// the same `onmessage` slot the router already consumes.
//
// Security boundary (design doc §3, threats 4 + 7):
//   The iframe is spawned with `sandbox="allow-scripts"` but WITHOUT
//   `allow-same-origin`, which gives it a **null origin**. We cannot
//   name that origin from the host side, so `postMessage(..., '*')`
//   is the only workable targetOrigin. The real guard is `event.source
//   === iframe.contentWindow` — every other tab, extension, devtools
//   probe, or injected script lives in a different window object and
//   cannot forge a matching `source`. We verify that identity on every
//   inbound frame before handing it to the router.
//
// What this file is NOT:
//   - The orchestrator. `IframePort` is pure transport; it knows
//     nothing about handshakes, watchdogs, or panel rendering.
//   - A general-purpose postMessage bridge. It binds to exactly one
//     iframe for its entire lifetime; the orchestrator creates a fresh
//     port per plugin instance.

import type { RpcEnvelope } from '@nexus/extension-api'
import { isRpcEnvelope } from '@nexus/extension-api'
import { clientLogger } from '../clientLogger'
import type { SandboxPort } from './router'

/**
 * Minimal contract the port depends on — lets tests inject a mock
 * iframe without needing a real DOM. An HTMLIFrameElement satisfies
 * this trivially.
 */
/**
 * Minimal contract for the iframe's `contentWindow` — a subset of
 * `Window` restricted to the postMessage surface we actually touch.
 * Widening beyond this would force tests to stub every Window member.
 */
export interface ContentWindowLike {
  postMessage(message: unknown, targetOrigin: string): void
}

export interface IframeLike {
  contentWindow: ContentWindowLike | null
}

/**
 * Minimal contract the port depends on for the outer window side —
 * lets tests swap in a stub with `addEventListener` + `removeEventListener`
 * without pulling in jsdom.
 */
export interface WindowLike {
  addEventListener(
    type: 'message',
    listener: (ev: MessageEvent) => void,
  ): void
  removeEventListener(
    type: 'message',
    listener: (ev: MessageEvent) => void,
  ): void
}

export interface IframePortOptions {
  iframe: IframeLike
  /**
   * Window object to attach the message listener to. Defaults to the
   * ambient `window`. Tests inject a stub; production always uses
   * `window`.
   */
  window?: WindowLike
  /**
   * Optional logger for diagnostic output. Defaults to `console.warn`
   * so dropped frames surface in the browser console without requiring
   * the orchestrator to plumb a logger through.
   */
  warn?: (...args: unknown[]) => void
}

export class IframePort implements SandboxPort {
  // `SandboxPort` is modelled on the native `MessagePort` — a single
  // onmessage slot the router writes to at construction time. We
  // forward every matching window-message into that slot.
  public onmessage: ((ev: MessageEvent) => void) | null = null

  private readonly iframe: IframeLike
  private readonly window: WindowLike
  private readonly warn: (...args: unknown[]) => void
  private readonly windowListener: (ev: MessageEvent) => void
  private closed = false

  constructor(opts: IframePortOptions) {
    this.iframe = opts.iframe
    // Fall back to the ambient `window` only when one exists; under
    // plain Node `globalThis.window` is undefined and the caller must
    // supply a stub. The `as unknown as WindowLike` cast avoids a hard
    // dependency on DOM lib types at module load.
    this.window =
      opts.window ??
      ((globalThis as { window?: WindowLike }).window as WindowLike)
    if (!this.window) {
      throw new Error(
        '[IframePort] no Window available — supply `opts.window` in tests',
      )
    }
    this.warn = opts.warn ?? ((...args) => clientLogger.warn('[IframePort]', ...args))

    this.windowListener = (ev: MessageEvent): void => {
      if (this.closed) return
      // ── Identity guard ──────────────────────────────────────────────
      // The only `source` we trust is the iframe's own contentWindow.
      // Any other source — the host's own window re-emitting a message,
      // a devtools probe, a sibling iframe, an extension content script
      // — gets dropped silently. This is the core of the sandbox
      // security model (design doc §3 threat 7).
      if (ev.source !== this.iframe.contentWindow) return

      // Drop non-envelopes cheaply. The router has its own `isRpcEnvelope`
      // check but filtering here keeps noise out of the warn log inside
      // the router (where it would mask genuinely malformed frames from
      // our own guest bootstrap).
      if (!isRpcEnvelope(ev.data)) return

      const listener = this.onmessage
      if (!listener) {
        // Dropped frames before the router attached — expected during
        // the brief window between `new IframePort()` and the router
        // wiring `port.onmessage`. Surface at debug verbosity only.
        return
      }
      try {
        listener(ev)
      } catch (err) {
        this.warn('router message handler threw', err)
      }
    }

    this.window.addEventListener('message', this.windowListener)
  }

  postMessage(message: unknown): void {
    if (this.closed) return
    const win = this.iframe.contentWindow
    if (!win) {
      // Iframe isn't attached yet or has been torn down. Drop the
      // frame rather than throw — the router treats postMessage as
      // fire-and-forget and a pending response will time out normally.
      this.warn('drop frame: iframe has no contentWindow')
      return
    }
    // targetOrigin "*" is the only workable value for a null-origin
    // iframe. Identity is authenticated on the return path via
    // `event.source`; see the header comment.
    win.postMessage(message as RpcEnvelope, '*')
  }

  /**
   * `SandboxPort` models MessagePort semantics; `start()` is a no-op
   * for the iframe path (we start listening at construction time).
   * Kept as a present-but-empty method so the router's optional chain
   * `this.port.start?.()` resolves cleanly.
   */
  start(): void {
    /* no-op — listener is attached in the constructor */
  }

  /** Tear down the window listener. Idempotent. */
  close(): void {
    if (this.closed) return
    this.closed = true
    try {
      this.window.removeEventListener('message', this.windowListener)
    } catch {
      /* best-effort */
    }
    this.onmessage = null
  }
}
