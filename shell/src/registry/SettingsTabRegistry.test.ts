// shell/src/registry/SettingsTabRegistry.test.ts
//
// OI-01 — plugin-extensible settings tabs.
//
// Coverage:
//   - Manifest-declared tab without a renderer is filtered out of `all()`
//     (no blank rail entry).
//   - Manifest-then-register sequence attaches a renderer and the entry
//     surfaces.
//   - `register()` without prior manifest entry synthesises one.
//   - `all()` sorts: options → core-plugins → community-plugins, then
//     priority asc, then id lexicographic.
//   - `unregister()` drops the entry entirely (metadata + renderer).

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SettingsTabRegistry } from './SettingsTabRegistry.ts'

const noopComponent = () => null

test('manifest-only entry without renderer is hidden from all()', () => {
  const reg = new SettingsTabRegistry()
  reg.registerFromManifest('p.a', { id: 'a.tab', title: 'A Tab' })
  assert.equal(reg.has('a.tab'), true)
  assert.equal(reg.all().length, 0)
})

test('register() after manifest attaches renderer and surfaces entry', () => {
  const reg = new SettingsTabRegistry()
  reg.registerFromManifest('p.a', { id: 'a.tab', title: 'A Tab', icon: 'star' })
  reg.register('p.a', 'a.tab', noopComponent)
  const all = reg.all()
  assert.equal(all.length, 1)
  assert.equal(all[0].id, 'a.tab')
  assert.equal(all[0].title, 'A Tab')
  assert.equal(all[0].icon, 'star')
})

test('register() without prior manifest synthesises an entry', () => {
  const reg = new SettingsTabRegistry()
  reg.register('p.a', 'a.tab', noopComponent, { title: 'A' })
  const all = reg.all()
  assert.equal(all.length, 1)
  assert.equal(all[0].title, 'A')
})

test('all() sorts by group, then priority, then id', () => {
  const reg = new SettingsTabRegistry()
  reg.register('p.a', 'z.opt', noopComponent, { title: 'Z', group: 'options', priority: 10 })
  reg.register('p.b', 'a.community', noopComponent, { title: 'A', group: 'community-plugins', priority: 1 })
  reg.register('p.c', 'b.core', noopComponent, { title: 'B', group: 'core-plugins', priority: 1 })
  reg.register('p.d', 'a.opt', noopComponent, { title: 'A', group: 'options', priority: 10 })
  reg.register('p.e', 'c.opt', noopComponent, { title: 'C', group: 'options', priority: 5 })

  const ids = reg.all().map((t) => t.id)
  assert.deepEqual(ids, ['c.opt', 'a.opt', 'z.opt', 'b.core', 'a.community'])
})

test('unregister() removes the entry entirely', () => {
  const reg = new SettingsTabRegistry()
  reg.registerFromManifest('p.a', { id: 'a.tab', title: 'A' })
  reg.register('p.a', 'a.tab', noopComponent)
  reg.unregister('a.tab')
  assert.equal(reg.has('a.tab'), false)
  assert.equal(reg.all().length, 0)
})

test('getRenderer returns the attached component', () => {
  const reg = new SettingsTabRegistry()
  reg.register('p.a', 'a.tab', noopComponent)
  assert.equal(reg.getRenderer('a.tab'), noopComponent)
  assert.equal(reg.getRenderer('missing'), undefined)
})
