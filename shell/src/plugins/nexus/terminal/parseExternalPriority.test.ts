// shell/src/plugins/nexus/terminal/parseExternalPriority.test.ts
//
// BL-059 follow-up — pin the splitting + canonicalisation rules of
// the `terminal.externalPriority` setting parser without driving
// React.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { parseExternalPriority } from './SavedCommandsView.tsx'

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
