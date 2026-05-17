// BL-113 follow-up — pure-helper tests for LaunchConfig.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  seedDefaults,
  buildLaunchOpts,
} from '../src/plugins/nexus/debugger/LaunchConfig'

test('seedDefaults: empty schema returns empty object', () => {
  assert.deepEqual(seedDefaults(null), {})
  assert.deepEqual(seedDefaults({}), {})
  assert.deepEqual(seedDefaults({ properties: {} }), {})
})

test('seedDefaults: uses explicit schema defaults when provided', () => {
  const out = seedDefaults({
    properties: {
      program: { type: 'string', default: '/usr/bin/python' },
      stop_on_entry: { type: 'boolean', default: true },
    },
  })
  assert.equal(out.program, '/usr/bin/python')
  assert.equal(out.stop_on_entry, true)
})

test('seedDefaults: falls back to type-appropriate empty', () => {
  const out = seedDefaults({
    properties: {
      program: { type: 'string' },
      args: { type: 'array', items: { type: 'string' } },
      stop_on_entry: { type: 'boolean' },
      port: { type: 'integer' },
    },
  })
  assert.equal(out.program, '')
  assert.deepEqual(out.args, [])
  assert.equal(out.stop_on_entry, false)
  assert.equal(out.port, 0)
})

test('buildLaunchOpts: hoists program / args / cwd / stop_on_entry', () => {
  const opts = buildLaunchOpts('python', {
    program: '/home/me/app.py',
    args: ['--verbose', '--port=8080'],
    cwd: '/home/me',
    stop_on_entry: true,
  })
  assert.equal(opts.adapter, 'python')
  assert.equal(opts.program, '/home/me/app.py')
  assert.deepEqual(opts.args, ['--verbose', '--port=8080'])
  assert.equal(opts.cwd, '/home/me')
  assert.equal(opts.stop_on_entry, true)
  assert.equal(opts.extra, undefined)
})

test('buildLaunchOpts: routes unknown keys into extra', () => {
  const opts = buildLaunchOpts('python', {
    program: 'x.py',
    justMyCode: true,
    pythonPath: '/usr/bin/python3',
  })
  assert.equal(opts.program, 'x.py')
  assert.deepEqual(opts.extra, { justMyCode: true, pythonPath: '/usr/bin/python3' })
})

test('buildLaunchOpts: coerces program to string', () => {
  const opts = buildLaunchOpts('python', { program: 42 as unknown as string })
  assert.equal(opts.program, '42')
})

test('buildLaunchOpts: accepts stopOnEntry camelCase alongside snake_case', () => {
  const opts = buildLaunchOpts('python', { program: 'x.py', stopOnEntry: true })
  assert.equal(opts.stop_on_entry, true)
})

test('buildLaunchOpts: drops empty cwd', () => {
  const opts = buildLaunchOpts('python', { program: 'x.py', cwd: '' })
  assert.equal(opts.cwd, undefined)
})

test('buildLaunchOpts: defaults program to empty when missing', () => {
  const opts = buildLaunchOpts('python', {})
  assert.equal(opts.program, '')
})
