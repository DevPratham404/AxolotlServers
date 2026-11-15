import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

import { cn } from '../../lib/cn'
import { ApiError, serverApi } from '../../lib/api'
import type { DirectoryEntry, FileContent } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { EmptyState } from '../../components/ui/Feedback'
import { GlassCard } from '../../components/ui/GlassCard'

const TEXT_EXTENSIONS = new Set([
  'yml',
  'yaml',
  'json',
  'properties',
  'txt',
  'conf',
  'toml',
  'mcmeta',
  'md',
  'log',
  'sh',
])

interface FileManagerPanelProps {
  serverId: string
}

interface EditorState {
  file: FileContent
  content: string
  saving: boolean
  error: string | null
  notice: string | null
}

type CreateKind = 'file' | 'folder'

type TransferKind = 'rename' | 'copy' | 'move'

interface TransferState {
  kind: TransferKind
  entry: DirectoryEntry
  destination: string
}

type SortKey = 'name' | 'size'
type SortDirection = 'asc' | 'desc'

export function FileManagerPanel({ serverId }: FileManagerPanelProps) {
  const [path, setPath] = useState('')
  const [entries, setEntries] = useState<DirectoryEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [opening, setOpening] = useState<string | null>(null)
  const [downloading, setDownloading] = useState<string | null>(null)
  const [uploading, setUploading] = useState(false)
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<DirectoryEntry | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [createKind, setCreateKind] = useState<CreateKind | null>(null)
  const [creating, setCreating] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)
  const [sort, setSort] = useState<{ key: SortKey; direction: SortDirection }>({
    key: 'name',
    direction: 'asc',
  })
  const [menuFor, setMenuFor] = useState<string | null>(null)
  const [transfer, setTransfer] = useState<TransferState | null>(null)
  const [transferring, setTransferring] = useState(false)
  const [transferError, setTransferError] = useState<string | null>(null)
  const requestId = useRef(0)
  const uploadInput = useRef<HTMLInputElement>(null)

  const load = useCallback(
    async (nextPath: string) => {
      const id = ++requestId.current
      setLoading(true)
      setError(null)
      try {
        const listing = await serverApi.listDirectory(serverId, nextPath)
        if (id === requestId.current) setEntries(listing)
      } catch (caught) {
        if (id !== requestId.current) return
        setEntries([])
        setError(caught instanceof ApiError ? caught.message : 'Could not list this directory.')
      } finally {
        if (id === requestId.current) setLoading(false)
      }
    },
    [serverId],
  )

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    void load(path)
    return () => {
      requestId.current += 1
    }
  }, [load, path])

  async function openFile(entry: DirectoryEntry) {
    const filePath = joinPath(path, entry.name)
    setOpening(entry.name)
    setError(null)
    try {
      const file = await serverApi.readFile(serverId, filePath)
      setEditor({ file, content: file.content, saving: false, error: null, notice: null })
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : `Could not open ${entry.name}.`)
    } finally {
      setOpening(null)
    }
  }

  async function downloadEntry(entry: DirectoryEntry) {
    const filePath = joinPath(path, entry.name)
    setDownloading(entry.name)
    setError(null)
    try {
      const blob = await serverApi.downloadFile(serverId, filePath)
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = entry.name
      document.body.appendChild(anchor)
      anchor.click()
      anchor.remove()
      window.setTimeout(() => URL.revokeObjectURL(url), 1500)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : `Could not download ${entry.name}.`)
    } finally {
      setDownloading(null)
    }
  }

  async function uploadFiles(files: FileList | null) {
    if (!files || files.length === 0) return
    setUploading(true)
    setError(null)
    try {
      for (const file of Array.from(files)) {
        const data = await file.arrayBuffer()
        await serverApi.uploadFile(serverId, joinPath(path, file.name), data)
      }
      await load(path)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not upload the selected file.')
    } finally {
      setUploading(false)
      if (uploadInput.current) uploadInput.current.value = ''
    }
  }

  async function createEntry(name: string) {
    if (!createKind) return
    const target = joinPath(path, name.trim())
    setCreating(true)
    setCreateError(null)
    try {
      if (createKind === 'folder') {
        await serverApi.createDirectory(serverId, target)
      } else {
        await serverApi.writeFile(serverId, target, '')
      }
      const kind = createKind
      setCreateKind(null)
      await load(path)
      if (kind === 'file') {
        const file = await serverApi.readFile(serverId, target)
        setEditor({ file, content: file.content, saving: false, error: null, notice: null })
      }
    } catch (caught) {
      setCreateError(caught instanceof ApiError ? caught.message : `Could not create ${target}.`)
    } finally {
      setCreating(false)
    }
  }

  async function saveEditor() {
    if (!editor) return
    setEditor({ ...editor, saving: true, error: null, notice: null })
    try {
      const file = await serverApi.writeFile(
        serverId,
        editor.file.path,
        editor.content,
        editor.file.hash,
      )
      setEditor({
        file,
        content: file.content,
        saving: false,
        error: null,
        notice: 'Saved to disk.',
      })
      await load(path)
    } catch (caught) {
      const message =
        caught instanceof ApiError && caught.status === 409
          ? 'This file changed on disk since you opened it. Close the editor and reopen it to merge.'
          : caught instanceof ApiError
            ? caught.message
            : 'Could not save this file.'
      setEditor((current) =>
        current ? { ...current, saving: false, error: message, notice: null } : current,
      )
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return
    setDeleting(true)
    setError(null)
    try {
      await serverApi.deleteFile(serverId, joinPath(path, deleteTarget.name))
      setDeleteTarget(null)
      await load(path)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : `Could not delete ${deleteTarget.name}.`)
      setDeleteTarget(null)
    } finally {
      setDeleting(false)
    }
  }

  function startTransfer(kind: TransferKind, entry: DirectoryEntry) {
    const destination =
      kind === 'rename'
        ? entry.name
        : kind === 'copy'
          ? joinPath(path, copyName(entry.name))
          : joinPath(path, entry.name)
    setMenuFor(null)
    setTransferError(null)
    setTransfer({ kind, entry, destination })
  }

  async function submitTransfer(value: string) {
    if (!transfer) return
    const source = joinPath(path, transfer.entry.name)
    const destination = transfer.kind === 'rename' ? joinPath(path, value) : value

    if (destination === source) {
      setTransferError('Choose a different destination.')
      return
    }

    setTransferring(true)
    setTransferError(null)
    try {
      if (transfer.kind === 'copy') {
        await serverApi.copyFile(serverId, source, destination)
      } else {
        await serverApi.renameFile(serverId, source, destination)
      }
      setTransfer(null)
      await load(path)
    } catch (caught) {
      setTransferError(
        caught instanceof ApiError ? caught.message : `Could not ${transfer.kind} this entry.`,
      )
    } finally {
      setTransferring(false)
    }
  }

  function toggleSort(key: SortKey) {
    setSort((current) =>
      current.key === key
        ? { key, direction: current.direction === 'asc' ? 'desc' : 'asc' }
        : { key, direction: 'asc' },
    )
  }

  const sortedEntries = useMemo(() => {
    const factor = sort.direction === 'asc' ? 1 : -1
    return [...entries].sort((left, right) => {
      if (left.is_dir !== right.is_dir) return left.is_dir ? -1 : 1
      const primary =
        sort.key === 'name'
          ? left.name.localeCompare(right.name)
          : left.size === right.size
            ? left.name.localeCompare(right.name)
            : left.size - right.size
      return primary * factor
    })
  }, [entries, sort])

  const segments = path ? path.split('/') : []

  return (
    <div className="flex flex-col gap-4">
      <GlassCard className="animate-rise sticky top-3 z-20 overflow-hidden">
        <div className="flex flex-wrap items-center justify-between gap-3 px-5 py-4">
          <div className="flex min-w-0 items-center gap-3">
            <span className="grid size-9 shrink-0 place-items-center rounded-xl border border-neon-cyan/25 bg-neon-cyan/10 text-neon-cyan">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" className="size-4">
                <path d="M3.5 7.5h6l2 2h9v9.5h-17z" strokeLinejoin="round" />
                <path d="M3.5 9.5V5h6l2 2h9v2.5" strokeLinejoin="round" />
              </svg>
            </span>
            <div className="min-w-0">
              <h2 className="text-sm font-semibold">Server files</h2>
              <p className="mt-0.5 truncate font-mono text-[10px] text-text-dim">
                /data{path ? `/${path}` : ''}
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <input
              ref={uploadInput}
              type="file"
              multiple
              className="hidden"
              data-testid="files-upload-input"
              onChange={(event) => void uploadFiles(event.target.files)}
            />
            <Button
              variant="secondary"
              size="sm"
              disabled={uploading}
              loading={uploading}
              onClick={() => uploadInput.current?.click()}
              data-testid="files-upload"
            >
              Upload File
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setCreateError(null)
                setCreateKind('file')
              }}
              data-testid="files-new-file"
            >
              New File
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setCreateError(null)
                setCreateKind('folder')
              }}
              data-testid="files-new-folder"
            >
              New Folder
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void load(path)}
              loading={loading}
              data-testid="files-refresh"
            >
              Refresh
            </Button>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-1 border-t border-white/[0.07] px-5 py-2.5 font-mono text-[11px]">
          <Breadcrumb onClick={() => setPath('')} active={path === ''}>
            data
          </Breadcrumb>
          {segments.map((segment, index) => {
            const nextPath = segments.slice(0, index + 1).join('/')
            return (
              <span key={nextPath} className="flex items-center gap-1">
                <span className="text-text-dim">/</span>
                <Breadcrumb
                  onClick={() => setPath(nextPath)}
                  active={index === segments.length - 1}
                >
                  {segment}
                </Breadcrumb>
              </span>
            )
          })}
        </div>

        {error ? (
          <p
            role="alert"
            className="border-t border-neon-rose/20 bg-neon-rose/[0.07] px-5 py-2.5 text-[11px] text-neon-rose"
          >
            {error}
          </p>
        ) : null}
      </GlassCard>

      <GlassCard className="animate-rise overflow-hidden">
        {loading && entries.length === 0 ? (
          <div className="space-y-2 p-4" aria-hidden="true">
            {Array.from({ length: 6 }).map((_, index) => (
              <div key={index} className="h-11 animate-pulse rounded-lg bg-white/[0.04]" />
            ))}
          </div>
        ) : entries.length === 0 ? (
          <EmptyState
            title="This folder is empty"
            description={
              path === ''
                ? 'Start the server once to generate its data volume, then refresh.'
                : 'No files or folders here yet.'
            }
            action={
              path ? (
                <Button variant="secondary" onClick={() => setPath(parentPath(path))}>
                  Go up
                </Button>
              ) : undefined
            }
          />
        ) : (
          <table className="w-full text-left text-xs">
            <thead>
              <tr className="border-b border-white/[0.07] font-mono text-[9px] tracking-[0.12em] text-text-dim uppercase">
                <th scope="col" className="px-5 py-2.5 font-normal">
                  <SortButton
                    label="Name"
                    active={sort.key === 'name'}
                    direction={sort.direction}
                    onClick={() => toggleSort('name')}
                  />
                </th>
                <th scope="col" className="px-3 py-2.5 text-right font-normal">
                  <SortButton
                    label="Size"
                    align="right"
                    active={sort.key === 'size'}
                    direction={sort.direction}
                    onClick={() => toggleSort('size')}
                  />
                </th>
                <th scope="col" className="w-64 px-5 py-2.5 text-right font-normal">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/[0.05]">
              {sortedEntries.map((entry) => {
                const textFile = !entry.is_dir && isTextFile(entry.name)
                return (
                  <tr key={entry.name} className="group transition hover:bg-white/[0.03]">
                    <td className="px-5 py-1.5">
                      <button
                        type="button"
                        onClick={() =>
                          entry.is_dir ? setPath(joinPath(path, entry.name)) : textFile ? void openFile(entry) : undefined
                        }
                        disabled={!entry.is_dir && !textFile}
                        className={cn(
                          'flex w-full min-w-0 items-center gap-2.5 rounded-md px-1.5 py-1.5 text-left transition',
                          (entry.is_dir || textFile) &&
                            'hover:text-text-primary focus-visible:ring-2 focus-visible:ring-neon-emerald/40 focus-visible:outline-none',
                          !entry.is_dir && !textFile && 'cursor-default',
                        )}
                        title={
                          entry.is_dir
                            ? `Open ${entry.name}`
                            : textFile
                              ? `Edit ${entry.name}`
                              : 'Preview is not available for this file type'
                        }
                      >
                        <FileGlyph directory={entry.is_dir} />
                        <span className="min-w-0 truncate font-mono">{entry.name}</span>
                        {opening === entry.name ? (
                          <span className="ml-auto font-mono text-[9px] text-text-dim">opening…</span>
                        ) : textFile ? (
                          <span className="ml-auto rounded border border-neon-cyan/20 bg-neon-cyan/10 px-1.5 py-0.5 font-mono text-[8px] tracking-[0.08em] text-neon-cyan uppercase">
                            text
                          </span>
                        ) : null}
                      </button>
                    </td>
                    <td className="px-3 py-1.5 text-right font-mono text-[10px] text-text-dim">
                      {entry.is_dir ? '—' : formatBytes(entry.size)}
                    </td>
                    <td className="px-5 py-1.5 text-right">
                      <div className="flex items-center justify-end gap-1">
                        {!entry.is_dir && textFile ? (
                          <button
                            type="button"
                            onClick={() => void openFile(entry)}
                            className="rounded-md border border-transparent px-2 py-1 font-mono text-[9px] tracking-[0.08em] text-text-dim uppercase transition hover:border-neon-cyan/30 hover:bg-neon-cyan/10 hover:text-neon-cyan focus-visible:ring-2 focus-visible:ring-neon-cyan/40 focus-visible:outline-none"
                          >
                            Edit
                          </button>
                        ) : null}
                        {!entry.is_dir ? (
                          <button
                            type="button"
                            disabled={downloading === entry.name}
                            onClick={() => void downloadEntry(entry)}
                            className="rounded-md border border-transparent px-2 py-1 font-mono text-[9px] tracking-[0.08em] text-text-dim uppercase transition hover:border-neon-emerald/30 hover:bg-neon-emerald/10 hover:text-neon-emerald focus-visible:ring-2 focus-visible:ring-neon-emerald/40 focus-visible:outline-none disabled:opacity-50"
                          >
                            {downloading === entry.name ? '…' : 'Download'}
                          </button>
                        ) : null}
                        <button
                          type="button"
                          onClick={() => startTransfer('rename', entry)}
                          className="rounded-md border border-transparent px-2 py-1 font-mono text-[9px] tracking-[0.08em] text-text-dim uppercase transition hover:border-neon-amber/30 hover:bg-neon-amber/10 hover:text-neon-amber focus-visible:ring-2 focus-visible:ring-neon-amber/40 focus-visible:outline-none"
                        >
                          Rename
                        </button>
                        <RowMenu
                          open={menuFor === entry.name}
                          onToggle={() =>
                            setMenuFor((current) => (current === entry.name ? null : entry.name))
                          }
                          onClose={() => setMenuFor(null)}
                        >
                          <MenuItem
                            label="Copy"
                            hint="Duplicate here or elsewhere"
                            onClick={() => startTransfer('copy', entry)}
                          />
                          <MenuItem
                            label="Move"
                            hint="Rename or change folder"
                            onClick={() => startTransfer('move', entry)}
                          />
                          <MenuItem
                            label="Delete"
                            hint="Remove permanently"
                            danger
                            onClick={() => {
                              setMenuFor(null)
                              setDeleteTarget(entry)
                            }}
                          />
                        </RowMenu>
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </GlassCard>

      <p className="px-1 text-[11px] text-text-dim">
        Editable text formats: {[...TEXT_EXTENSIONS].join(', ')}. Uploads are binary-safe (up to 100
        MiB). Rename, copy and move are sandboxed: paths cannot escape the server volume.
      </p>

      {editor ? (
        <FileEditor
          editor={editor}
          onChange={(content) => setEditor((current) => (current ? { ...current, content, notice: null } : current))}
          onSave={() => void saveEditor()}
          onClose={() => setEditor(null)}
        />
      ) : null}

      {deleteTarget ? (
        <ConfirmDelete
          name={deleteTarget.name}
          deleting={deleting}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void confirmDelete()}
        />
      ) : null}

      {createKind ? (
        <NamePrompt
          kind={createKind}
          busy={creating}
          error={createError}
          basePath={path}
          onCancel={() => {
            setCreateKind(null)
            setCreateError(null)
          }}
          onCreate={(name) => void createEntry(name)}
        />
      ) : null}

      {transfer ? (
        <TransferPrompt
          kind={transfer.kind}
          entry={transfer.entry}
          destination={transfer.destination}
          busy={transferring}
          error={transferError}
          onCancel={() => {
            setTransfer(null)
            setTransferError(null)
          }}
          onSubmit={(value) => void submitTransfer(value)}
        />
      ) : null}
    </div>
  )
}

function Breadcrumb({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: ReactNode
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={active}
      className={cn(
        'rounded px-1 py-0.5 transition',
        active
          ? 'text-text-primary'
          : 'text-text-dim hover:bg-white/5 hover:text-text-muted',
      )}
    >
      {children}
    </button>
  )
}

function FileGlyph({ directory }: { directory: boolean }) {
  return directory ? (
    <svg viewBox="0 0 20 20" className="size-4 shrink-0 text-neon-amber" fill="currentColor" aria-hidden="true">
      <path d="M2 5.5A1.5 1.5 0 0 1 3.5 4h4l1.7 1.7H16.5A1.5 1.5 0 0 1 18 7.2v7.3a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 2 14.5z" />
    </svg>
  ) : (
    <svg viewBox="0 0 20 20" className="size-4 shrink-0 text-neon-cyan/80" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
      <path d="M5 2.8h6l4 4v10.4H5z" strokeLinejoin="round" />
      <path d="M11 2.8v4h4" strokeLinejoin="round" />
    </svg>
  )
}

function FileEditor({
  editor,
  onChange,
  onSave,
  onClose,
}: {
  editor: EditorState
  onChange: (content: string) => void
  onSave: () => void
  onClose: () => void
}) {
  const dirty = editor.content !== editor.file.content

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape' && !editor.saving) {
        if (!dirty || window.confirm('Discard your unsaved changes?')) onClose()
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
        event.preventDefault()
        if (dirty && !editor.saving) onSave()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [dirty, editor.saving, onClose, onSave])

  return createPortal(
    <div
      className="animate-fade-in fixed inset-0 z-50 grid place-items-center bg-void/80 px-4 py-6 backdrop-blur-sm"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget && !editor.saving) {
          if (!dirty || window.confirm('Discard your unsaved changes?')) onClose()
        }
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="file-editor-title"
        data-testid="file-editor"
        className="animate-rise flex max-h-[90vh] w-full max-w-4xl flex-col overflow-hidden rounded-glass border border-white/10 bg-abyss/95 shadow-glass backdrop-blur-2xl"
      >
        <header className="flex flex-wrap items-start justify-between gap-3 border-b border-white/[0.07] px-5 py-4">
          <div className="min-w-0">
            <h2 id="file-editor-title" className="truncate font-mono text-sm font-semibold">
              {editor.file.path}
            </h2>
            <p className="mt-1 font-mono text-[10px] text-text-dim">
              hash {editor.file.hash.slice(0, 12)}
              {dirty ? ' · unsaved changes' : ''}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={onClose} disabled={editor.saving}>
              Close
            </Button>
            <Button
              variant="primary"
              size="sm"
              loading={editor.saving}
              disabled={!dirty}
              onClick={onSave}
              data-testid="file-editor-save"
            >
              Save file
            </Button>
          </div>
        </header>

        {editor.error ? (
          <p role="alert" className="border-b border-neon-rose/20 bg-neon-rose/[0.07] px-5 py-2.5 text-[11px] text-neon-rose">
            {editor.error}
          </p>
        ) : editor.notice ? (
          <p className="border-b border-neon-emerald/20 bg-neon-emerald/[0.07] px-5 py-2.5 text-[11px] text-neon-emerald">
            {editor.notice}
          </p>
        ) : null}

        <textarea
          value={editor.content}
          onChange={(event) => onChange(event.target.value)}
          spellCheck={false}
          aria-label={`Contents of ${editor.file.path}`}
          className="min-h-[48vh] flex-1 resize-none bg-black/35 px-5 py-4 font-mono text-xs leading-relaxed text-text-primary outline-none focus:bg-black/45"
        />

        <footer className="flex items-center justify-between gap-3 border-t border-white/[0.07] px-5 py-2.5 font-mono text-[10px] text-text-dim">
          <span>{editor.content.length.toLocaleString()} characters</span>
          <span>Ctrl/Cmd+S saves · Esc closes</span>
        </footer>
      </div>
    </div>,
    document.body,
  )
}

function NamePrompt({
  kind,
  busy,
  error,
  basePath,
  onCancel,
  onCreate,
}: {
  kind: CreateKind
  busy: boolean
  error: string | null
  basePath: string
  onCancel: () => void
  onCreate: (name: string) => void
}) {
  const [name, setName] = useState('')
  const trimmed = name.trim()
  const invalid =
    trimmed.length === 0 ||
    trimmed === '.' ||
    trimmed === '..' ||
    trimmed.includes('/') ||
    trimmed.includes('\\')
  const noun = kind === 'folder' ? 'folder' : 'file'

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape' && !busy) onCancel()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [busy, onCancel])

  return createPortal(
    <div
      className="animate-fade-in fixed inset-0 z-50 grid place-items-center bg-void/80 px-4 backdrop-blur-sm"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel()
      }}
    >
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby="create-entry-title"
        className="animate-rise w-full max-w-sm rounded-glass border border-white/10 bg-abyss/95 p-6 shadow-glass backdrop-blur-2xl"
        onSubmit={(event) => {
          event.preventDefault()
          if (!invalid && !busy) onCreate(trimmed)
        }}
      >
        <h2 id="create-entry-title" className="text-base font-semibold">
          New {noun}
        </h2>
        <p className="mt-1 text-[11px] text-text-dim">
          Created inside <span className="font-mono text-text-muted">/data{basePath ? `/${basePath}` : ''}</span>.
        </p>

        <input
          autoFocus
          value={name}
          disabled={busy}
          aria-label={`${noun} name`}
          placeholder={kind === 'folder' ? 'plugins' : 'config.yml'}
          onChange={(event) => setName(event.target.value)}
          className="mt-4 h-10 w-full rounded-xl border border-white/10 bg-black/30 px-3 font-mono text-xs text-text-primary outline-none transition focus:border-neon-emerald/50 focus:ring-2 focus:ring-neon-emerald/15"
        />

        {error ? (
          <p role="alert" className="mt-3 text-[11px] text-neon-rose">
            {error}
          </p>
        ) : invalid && name.length > 0 ? (
          <p className="mt-3 text-[11px] text-neon-amber">
            Names cannot be empty or contain slashes.
          </p>
        ) : null}

        <div className="mt-6 flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy} disabled={invalid}>
            Create {noun}
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  )
}

