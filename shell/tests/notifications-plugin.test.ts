// BL-133 follow-up — pure-helper tests for the shell-side
// notifications subscriber. End-to-end render is too heavy for a
// unit test (would need to mount the toast surface and drive the
// kernel bus through happy-dom); these tests pin the helpers that
// translate the bus-event payload shape into the api.notifications
// surface arguments.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  composeToastMessage,
  toastTypeFor,
  type NotificationDeliveredPayload,
} from '../src/plugins/nexus/notifications/index'

function payload(
  partial: Partial<NotificationDeliveredPayload> = {},
): NotificationDeliveredPayload {
  return {
    channel: 'desktop',
    title: 'Nexus',
    message: 'hello',
    ...partial,
  }
}

test('toastTypeFor: defaults to info', () => {
  assert.equal(toastTypeFor(payload()), 'info')
})

test('toastTypeFor: error in title routes to error', () => {
  assert.equal(toastTypeFor(payload({ title: 'AI Error' })), 'error')
  assert.equal(toastTypeFor(payload({ title: 'failed to fetch' })), 'error')
})

test('toastTypeFor: warning in title routes to warning', () => {
  assert.equal(toastTypeFor(payload({ title: 'Warning' })), 'warning')
  assert.equal(toastTypeFor(payload({ title: 'warn: low disk' })), 'warning')
})

test('toastTypeFor: completion words route to success', () => {
  assert.equal(toastTypeFor(payload({ title: 'Build complete' })), 'success')
  assert.equal(toastTypeFor(payload({ title: 'Backup done' })), 'success')
  assert.equal(toastTypeFor(payload({ title: 'Sync success' })), 'success')
})

test('toastTypeFor: ordering is error > warning > success > info', () => {
  // Mixed title — `error` wins over `complete`.
  assert.equal(
    toastTypeFor(payload({ title: 'AI Error: backup did not complete' })),
    'error',
  )
})

test('composeToastMessage: default Nexus title produces bare message', () => {
  assert.equal(composeToastMessage(payload({ title: 'Nexus' })), 'hello')
})

test('composeToastMessage: empty title produces bare message', () => {
  assert.equal(composeToastMessage(payload({ title: '' })), 'hello')
  assert.equal(composeToastMessage(payload({ title: '   ' })), 'hello')
})

test('composeToastMessage: custom title prepends with colon', () => {
  assert.equal(
    composeToastMessage(payload({ title: 'Workflow', message: 'run complete' })),
    'Workflow: run complete',
  )
})

test('composeToastMessage: trims title whitespace', () => {
  assert.equal(
    composeToastMessage(payload({ title: '  Build  ', message: 'ok' })),
    'Build: ok',
  )
})
