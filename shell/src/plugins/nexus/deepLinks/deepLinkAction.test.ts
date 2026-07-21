// C71 (#424) — unit tests for the pure `nexus://` URL → action parser.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { parseDeepLink } from './deepLinkAction.ts'

test('parseDeepLink: nexus://open?path= parses the path', () => {
  const action = parseDeepLink(new URL('nexus://open?path=notes/todo.md'))
  assert.deepEqual(action, { kind: 'open', path: 'notes/todo.md' })
})

test('parseDeepLink: nexus://open with no path is invalid', () => {
  assert.equal(parseDeepLink(new URL('nexus://open')), null)
})

test('parseDeepLink: nexus://open with an empty path is invalid', () => {
  assert.equal(parseDeepLink(new URL('nexus://open?path=')), null)
})

test('parseDeepLink: nexus://search?q= parses the query', () => {
  const action = parseDeepLink(new URL('nexus://search?q=hello+world'))
  assert.deepEqual(action, { kind: 'search', query: 'hello world' })
})

test('parseDeepLink: nexus://search with an empty query is still valid (clears the search)', () => {
  const action = parseDeepLink(new URL('nexus://search?q='))
  assert.deepEqual(action, { kind: 'search', query: '' })
})

test('parseDeepLink: nexus://search with no q param is invalid', () => {
  assert.equal(parseDeepLink(new URL('nexus://search')), null)
})

test('parseDeepLink: nexus://new?path=&content= parses both', () => {
  const action = parseDeepLink(
    new URL('nexus://new?path=inbox%2Fcapture.md&content=hello%20world'),
  )
  assert.deepEqual(action, {
    kind: 'new',
    path: 'inbox/capture.md',
    content: 'hello world',
  })
})

test('parseDeepLink: nexus://new with no content defaults to an empty string', () => {
  const action = parseDeepLink(new URL('nexus://new?path=a.md'))
  assert.deepEqual(action, { kind: 'new', path: 'a.md', content: '' })
})

test('parseDeepLink: nexus://new with no path is invalid', () => {
  assert.equal(parseDeepLink(new URL('nexus://new?content=hi')), null)
})

test('parseDeepLink: unrecognized action returns null', () => {
  assert.equal(parseDeepLink(new URL('nexus://delete?path=a.md')), null)
})

test('parseDeepLink: action name is case-insensitive', () => {
  const action = parseDeepLink(new URL('nexus://OPEN?path=a.md'))
  assert.deepEqual(action, { kind: 'open', path: 'a.md' })
})
