import { useEffect, useMemo, useState } from 'react'
import type { FormEvent } from 'react'

import { cn } from '../../lib/cn'
import { ApiError, serverApi } from '../../lib/api'
import { SERVER_TYPES, serverTypeMeta } from '../../lib/servers'
import type { ServerType, ServerView } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { ErrorBanner } from '../../components/ui/Feedback'
import { Spinner } from '../../components/ui/Spinner'
import { TextField } from '../../components/ui/TextField'

const STEP_LABELS = ['Source', 'Configuration', 'Import'] as const
const MIN_RAM_GB = 1
const MAX_RAM_GB = 16
const MIN_CPU = 0.5
const MAX_CPU = 4
const VERSION_SUGGESTIONS = [
  'LATEST',
  'SNAPSHOT',
  '1.21.4',
  '1.21.1',
  '1.20.6',
  '1.20.4',
  '1.20.1',
  '1.19.4',
] as const
const PINNED_VERSION_FALLBACKS: Partial<Record<ServerType, string[]>> = {
  arclight: ['26.1.2'],
  catserver: ['1.18.2', '1.16.5', '1.12.2'],
  mohist: ['1.21.4', '1.21.1', '1.20.1', '1.19.4', '1.18.2', '1.16.5', '1.12.2'],
  magma: ['1.21.1'],
}

type SourceMode = 'zip' | 'path'

interface ImportServerWizardProps {
  onClose: () => void
  onImported: (server: ServerView) => void
}

