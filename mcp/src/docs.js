// Documentation discovery for the "markdown context protocol" resources.
//
// Every markdown document in the repository is exposed as an MCP resource so a
// client can ground answers in the project's own docs instead of guessing.

import { readFileSync } from 'node:fs'
import { readdir } from 'node:fs/promises'
import { basename, extname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const DEFAULT_REPO_ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)))

/** Directories that never contain project documentation. */
const IGNORED_DIRECTORIES = new Set([
  '.git',
  'node_modules',
  'target',
  'dist',
  'data',
  'deployments',
  'backups',
])

/** Hard cap so a stray generated file cannot flood `resources/list`. */
const MAX_DOCUMENTS = 200
const MAX_DOCUMENT_BYTES = 1024 * 1024

export function repoRoot(configuredRoot = null) {
  return configuredRoot ? resolve(configuredRoot) : DEFAULT_REPO_ROOT
}

/**
 * List markdown files under the repository root.
 *
 * @returns {Promise<Array<{ path: string, absolute: string, title: string }>>}
 */
export async function listMarkdownDocuments(root = repoRoot()) {
  const found = []
  const pending = [root]

  while (pending.length > 0 && found.length < MAX_DOCUMENTS) {
    const directory = pending.pop()
    let entries
    try {
      entries = await readdir(directory, { withFileTypes: true })
    } catch {
      continue
    }
    for (const entry of entries) {
      if (found.length >= MAX_DOCUMENTS) break
      if (entry.name.startsWith('.') || entry.name === 'node_modules') continue
      const absolute = join(directory, entry.name)
      if (entry.isDirectory()) {
        if (IGNORED_DIRECTORIES.has(entry.name)) continue
        pending.push(absolute)
        continue
      }
      if (!entry.isFile() || extname(entry.name).toLowerCase() !== '.md') continue
      const path = relative(root, absolute).split(sep).join('/')
      found.push({ path, absolute, title: titleFor(path) })
    }
  }

  return found.sort((left, right) => {
    const byRank = documentRank(left.path) - documentRank(right.path)
    return byRank !== 0 ? byRank : left.path.localeCompare(right.path)
  })
}

/** Stable presentation order: overview, design docs, MCP guide, operations. */
function documentRank(path) {
  if (path === 'README.md') return 0
  if (path.startsWith('docs/')) return 1
  if (path === 'mcp/README.md') return 2
  if (path.startsWith('deployment/')) return 3
  return 2
}

export function titleFor(path) {
  if (path === 'README.md') return 'Cluster Console README'
  if (path === 'docs/architecture.md') return 'Architecture'
  if (path === 'docs/state.md') return 'Current state'
  if (path === 'docs/todo.md') return 'Roadmap and TODO'
  if (path === 'docs/wisehosting_features_roadmap.md') {
    return 'WiseHosting feature parity roadmap'
  }
  if (path === 'mcp/README.md') return 'MCP integration guide'
  const name = basename(path, '.md').replaceAll(/[-_]/g, ' ')
  return `${name} (${path})`
}

export function resourceUri(path) {
  return `cluster://docs/${path
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/')}`
}

/** Read a known markdown document, re-reading on every request for freshness. */
export function readMarkdownDocument(document) {
  const bytes = readFileSync(document.absolute)
  if (bytes.byteLength > MAX_DOCUMENT_BYTES) {
    throw new Error(`document is larger than ${MAX_DOCUMENT_BYTES} bytes`)
  }
  return bytes.toString('utf8')
}
