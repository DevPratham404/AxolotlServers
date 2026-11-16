import assert from 'node:assert/strict'
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import { listMarkdownDocuments, resourceUri, titleFor } from '../src/docs.js'

test('discovers markdown documents and skips implementation directories', async () => {
  const root = await mkdtemp(join(tmpdir(), 'mcp-docs-'))
  try {
    await mkdir(join(root, 'docs'), { recursive: true })
    await mkdir(join(root, 'node_modules', 'pkg'), { recursive: true })
    await writeFile(join(root, 'README.md'), '# Root\n')
    await writeFile(join(root, 'docs', 'architecture.md'), '# Architecture\n')
    await writeFile(join(root, 'docs', 'notes.txt'), 'not markdown\n')
    await writeFile(join(root, 'node_modules', 'pkg', 'README.md'), '# Hidden\n')

    const documents = await listMarkdownDocuments(root)
    assert.deepEqual(
      documents.map((document) => document.path),
      ['README.md', 'docs/architecture.md'],
    )
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('builds stable resource URIs with encoded segments', () => {
  assert.equal(resourceUri('docs/architecture.md'), 'cluster://docs/docs/architecture.md')
  assert.equal(resourceUri('docs/my notes.md'), 'cluster://docs/docs/my%20notes.md')
})

test('uses friendly titles for the core documents', () => {
  assert.equal(titleFor('README.md'), 'Cluster Console README')
  assert.equal(titleFor('docs/state.md'), 'Current state')
  assert.equal(
    titleFor('docs/wisehosting_features_roadmap.md'),
    'WiseHosting feature parity roadmap',
  )
  assert.equal(titleFor('docs/custom-guide.md'), 'custom guide (docs/custom-guide.md)')
})
