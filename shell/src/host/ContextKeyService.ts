// src/host/ContextKeyService.ts
// Shared ambient application state.
// Drives command enablement (when-clauses) and conditional rendering.

import { create } from 'zustand'

interface ContextKeyStore {
  keys: Record<string, unknown>
  set: (key: string, value: unknown) => void
  get: (key: string) => unknown
  evaluate: (expression: string) => boolean
  snapshot: () => Record<string, unknown>
}

export const useContextKeyStore = create<ContextKeyStore>((set, get) => ({
  keys: {
    // Shell-level defaults set at startup
    shellReady: false,
    os: detectOS(),
  },

  set: (key, value) =>
    set(s => ({ keys: { ...s.keys, [key]: value } })),

  get: (key) => get().keys[key],

  evaluate: (expression) => {
    if (!expression?.trim()) return true
    return evaluateWhen(expression, get().keys)
  },

  snapshot: () => ({ ...get().keys }),
}))

/** React hook — re-renders when the key changes */
export function useContextKey(key: string): unknown {
  return useContextKeyStore(s => s.keys[key])
}

/** Non-reactive read for use outside React */
export const contextKeyService = {
  set: (key: string, value: unknown) =>
    useContextKeyStore.getState().set(key, value),

  get: (key: string) =>
    useContextKeyStore.getState().get(key),

  evaluate: (expression: string) =>
    useContextKeyStore.getState().evaluate(expression),

  snapshot: () =>
    useContextKeyStore.getState().snapshot(),
}

// ─── OS detection ─────────────────────────────────────────────────────────────

function detectOS(): 'windows' | 'macos' | 'linux' {
  if (typeof navigator === 'undefined') return 'linux'
  const p = navigator.platform.toLowerCase()
  if (p.includes('win'))   return 'windows'
  if (p.includes('mac'))   return 'macos'
  return 'linux'
}

// ─── When-clause evaluator ────────────────────────────────────────────────────

/**
 * Evaluates a simple boolean expression against context keys.
 * Supports: &&, ||, !, ==, !=, parentheses, string/boolean literals.
 *
 * Examples:
 *   'editorFocus'
 *   'editorFocus && !readOnly'
 *   'fileExtension == md'
 *   'sidebarFocus || explorerFocus'
 */
export function evaluateWhen(
  expression: string,
  keys: Record<string, unknown>
): boolean {
  if (!expression?.trim()) return true

  try {
    const tokens = tokenize(expression)
    const result = parseOr(tokens, keys)
    return Boolean(result)
  } catch {
    console.warn(`[ContextKeyService] Failed to evaluate: '${expression}'`)
    return false
  }
}

function tokenize(expr: string): string[] {
  return expr
    .match(/(\w+(?:\.\w+)*|&&|\|\||[!()=!]=?|'[^']*'|"[^"]*")/g) ?? []
}

function parseOr(tokens: string[], keys: Record<string, unknown>): boolean {
  let left = parseAnd(tokens, keys)
  while (tokens[0] === '||') {
    tokens.shift()
    const right = parseAnd(tokens, keys)
    left = left || right
  }
  return left
}

function parseAnd(tokens: string[], keys: Record<string, unknown>): boolean {
  let left = parseUnary(tokens, keys)
  while (tokens[0] === '&&') {
    tokens.shift()
    const right = parseUnary(tokens, keys)
    left = left && right
  }
  return left
}

function parseUnary(tokens: string[], keys: Record<string, unknown>): boolean {
  if (tokens[0] === '!') {
    tokens.shift()
    return !parseUnary(tokens, keys)
  }
  return parsePrimary(tokens, keys)
}

function parsePrimary(tokens: string[], keys: Record<string, unknown>): boolean {
  if (tokens[0] === '(') {
    tokens.shift()
    const result = parseOr(tokens, keys)
    tokens.shift() // ')'
    return result
  }

  const left = tokens.shift() ?? ''
  const leftVal = resolveValue(left, keys)

  // Equality checks
  if (tokens[0] === '==' || tokens[0] === '!=') {
    const op = tokens.shift()
    const right = tokens.shift() ?? ''
    const rightVal = resolveValue(right, keys)
    return op === '==' ? leftVal == rightVal : leftVal != rightVal
  }

  return Boolean(leftVal)
}

function resolveValue(token: string, keys: Record<string, unknown>): unknown {
  if (token === 'true')  return true
  if (token === 'false') return false
  if (token === 'null')  return null
  if (token.startsWith("'") || token.startsWith('"')) {
    return token.slice(1, -1)
  }
  // Number literal
  if (/^\d+(\.\d+)?$/.test(token)) return parseFloat(token)
  // Context key lookup
  return keys[token] ?? false
}
