// Vitest setup: wire jest-dom matchers and register global mocks for
// the Tauri IPC surface. Tests can't run a real Tauri host, so every
// panel under test goes through these stubs.

import "@testing-library/jest-dom/vitest";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

// ── Tauri invoke mock ───────────────────────────────────────────────
//
// Tests configure per-command responses through `setInvokeHandlers`
// (see `src/test/tauri.ts`). The mock here just dispatches.

type InvokeHandler = (args?: Record<string, unknown>) => unknown | Promise<unknown>;

const invokeRegistry: Record<string, InvokeHandler> = {};
const invokeCalls: { command: string; args?: Record<string, unknown> }[] = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    invokeCalls.push({ command, args });
    const handler = invokeRegistry[command];
    if (!handler) {
      throw new Error(`[test] no mock registered for invoke("${command}")`);
    }
    return handler(args);
  }),
}));

// ── Tauri event listen mock ─────────────────────────────────────────

type EventHandler = (ev: { payload: unknown }) => void;
const eventHandlers: Record<string, EventHandler[]> = {};

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (topic: string, handler: EventHandler) => {
    (eventHandlers[topic] ??= []).push(handler);
    return () => {
      eventHandlers[topic] = (eventHandlers[topic] ?? []).filter((h) => h !== handler);
    };
  }),
}));

// ── Test-facing helpers ─────────────────────────────────────────────
//
// Attached to `globalThis` so tests can import them without threading
// the registry through module state.

declare global {
  // eslint-disable-next-line no-var
  var __setInvokeHandlers: (h: Record<string, InvokeHandler>) => void;
  // eslint-disable-next-line no-var
  var __invokeCalls: typeof invokeCalls;
  // eslint-disable-next-line no-var
  var __fireTauriEvent: (topic: string, payload: unknown) => void;
}

globalThis.__setInvokeHandlers = (h) => {
  for (const key of Object.keys(invokeRegistry)) delete invokeRegistry[key];
  Object.assign(invokeRegistry, h);
};
globalThis.__invokeCalls = invokeCalls;
globalThis.__fireTauriEvent = (topic, payload) => {
  for (const h of eventHandlers[topic] ?? []) h({ payload });
};

afterEach(() => {
  cleanup();
  for (const key of Object.keys(invokeRegistry)) delete invokeRegistry[key];
  invokeCalls.length = 0;
  for (const key of Object.keys(eventHandlers)) delete eventHandlers[key];
});
