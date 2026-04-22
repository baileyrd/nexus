// src/types/e2eGlobal.d.ts
// Ambient declaration for the dev-only E2E harness global. Assigned in
// src/main.tsx when both `import.meta.env.DEV` and
// `import.meta.env.VITE_E2E === 'true'`. See shell/e2e/support/app.ts for
// the consumer side. Intentionally typed loosely — the harness already
// narrows via `window as unknown as { ... }` where it dereferences.

export {}

declare global {
  interface Window {
    /**
     * Dev + VITE_E2E-gated hook for the WDIO harness. `undefined` in
     * production builds and in plain `pnpm dev`.
     */
    __nexusShellApi?: {
      kernel: {
        invoke: <T = unknown>(
          pluginId: string,
          commandId: string,
          args?: unknown,
          timeoutMs?: number,
        ) => Promise<T>
        available: () => Promise<boolean>
      }
      commands: {
        execute: (id: string, ...args: unknown[]) => Promise<unknown>
        all: () => unknown[]
      }
      events: {
        emit: (topic: string, payload: unknown) => void
        on: <T = unknown>(topic: string, handler: (payload: T) => void) => () => void
      }
      storage: {
        get: (key: string) => string | null
        set: (key: string, value: string) => void
        delete: (key: string) => void
      }
      registry: unknown
    }
  }

  interface ImportMetaEnv {
    readonly VITE_E2E?: string
  }
}
