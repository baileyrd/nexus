/**
 * OI-08 — plugin manifest sanity test.
 *
 * Imports `extensionsTabPlugin` and asserts the manifest shape is
 * intact. A circular-import or evaluation-order bug would cause this
 * import to throw at module-load time; the test catches that
 * regression class.
 *
 * Note: a full ExtensionHost.loadAll smoke would be ideal, but
 * `ExtensionHost` transitively imports the editor view module which
 * has a `.css` side-effect import that Node's test runner can't
 * parse without the Vite plugin. This test exercises the smaller
 * surface that's runtime-relevant: the plugin module evaluates
 * cleanly and exports a well-formed manifest.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { extensionsTabPlugin } from '../src/plugins/nexus/extensionsTab'

test('extensionsTabPlugin module evaluates and exports a valid manifest', () => {
  // If the circular-import regression were back, this import would
  // throw at module load and the test file would fail to load at all.
  assert.ok(extensionsTabPlugin, 'plugin must be defined')
  assert.equal(extensionsTabPlugin.manifest.id, 'nexus.extensionsTab')
  assert.equal(typeof extensionsTabPlugin.activate, 'function')

  const settingsTabs = extensionsTabPlugin.manifest.contributes?.settingsTabs
  assert.ok(settingsTabs, 'manifest must contribute settingsTabs')
  assert.equal(settingsTabs.length, 1)
  assert.equal(settingsTabs[0].id, 'extensions')
  assert.equal(settingsTabs[0].title, 'Extensions')
  assert.equal(settingsTabs[0].group, 'options')
})
