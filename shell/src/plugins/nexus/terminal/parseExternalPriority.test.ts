// shell/src/plugins/nexus/terminal/parseExternalPriority.test.ts
//
// BL-059 follow-up — pin the splitting + canonicalisation rules of
// the `terminal.externalPriority` setting parser without driving
// React.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  envVarsToText,
  parseEnvVars,
  parseExternalPriority,
} from './SavedCommandsView.tsx'

test('parseExternalPriority: empty string → empty list', () => {
  assert.deepEqual(parseExternalPriority(''), [])
  assert.deepEqual(parseExternalPriority('   '), [])
})

test('parseExternalPriority: comma-separated canonicalises kebab to snake', () => {
  assert.deepEqual(
    parseExternalPriority('wezterm, gnome-terminal, alacritty'),
    ['wezterm', 'gnome_terminal', 'alacritty'],
  )
})

test('parseExternalPriority: whitespace-separated also accepted', () => {
  assert.deepEqual(
    parseExternalPriority('kitty alacritty xterm'),
    ['kitty', 'alacritty', 'xterm'],
  )
})

test('parseExternalPriority: unknown tokens silently dropped', () => {
  assert.deepEqual(
    parseExternalPriority('wezterm, frobinator, kitty'),
    ['wezterm', 'kitty'],
  )
})

test('parseExternalPriority: duplicates collapsed, first wins', () => {
  assert.deepEqual(
    parseExternalPriority('wezterm, kitty, wezterm'),
    ['wezterm', 'kitty'],
  )
})

test('parseExternalPriority: case-insensitive', () => {
  assert.deepEqual(
    parseExternalPriority('WezTerm, KITTY'),
    ['wezterm', 'kitty'],
  )
})

// ── BL-059 follow-up — env_vars textarea round-trip ─────────────────

test('envVarsToText: sorts keys and emits KEY=VALUE per line', () => {
  assert.equal(envVarsToText({ FOO: '1', BAR: 'x' }), 'BAR=x\nFOO=1')
})

test('envVarsToText: empty map → empty string', () => {
  assert.equal(envVarsToText({}), '')
})

test('parseEnvVars: round-trips a typical multi-line block', () => {
  const text = 'NODE_ENV=development\nDEBUG=1'
  assert.deepEqual(parseEnvVars(text), { NODE_ENV: 'development', DEBUG: '1' })
})

test('parseEnvVars: tolerates blank lines and comments', () => {
  const text = '# comment\n\n  \nFOO=bar\n# trailing'
  assert.deepEqual(parseEnvVars(text), { FOO: 'bar' })
})

test('parseEnvVars: keeps embedded `=` characters in the value', () => {
  // Bash treats anything after the first `=` as the literal value;
  // the parser matches that — `URL=https://x?q=a=b` survives.
  assert.deepEqual(
    parseEnvVars('URL=https://x?q=a=b'),
    { URL: 'https://x?q=a=b' },
  )
})

test('parseEnvVars: drops malformed lines (no `=`, leading `=`, blank key)', () => {
  const text = 'no_equals_here\n=value-only\n   =foo\nGOOD=ok'
  assert.deepEqual(parseEnvVars(text), { GOOD: 'ok' })
})

test('round-trip: parseEnvVars(envVarsToText(x)) === x', () => {
  const original = { ALPHA: '1', BETA: '2', GAMMA: 'three=four' }
  assert.deepEqual(parseEnvVars(envVarsToText(original)), original)
})

test('parseExternalPriority: every documented tag is recognised', () => {
  // Smoke that the user-facing list in the settings description maps
  // 1:1 with the parser's allowlist.
  const advertised = [
    'iterm2',
    'wezterm',
    'ghostty',
    'kitty',
    'alacritty',
    'windows-terminal',
    'gnome-terminal',
    'konsole',
    'xfce4-terminal',
    'mac-terminal',
    'x-terminal-emulator',
    'xterm',
  ]
  const parsed = parseExternalPriority(advertised.join(','))
  assert.equal(parsed.length, advertised.length)
})
