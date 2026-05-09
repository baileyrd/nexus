// BL-077 follow-up — unit tests for the WorkspaceEdit applier.

import test from 'node:test'
import assert from 'node:assert/strict'

import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

import {
  applyTextEditsToString,
  applyWorkspaceEdit,
  groupEditsByRelpath,
  uriToRelpath,
  type LspWorkspaceEdit,
} from './workspaceEdit.ts'

// ── uriToRelpath ──────────────────────────────────────────────────────────────

test('uriToRelpath: strips file:// scheme and normalises against forge root', () => {
  assert.equal(
    uriToRelpath('file:///home/me/forge/src/main.rs', '/home/me/forge'),
    'src/main.rs',
  )
})

test('uriToRelpath: trailing slash on forge root is tolerated', () => {
  assert.equal(
    uriToRelpath('file:///home/me/forge/src/main.rs', '/home/me/forge/'),
    'src/main.rs',
  )
})

test('uriToRelpath: returns null when URI is outside the forge root', () => {
  assert.equal(uriToRelpath('file:///etc/passwd', '/home/me/forge'), null)
})

test('uriToRelpath: percent-decodes spaces and unicode', () => {
  assert.equal(
    uriToRelpath('file:///home/me/forge/Some%20File.rs', '/home/me/forge'),
    'Some File.rs',
  )
})

test('uriToRelpath: passes a bare relative path through', () => {
  assert.equal(uriToRelpath('src/main.rs', '/home/me/forge'), 'src/main.rs')
})

test('uriToRelpath: handles Windows file:///C:/ shape', () => {
  assert.equal(
    uriToRelpath('file:///C:/Users/me/forge/src/main.rs', 'C:/Users/me/forge'),
    'src/main.rs',
  )
})

// ── applyTextEditsToString ────────────────────────────────────────────────────

test('applyTextEditsToString: replaces a substring at a single line', () => {
  const before = 'fn old_name() {\n    println!("hi");\n}\n'
  const after = applyTextEditsToString(before, [
    {
      range: { start: { line: 0, character: 3 }, end: { line: 0, character: 11 } },
      newText: 'new_name',
    },
  ])
  assert.equal(after, 'fn new_name() {\n    println!("hi");\n}\n')
})

test('applyTextEditsToString: applies bottom-up so earlier offsets stay valid', () => {
  // Two edits at different offsets — must apply right-to-left.
  const before = 'foo bar baz'
  const after = applyTextEditsToString(before, [
    {
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 3 } },
      newText: 'XX',
    },
    {
      range: { start: { line: 0, character: 8 }, end: { line: 0, character: 11 } },
      newText: 'YYYY',
    },
  ])
  assert.equal(after, 'XX bar YYYY')
})

test('applyTextEditsToString: clamps positions past EOF', () => {
  const before = 'short'
  const after = applyTextEditsToString(before, [
    {
      range: { start: { line: 99, character: 0 }, end: { line: 99, character: 0 } },
      newText: '!',
    },
  ])
  // Edit at past-EOF clamps to total length, so it appends.
  assert.equal(after, 'short!')
})

test('applyTextEditsToString: empty edits is a no-op', () => {
  assert.equal(applyTextEditsToString('hello', []), 'hello')
})

// ── groupEditsByRelpath ───────────────────────────────────────────────────────

test('groupEditsByRelpath: maps every URI to a relpath', () => {
  const edit: LspWorkspaceEdit = {
    changes: {
      'file:///root/src/a.rs': [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          newText: 'X',
        },
      ],
      'file:///root/src/b.rs': [
        {
          range: { start: { line: 1, character: 0 }, end: { line: 1, character: 1 } },
          newText: 'Y',
        },
      ],
    },
  }
  const groups = groupEditsByRelpath(edit, '/root')
  assert.equal(groups.length, 2)
  assert.deepEqual(
    groups.map((g) => g.relpath).sort(),
    ['src/a.rs', 'src/b.rs'],
  )
})

test('groupEditsByRelpath: skips outside-forge URIs and drops empty edit lists', () => {
  const skipped: Array<[string, string]> = []
  const edit: LspWorkspaceEdit = {
    changes: {
      'file:///root/src/a.rs': [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          newText: 'X',
        },
      ],
      'file:///elsewhere/x.rs': [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          newText: 'X',
        },
      ],
      'file:///root/src/empty.rs': [],
    },
  }
  const groups = groupEditsByRelpath(edit, '/root', (uri, reason) => {
    skipped.push([uri, reason])
  })
  assert.equal(groups.length, 1)
  assert.equal(groups[0].relpath, 'src/a.rs')
  assert.equal(skipped.length, 1)
  assert.equal(skipped[0][0], 'file:///elsewhere/x.rs')
})

// ── applyWorkspaceEdit ────────────────────────────────────────────────────────

function makeView(doc: string): EditorView {
  const state = EditorState.create({ doc })
  return new EditorView({ state, parent: undefined })
}