export function ImportServerWizard({ onClose, onImported }: ImportServerWizardProps) {
  const [step, setStep] = useState(0)
  const [sourceMode, setSourceMode] = useState<SourceMode>('zip')
  const [file, setFile] = useState<File | null>(null)
  const [localPath, setLocalPath] = useState('')
  const [serverType, setServerType] = useState<ServerType>('paper')
  const [gameVersion, setGameVersion] = useState('LATEST')
  const [versionSuggestions, setVersionSuggestions] = useState<string[]>([
    ...VERSION_SUGGESTIONS,
  ])
  const [ramGb, setRamGb] = useState(4)
  const [cpu, setCpu] = useState(2)
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const meta = useMemo(() => serverTypeMeta(serverType), [serverType])
  const trimmedName = name.trim()
  const trimmedVersion = gameVersion.trim()
  const trimmedPath = localPath.trim()
  const nameValid = trimmedName.length >= 1 && trimmedName.length <= 64
  const versionValid = /^[A-Za-z0-9._+-]{1,32}$/.test(trimmedVersion)
  const pathValid = trimmedPath.startsWith('/') && !/[\0\r\n]/.test(trimmedPath)
  const sourceValid = sourceMode === 'zip' ? file !== null : pathValid

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy) onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [busy, onClose])

  useEffect(() => {
    let cancelled = false
    void serverApi
      .runtimeVersions(serverType)
      .then((versions) => {
        if (cancelled) return
        const available = versions.length > 0 ? versions : ['LATEST']
        setVersionSuggestions(available)
        setGameVersion((current) =>
          available.includes(current) ? current : available[0],
        )
      })
      .catch(() => {
        if (cancelled) return
        const available = PINNED_VERSION_FALLBACKS[serverType] ?? [...VERSION_SUGGESTIONS]
        setVersionSuggestions(available)
        setGameVersion((current) =>
          available.includes(current) ? current : available[0],
        )
      })
    return () => {
      cancelled = true
    }
  }, [serverType])

  function selectFile(next: File | null) {
    setFile(next)
    setError(null)
    if (next && trimmedName.length === 0) {
      setName(next.name.replace(/\.zip$/i, '').slice(0, 64))
    }
  }

  function updatePath(value: string) {
    setLocalPath(value)
    setError(null)
    if (trimmedName.length === 0) {
      const base = value
        .split('/')
        .filter(Boolean)
        .pop()
      if (base) setName(base.slice(0, 64))
    }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (busy) return

    if (step === 0) {
      if (sourceValid) setStep(1)
      return
    }
    if (step === 1) {
      if (nameValid && versionValid) setStep(2)
      return
    }
    if (!sourceValid || !nameValid || !versionValid) return

    setBusy(true)
    setError(null)
    const form = new FormData()
    form.append('name', trimmedName)
    form.append('server_type', serverType)
    form.append('game_version', trimmedVersion)
    form.append('ram_mb', String(ramGb * 1024))
    form.append('cpu_limit', String(cpu))
    if (sourceMode === 'zip' && file) {
      form.append('file', file, file.name)
    } else {
      form.append('local_path', trimmedPath)
    }

    try {
      const imported = await serverApi.importServer(form)
      onImported(imported)
    } catch (caught) {
      setError(
        caught instanceof ApiError ? caught.message : 'Could not import the server.',
      )
      setBusy(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-black/65 px-4 py-8 backdrop-blur-md sm:py-12">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="import-wizard-title"
        className="glass-deep animate-rise relative w-full max-w-2xl rounded-glass shadow-glass"
      >
        <header className="flex items-start justify-between gap-4 border-b border-white/[0.07] px-6 py-5 sm:px-7">
          <div>
            <p className="font-mono text-[10px] tracking-[0.22em] text-neon-cyan uppercase">
              import server
            </p>
            <h2 id="import-wizard-title" className="mt-1 text-xl font-semibold tracking-tight">
              Bring an existing world on board
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Close import wizard"
            className="grid size-9 place-items-center rounded-lg text-text-dim transition hover:bg-white/5 hover:text-text-primary disabled:opacity-40"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" className="size-4">
              <path d="m6 6 12 12M18 6 6 18" />
            </svg>
          </button>
        </header>

        <div className="flex items-center gap-2 px-6 pt-5 sm:px-7">
          {STEP_LABELS.map((label, index) => (
            <div key={label} className="flex flex-1 items-center gap-2">
              <span
                className={cn(
                  'grid size-6 shrink-0 place-items-center rounded-full border font-mono text-[10px] transition',
                  index < step
                    ? 'border-neon-cyan/40 bg-neon-cyan/15 text-neon-cyan'
                    : index === step
                      ? 'border-neon-cyan bg-neon-cyan text-void'
                      : 'border-white/10 text-text-dim',
                )}
              >
                {index < step ? '✓' : index + 1}
              </span>
              <span
                className={cn(
                  'hidden text-[11px] sm:block',
                  index === step ? 'text-text-primary' : 'text-text-dim',
                )}
              >
                {label}
              </span>
              {index < STEP_LABELS.length - 1 ? (
                <span className={cn('h-px flex-1', index < step ? 'bg-neon-cyan/30' : 'bg-white/10')} />
              ) : null}
            </div>
          ))}
        </div>

        <form onSubmit={submit} className="px-6 py-6 sm:px-7">
          {error ? <ErrorBanner className="mb-5">{error}</ErrorBanner> : null}

          {step === 0 ? (
            <div className="animate-rise">
              <p className="mb-4 text-xs text-text-muted">
                Upload a ZIP of the server directory, or point at an existing folder on this VPS.
                The data is copied into a new isolated volume.
              </p>

              <div className="grid gap-2.5 sm:grid-cols-2">
                <SourceCard
                  active={sourceMode === 'zip'}
                  title="Upload ZIP"
                  detail="Stream a .zip from your computer"
                  onClick={() => setSourceMode('zip')}
                  icon={<path d="M12 16V4M7 9l5-5 5 5M4 16v3h16v-3" />}
                />
                <SourceCard
                  active={sourceMode === 'path'}
                  title="Local Path"
                  detail="Absolute path already on the host"
                  onClick={() => setSourceMode('path')}
                  icon={<path d="M3 6h7l2 2h9v11H3z" />}
                />
              </div>

              {sourceMode === 'zip' ? (
                <div className="mt-5">
                  <label className="block cursor-pointer rounded-2xl border border-dashed border-white/15 bg-black/20 px-5 py-8 text-center transition hover:border-neon-cyan/40 hover:bg-neon-cyan/[0.04]">
                    <input
                      type="file"
                      accept=".zip,application/zip,application/x-zip-compressed"
                      className="sr-only"
                      disabled={busy}
                      onChange={(event) => selectFile(event.target.files?.[0] ?? null)}
                    />
                    <span className="mx-auto grid size-11 place-items-center rounded-xl border border-white/10 bg-black/30 text-neon-cyan">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="size-5">
                        <path d="M12 16V4M7 9l5-5 5 5M4 16v3h16v-3" />
                      </svg>
                    </span>
                    <span className="mt-3 block text-sm font-medium">
                      {file ? file.name : 'Choose a server ZIP'}
                    </span>
                    <span className="mt-1 block font-mono text-[10px] text-text-dim">
                      {file
                        ? `${(file.size / 1024 / 1024).toFixed(1)} MB selected`
                        : 'up to 10 GB · extracted into the new data volume'}
                    </span>
                  </label>
                  {file ? (
                    <button
                      type="button"
                      onClick={() => selectFile(null)}
                      className="mt-2 text-[11px] text-text-dim transition hover:text-neon-rose"
                    >
                      Remove selection
                    </button>
                  ) : null}
                </div>
              ) : (
                <div className="mt-5">
                  <TextField
                    label="Absolute host path"
                    value={localPath}
                    onChange={(event) => updatePath(event.target.value)}
                    placeholder="/srv/minecraft/survival"
                    hint="Read by the daemon on this machine; the source directory is not modified."
                    error={
                      localPath.length > 0 && !pathValid
                        ? 'Use an absolute path starting with / and no line breaks.'
                        : undefined
                    }
                    autoFocus
                  />
                </div>
              )}

              <div className="mt-5 rounded-xl border border-neon-amber/20 bg-neon-amber/[0.06] px-3.5 py-2.5 text-[11px] leading-relaxed text-neon-amber">
                The daemon copies files into <span className="font-mono">/data</span> but does not
                infer the runtime. Choose the type and game version in the next step.
              </div>
            </div>
          ) : null}

          {step === 1 ? (
            <div className="animate-rise space-y-6">
              <TextField
                label="Server name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="e.g. survival"
                hint={`${trimmedName.length}/64`}
                error={name.length > 0 && !nameValid ? 'Use 1–64 characters.' : undefined}
                autoFocus
                required
              />

              <div>
                <p className="mb-2 text-sm font-medium">Runtime</p>
                <div className="grid gap-2 sm:grid-cols-3">
                  {SERVER_TYPES.map((option) => {
                    const selected = option.value === serverType
                    return (
                      <button
                        key={option.value}
                        type="button"
                        onClick={() => setServerType(option.value)}
                        aria-pressed={selected}
                        className={cn(
                          'rounded-xl border p-3 text-left transition-all duration-150',
                          selected
                            ? 'border-neon-cyan/40 bg-neon-cyan/[0.07] ring-1 ring-neon-cyan/30'
                            : 'border-white/[0.08] bg-black/20 hover:border-white/15 hover:bg-white/[0.04]',
                        )}
                      >
                        <span className="flex items-center justify-between">
                          <span
                            className={cn(
                              'font-mono text-xs font-semibold',
                              option.accent,
                            )}
                          >
                            {option.monogram}
                          </span>
                          <span className="font-mono text-[8px] tracking-[0.12em] text-text-dim uppercase">
                            {option.tagline}
                          </span>
                        </span>
                        <span className="mt-2 block text-sm font-semibold">{option.label}</span>
                      </button>
                    )
                  })}
                </div>
              </div>

              <TextField
                label="Game version"
                value={gameVersion}
                onChange={(event) => setGameVersion(event.target.value)}
                placeholder="LATEST"
                list="import-game-version-suggestions"
                hint="Pinned in the container's VERSION env"
                error={
                  gameVersion.length > 0 && !versionValid
                    ? 'Use LATEST, SNAPSHOT, or a version like 1.21.1.'
                    : undefined
                }
              />
              <datalist id="import-game-version-suggestions">
                {versionSuggestions.map((version) => (
                  <option key={version} value={version} />
                ))}
              </datalist>

              <div>
                <div className="flex items-baseline justify-between">
                  <label htmlFor="import-ram" className="text-sm font-medium">
                    Memory limit
                  </label>
                  <span className="font-mono text-sm text-neon-emerald">{ramGb} GB</span>
                </div>
                <input
                  id="import-ram"
                  type="range"
                  min={MIN_RAM_GB}
                  max={MAX_RAM_GB}
                  step={1}
                  value={ramGb}
                  onChange={(event) => setRamGb(Number(event.target.value))}
                  className="range-neon mt-3"
                  style={sliderFill(((ramGb - MIN_RAM_GB) / (MAX_RAM_GB - MIN_RAM_GB)) * 100)}
                />
              </div>

              <div>
                <div className="flex items-baseline justify-between">
                  <label htmlFor="import-cpu" className="text-sm font-medium">
                    CPU allocation
                  </label>
                  <span className="font-mono text-sm text-neon-cyan">
                    {cpu.toFixed(1)} vCPU
                  </span>
                </div>
                <input
                  id="import-cpu"
                  type="range"
                  min={MIN_CPU}
                  max={MAX_CPU}
                  step={0.5}
                  value={cpu}
                  onChange={(event) => setCpu(Number(event.target.value))}
                  className="range-neon mt-3"
                  style={sliderFill(((cpu - MIN_CPU) / (MAX_CPU - MIN_CPU)) * 100)}
                />
              </div>
            </div>
          ) : null}

          {step === 2 ? (
            <div className="animate-rise space-y-4">
              <div className="rounded-2xl border border-white/[0.07] bg-black/20 p-4">
                <p className="font-mono text-[10px] tracking-[0.18em] text-text-dim uppercase">
                  review
                </p>
                <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2.5 text-sm">
                  <div className="flex justify-between gap-3">
                    <dt className="text-text-dim">Source</dt>
                    <dd className="max-w-[16rem] truncate font-mono text-[11px]">
                      {sourceMode === 'zip' ? (file?.name ?? '—') : trimmedPath}
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-text-dim">Runtime</dt>
                    <dd className={meta.accent}>{meta.label}</dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-text-dim">Version</dt>
                    <dd className="truncate font-mono">{trimmedVersion || '—'}</dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-text-dim">Memory</dt>
                    <dd className="font-mono">{ramGb} GB</dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-text-dim">CPU</dt>
                    <dd className="font-mono">{cpu.toFixed(1)} vCPU</dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-text-dim">Game port</dt>
                    <dd className="font-mono">auto-assigned</dd>
                  </div>
                </dl>
                <p className="mt-3 border-t border-white/[0.06] pt-3 text-[11px] leading-relaxed text-text-muted">
                  The panel creates a stopped container and copies the files into its volume. The
                  source folder is left untouched; existing files in the new volume are never
                  overwritten.
                </p>
              </div>

              {busy ? (
                <div
                  role="status"
                  aria-live="polite"
                  className="flex items-start gap-3 rounded-2xl border border-neon-cyan/25 bg-neon-cyan/[0.07] px-4 py-4"
                >
                  <Spinner className="mt-0.5 size-5 text-neon-cyan" />
                  <div>
                    <p className="text-sm font-medium text-neon-cyan">
                      Importing and extracting server (this may take a while)…
                    </p>
                    <p className="mt-1 font-mono text-[10px] leading-relaxed text-text-dim">
                      Keep this tab open. Large worlds can take several minutes to transfer and
                      unpack.
                    </p>
                  </div>
                </div>
              ) : null}
            </div>
          ) : null}

          <footer className="mt-7 flex items-center justify-between gap-3 border-t border-white/[0.07] pt-5">
            <Button
              variant="ghost"
              onClick={() => (step === 0 ? onClose() : setStep((current) => current - 1))}
              disabled={busy}
            >
              {step === 0 ? 'Cancel' : 'Back'}
            </Button>

            <Button
              type="submit"
              variant="primary"
              loading={busy}
              disabled={
                (step === 0 && !sourceValid) ||
                (step === 1 && (!nameValid || !versionValid)) ||
                (step === 2 && (!sourceValid || !nameValid || !versionValid))
              }
            >
              {step === 2 ? 'Import server' : 'Continue'}
            </Button>
          </footer>
        </form>
      </div>
    </div>
  )
}

function SourceCard({
  active,
  title,
  detail,
  onClick,
  icon,
}: {
  active: boolean
  title: string
  detail: string
  onClick: () => void
  icon: React.ReactNode
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={cn(
        'rounded-2xl border p-4 text-left transition-all duration-150',
        active
          ? 'border-neon-cyan/40 bg-neon-cyan/[0.07] ring-1 ring-neon-cyan/30'
          : 'border-white/[0.08] bg-black/20 hover:border-white/15 hover:bg-white/[0.04]',
      )}
    >
      <span className="grid size-9 place-items-center rounded-lg border border-white/10 bg-black/30 text-neon-cyan">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="size-4">
          {icon}
        </svg>
      </span>
      <span className="mt-3 block text-sm font-semibold">{title}</span>
      <span className="mt-1 block text-[11px] leading-relaxed text-text-muted">{detail}</span>
    </button>
  )
}

function sliderFill(percent: number): { background: string } {
  return {
    background: `linear-gradient(to right, #22d3ee 0%, #22d3ee ${percent}%, rgba(255,255,255,0.1) ${percent}%, rgba(255,255,255,0.1) 100%)`,
  }
}