function ConfirmDelete({
  name,
  deleting,
  onCancel,
  onConfirm,
}: {
  name: string
  deleting: boolean
  onCancel: () => void
  onConfirm: () => void
}) {
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape' && !deleting) onCancel()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [deleting, onCancel])

  return createPortal(
    <div
      className="animate-fade-in fixed inset-0 z-50 grid place-items-center bg-void/80 px-4 backdrop-blur-sm"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget && !deleting) onCancel()
      }}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="delete-file-title"
        className="animate-rise w-full max-w-sm rounded-glass border border-white/10 bg-abyss/95 p-6 shadow-glass backdrop-blur-2xl"
      >
        <h2 id="delete-file-title" className="text-base font-semibold">
          Delete this file?
        </h2>
        <p className="mt-2 text-sm leading-relaxed text-text-muted">
          <span className="break-all font-mono text-text-primary">{name}</span> will be removed from
          the server data volume. This cannot be undone.
        </p>
        <div className="mt-6 flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel} disabled={deleting}>
            Cancel
          </Button>
          <Button variant="danger" onClick={onConfirm} loading={deleting}>
            Delete file
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  )
}

function SortButton({
  label,
  align = 'left',
  active,
  direction,
  onClick,
}: {
  label: string
  align?: 'left' | 'right'
  active: boolean
  direction: SortDirection
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'inline-flex items-center gap-1 rounded px-1 py-0.5 tracking-[0.12em] uppercase transition hover:text-text-muted focus-visible:ring-2 focus-visible:ring-neon-emerald/40 focus-visible:outline-none',
        align === 'right' && 'flex-row-reverse',
        active ? 'text-text-primary' : 'text-text-dim',
      )}
      title={`Sort by ${label.toLowerCase()} ${active && direction === 'asc' ? 'descending' : 'ascending'}`}
    >
      {label}
      <span className={cn('text-[8px]', active ? 'opacity-100' : 'opacity-30')}>
        {active && direction === 'desc' ? '▼' : '▲'}
      </span>
    </button>
  )
}