test('applyWorkspaceEdit: routes the active-tab slice through the live CM view', async () => {
  const view = makeView('fn old() {}\n')
  const reads: string[] = []
  const writes: Array<[string, string]> = []
  const result = await applyWorkspaceEdit(
    {
      changes: {
        'file:///root/src/a.rs': [
          {
            range: {
              start: { line: 0, character: 3 },
              end: { line: 0, character: 6 },
            },
            newText: 'newname',
          },
        ],
      },
    },
    {
      forgeRoot: '/root',
      activeView: view,
      activeRelpath: 'src/a.rs',
      readFile: async (p) => {
        reads.push(p)
        return ''
      },
      writeFile: async (p, c) => {
        writes.push([p, c])
      },
    },
  )
  assert.equal(result.liveViewFiles, 1)
  assert.equal(result.storageFiles, 0)
  assert.equal(reads.length, 0, 'never reads via storage when applying live')
  assert.equal(writes.length, 0, 'never writes via storage when applying live')
  assert.equal(view.state.doc.toString(), 'fn newname() {}\n')
})

test('applyWorkspaceEdit: routes non-active slices through storage', async () => {
  const view = makeView('fn a() {}\n')
  const reads: string[] = []
  const writes: Array<[string, string]> = []
  const fakeStorage = new Map<string, string>([
    ['src/b.rs', 'fn old() {}\n'],
  ])
  const result = await applyWorkspaceEdit(
    {
      changes: {
        'file:///root/src/b.rs': [
          {
            range: {
              start: { line: 0, character: 3 },
              end: { line: 0, character: 6 },
            },
            newText: 'newname',
          },
        ],
      },
    },
    {
      forgeRoot: '/root',
      activeView: view,
      activeRelpath: 'src/a.rs', // active is not in the edit set
      readFile: async (p) => {
        reads.push(p)
        return fakeStorage.get(p) ?? ''
      },
      writeFile: async (p, c) => {
        writes.push([p, c])
      },
    },
  )
  assert.equal(result.liveViewFiles, 0)
  assert.equal(result.storageFiles, 1)
  assert.deepEqual(reads, ['src/b.rs'])
  assert.equal(writes.length, 1)
  assert.equal(writes[0][0], 'src/b.rs')
  assert.equal(writes[0][1], 'fn newname() {}\n')
  // The view stays untouched.
  assert.equal(view.state.doc.toString(), 'fn a() {}\n')
})

test('applyWorkspaceEdit: mixed live + storage files in one edit', async () => {
  const view = makeView('use crate::old;\n')
  const writes: Array<[string, string]> = []
  const fakeStorage = new Map<string, string>([
    ['src/lib.rs', 'pub fn old() {}\n'],
    ['src/util.rs', 'use crate::old;\n'],
  ])
  const result = await applyWorkspaceEdit(
    {
      changes: {
        'file:///root/src/main.rs': [
          {
            range: {
              start: { line: 0, character: 11 },
              end: { line: 0, character: 14 },
            },
            newText: 'newname',
          },
        ],
        'file:///root/src/lib.rs': [
          {
            range: {
              start: { line: 0, character: 7 },
              end: { line: 0, character: 10 },
            },
            newText: 'newname',
          },
        ],
        'file:///root/src/util.rs': [
          {
            range: {
              start: { line: 0, character: 11 },
              end: { line: 0, character: 14 },
            },
            newText: 'newname',
          },
        ],
      },
    },
    {
      forgeRoot: '/root',
      activeView: view,
      activeRelpath: 'src/main.rs',
      readFile: async (p) => fakeStorage.get(p) ?? '',
      writeFile: async (p, c) => {
        writes.push([p, c])
      },
    },
  )
  assert.equal(result.liveViewFiles, 1)
  assert.equal(result.storageFiles, 2)
  assert.equal(view.state.doc.toString(), 'use crate::newname;\n')
  const writesByPath = new Map(writes)
  assert.equal(writesByPath.get('src/lib.rs'), 'pub fn newname() {}\n')
  assert.equal(writesByPath.get('src/util.rs'), 'use crate::newname;\n')
})

test('applyWorkspaceEdit: skipped URIs surface in result and are not read', async () => {
  const reads: string[] = []
  const writes: Array<[string, string]> = []
  const result = await applyWorkspaceEdit(
    {
      changes: {
        'file:///elsewhere/x.rs': [
          {
            range: {
              start: { line: 0, character: 0 },
              end: { line: 0, character: 1 },
            },
            newText: 'X',
          },
        ],
      },
    },
    {
      forgeRoot: '/root',
      activeView: null,
      activeRelpath: null,
      readFile: async (p) => {
        reads.push(p)
        return ''
      },
      writeFile: async (p, c) => {
        writes.push([p, c])
      },
    },
  )
  assert.equal(result.liveViewFiles, 0)
  assert.equal(result.storageFiles, 0)
  assert.deepEqual(result.skipped, ['file:///elsewhere/x.rs'])
  assert.equal(reads.length, 0)
  assert.equal(writes.length, 0)
})

test('applyWorkspaceEdit: skips writeFile when the edit is a content-preserving no-op', async () => {
  const writes: Array<[string, string]> = []
  const fakeStorage = new Map<string, string>([['src/a.rs', 'hello']])
  const result = await applyWorkspaceEdit(
    {
      changes: {
        'file:///root/src/a.rs': [
          {
            range: {
              start: { line: 0, character: 0 },
              end: { line: 0, character: 5 },
            },
            newText: 'hello',
          },
        ],
      },
    },
    {
      forgeRoot: '/root',
      activeView: null,
      activeRelpath: null,
      readFile: async (p) => fakeStorage.get(p) ?? '',
      writeFile: async (p, c) => {
        writes.push([p, c])
      },
    },
  )
  // We still count the file as touched in the loop, but writeFile
  // skipped because content didn't change.
  assert.equal(result.storageFiles, 0)
  assert.equal(writes.length, 0)
})