function RowMenu({
  open,
  onToggle,
  onClose,
  children,
}: {
  open: boolean
  onToggle: () => void
  onClose: () => void
  children: ReactNode
}) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    function onPointerDown(event: MouseEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) onClose()
    }
    document.addEventListener('mousedown', onPointerDown)
    return () => document.removeEventListener('mousedown', onPointerDown)
  }, [onClose, open])

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={onToggle}
        className={cn(
          'rounded-md border px-2 py-1 font-mono text-[9px] tracking-[0.08em] uppercase transition focus-visible:ring-2 focus-visible:ring-neon-cyan/40 focus-visible:outline-none',
          open
            ? 'border-neon-cyan/30 bg-neon-cyan/10 text-neon-cyan'
            : 'border-transparent text-text-dim hover:border-white/15 hover:bg-white/5 hover:text-text-muted',
        )}
      >
        More
      </button>
      {open ? (
        <div
          role="menu"
          className="absolute top-full right-0 z-30 mt-1 w-48 overflow-hidden rounded-xl border border-white/10 bg-abyss/95 p-1 text-left shadow-glass backdrop-blur-xl"
        >
          {children}
        </div>
      ) : null}
    </div>
  )
}

function MenuItem({
  label,
  hint,
  danger,
  onClick,
}: {
  label: string
  hint: string
  danger?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className={cn(
        'flex w-full flex-col rounded-lg px-2.5 py-2 text-left transition',
        danger
          ? 'hover:bg-neon-rose/10 hover:text-neon-rose'
          : 'hover:bg-white/[0.06] hover:text-text-primary',
      )}
    >
      <span className="font-mono text-[10px] tracking-[0.08em] uppercase">{label}</span>
      <span className="mt-0.5 text-[10px] text-text-dim">{hint}</span>
    </button>
  )
}

function TransferPrompt({
  kind,
  entry,
  destination,
  busy,
  error,
  onCancel,
  onSubmit,
}: {
  kind: TransferKind
  entry: DirectoryEntry
  destination: string
  busy: boolean
  error: string | null
  onCancel: () => void
  onSubmit: (value: string) => void
}) {
  const [value, setValue] = useState(destination)
  const trimmed = value.trim()
  const single = kind === 'rename'
  const invalid =
    trimmed.length === 0 ||
    trimmed.startsWith('/') ||
    trimmed.startsWith('\\') ||
    (single && (trimmed === '.' || trimmed === '..' || /[\\/]/.test(trimmed))) ||
    (!single && trimmed.split('/').some((part) => part === '' || part === '.' || part === '..'))
  const title =
    kind === 'rename' ? `Rename ${entry.name}` : kind === 'copy' ? `Copy ${entry.name}` : `Move ${entry.name}`

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape' && !busy) onCancel()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [busy, onCancel])

  return createPortal(
    <div
      className="animate-fade-in fixed inset-0 z-50 grid place-items-center bg-void/80 px-4 backdrop-blur-sm"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel()
      }}
    >
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby="transfer-title"
        className="animate-rise w-full max-w-md rounded-glass border border-white/10 bg-abyss/95 p-6 shadow-glass backdrop-blur-2xl"
        onSubmit={(event) => {
          event.preventDefault()
          if (!invalid && !busy) onSubmit(trimmed)
        }}
      >
        <h2 id="transfer-title" className="text-base font-semibold">
          {title}
        </h2>
        <p className="mt-1 text-[11px] leading-relaxed text-text-dim">
          {single
            ? 'Enter a new name. The entry stays in the current folder.'
            : 'Enter a destination path relative to /data. Existing destinations are never overwritten.'}
        </p>

        <label className="mt-4 block">
          <span className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
            {single ? 'New name' : 'Destination path'}
          </span>
          <input
            autoFocus
            value={value}
            disabled={busy}
            aria-label={single ? 'New name' : 'Destination path'}
            placeholder={single ? entry.name : 'plugins/example.jar'}
            onChange={(event) => setValue(event.target.value)}
            className="mt-2 h-10 w-full rounded-xl border border-white/10 bg-black/30 px-3 font-mono text-xs text-text-primary outline-none transition focus:border-neon-emerald/50 focus:ring-2 focus:ring-neon-emerald/15"
          />
        </label>

        {error ? (
          <p role="alert" className="mt-3 text-[11px] text-neon-rose">
            {error}
          </p>
        ) : invalid && value.length > 0 ? (
          <p className="mt-3 text-[11px] text-neon-amber">
            {single
              ? 'Names cannot be empty or contain slashes.'
              : 'Use a relative path without empty, “.” or “..” segments.'}
          </p>
        ) : null}

        <div className="mt-6 flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy} disabled={invalid}>
            {kind === 'rename' ? 'Rename' : kind === 'copy' ? 'Copy' : 'Move'}
          </Button>
        </div>
      </form>
    </div>,
    document.body,
  )
}

function copyName(name: string): string {
  const dot = name.lastIndexOf('.')
  if (dot <= 0) return `${name}-copy`
  return `${name.slice(0, dot)}-copy${name.slice(dot)}`
}

function joinPath(base: string, name: string): string {
  return base ? `${base}/${name}` : name
}

function parentPath(path: string): string {
  const index = path.lastIndexOf('/')
  return index === -1 ? '' : path.slice(0, index)
}

function isTextFile(name: string): boolean {
  const dot = name.lastIndexOf('.')
  if (dot < 0) return false
  return TEXT_EXTENSIONS.has(name.slice(dot + 1).toLowerCase())
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB']
  let value = bytes / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`
}
