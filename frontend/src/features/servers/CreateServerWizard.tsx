import { useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent } from 'react'

import { cn } from '../../lib/cn'
import { ApiError, serverApi } from '../../lib/api'
import { SERVER_TYPES, serverTypeMeta } from '../../lib/servers'
import type { ServerType, ServerView } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { ErrorBanner } from '../../components/ui/Feedback'
import { TextField } from '../../components/ui/TextField'

const STEP_LABELS = ['Runtime', 'World', 'Resources', 'Details'] as const
const MIN_RAM_GB = 1
const MAX_RAM_GB = 16
const MIN_CPU = 0.5
const MAX_CPU = 4
const CATEGORIES = ['Official', 'Plugins', 'Modded', 'Hybrid', 'Proxy', 'Bedrock'] as const

interface CreateServerWizardProps {
  suggestedPort: number
  onClose: () => void
  onCreated: (server: ServerView) => void
}

export function CreateServerWizard({ suggestedPort, onClose, onCreated }: CreateServerWizardProps) {
  const [step, setStep] = useState(0)
  const [serverType, setServerType] = useState<ServerType>('paper')
  const [category, setCategory] = useState<(typeof CATEGORIES)[number]>('Plugins')
  const [gameVersion, setGameVersion] = useState('LATEST')
  const [versions, setVersions] = useState<string[]>(['LATEST'])
  const [versionsLoading, setVersionsLoading] = useState(true)
  const worldUpload = useRef<HTMLInputElement>(null)
  const [worldArchive, setWorldArchive] = useState<File | null>(null)
  const [ramGb, setRamGb] = useState(4)
  const [cpu, setCpu] = useState(2)
  const [worldName, setWorldName] = useState('world')
  const [worldSeed, setWorldSeed] = useState('')
  const [worldType, setWorldType] = useState('default')
  const [difficulty, setDifficulty] = useState('normal')
  const [gamemode, setGamemode] = useState('survival')
  const [name, setName] = useState('')
  const [port, setPort] = useState(String(suggestedPort))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const meta = useMemo(() => serverTypeMeta(serverType), [serverType])
  const parsedPort = Number.parseInt(port, 10)
  const trimmedName = name.trim()
  const trimmedVersion = gameVersion.trim()
  const nameValid = trimmedName.length >= 1 && trimmedName.length <= 64
  const versionValid = /^[A-Za-z0-9._+-]{1,32}$/.test(trimmedVersion)
  const portValid = Number.isInteger(parsedPort) && parsedPort >= 1024 && parsedPort <= 65535

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy) onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [busy, onClose])

  useEffect(() => {
    let cancelled = false
    // Mojang's official version manifest is the source of the release IDs used by Java runtimes.
    void fetch('https://piston-meta.mojang.com/mc/game/version_manifest_v2.json')
      .then((response) => response.ok ? response.json() as Promise<{ versions: { id: string; type: string }[] }> : Promise.reject())
      .then((manifest) => {
        if (!cancelled) setVersions(['LATEST', ...manifest.versions.filter((item) => item.type === 'release').map((item) => item.id).slice(0, 30)])
      })
      .catch(() => { if (!cancelled) setVersions(['LATEST']) })
      .finally(() => { if (!cancelled) setVersionsLoading(false) })
    return () => { cancelled = true }
  }, [])

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (step < STEP_LABELS.length - 1) {
      if (step === 0 && !versionValid) return
      setStep((current) => current + 1)
      return
    }
    if (!nameValid || !portValid || !versionValid || busy) return

    setBusy(true)
    setError(null)
    try {
      const created = await serverApi.create({
        name: trimmedName,
        server_type: serverType,
        game_version: trimmedVersion,
        game_port: parsedPort,
        ram_mb: ramGb * 1024,
        cpu_limit: cpu,
        world_name: worldName.trim() || 'world', world_seed: worldSeed.trim(), world_type: worldType, difficulty, gamemode,
        is_behind_proxy: false,
      })
      onCreated(created)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not create the server.')
      setBusy(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-black/65 px-4 py-8 backdrop-blur-md sm:py-12">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="wizard-title"
        className="glass-deep animate-rise relative w-full max-w-2xl rounded-glass shadow-glass"
      >
        <header className="flex items-start justify-between gap-4 border-b border-white/[0.07] px-6 py-5 sm:px-7">
          <div>
            <p className="font-mono text-[10px] tracking-[0.22em] text-neon-emerald uppercase">
              new server
            </p>
            <h2 id="wizard-title" className="mt-1 text-xl font-semibold tracking-tight">
              Provision a container
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Close wizard"
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
                    ? 'border-neon-emerald/40 bg-neon-emerald/15 text-neon-emerald'
                    : index === step
                      ? 'border-neon-emerald bg-neon-emerald text-void'
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
                <span className={cn('h-px flex-1', index < step ? 'bg-neon-emerald/30' : 'bg-white/10')} />
              ) : null}
            </div>
          ))}
        </div>

        <form onSubmit={submit} className="px-6 py-6 sm:px-7">
          {error ? <ErrorBanner className="mb-5">{error}</ErrorBanner> : null}

          {step === 0 ? (
            <div className="animate-rise">
              <p className="mb-4 text-xs text-text-muted">
                Pick the runtime baked into the container. Swapping later requires a recreate.
              </p>
              <div className="mb-5 flex flex-wrap gap-2">
                {CATEGORIES.map((item) => <button key={item} type="button" onClick={() => setCategory(item)} className={cn('rounded-full border px-3 py-1.5 text-xs transition', category === item ? 'border-neon-emerald/40 bg-neon-emerald/10 text-neon-emerald' : 'border-white/10 text-text-muted hover:bg-white/5')}>{item}</button>)}
              </div>
              <section className="mb-5">
                <p className="mb-2 font-mono text-[10px] tracking-[0.18em] text-text-dim uppercase">{category} software</p>
              <div className="grid gap-2.5 sm:grid-cols-2">
                {SERVER_TYPES.filter((option) => option.category === category).map((option) => {
                  const selected = option.value === serverType
                  return (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => setServerType(option.value)}
                      aria-pressed={selected}
                      className={cn(
                        'group rounded-2xl border p-4 text-left transition-all duration-150',
                        selected
                          ? 'border-neon-emerald/40 bg-neon-emerald/[0.07] ring-1 ring-neon-emerald/30'
                          : 'border-white/[0.08] bg-black/20 hover:border-white/15 hover:bg-white/[0.04]',
                      )}
                    >
                      <span className="flex items-center justify-between">
                        <span
                          className={cn(
                            'grid size-9 place-items-center rounded-lg border border-white/10 bg-black/30 font-mono text-sm font-semibold',
                            option.accent,
                          )}
                        >
                          {option.monogram}
                        </span>
                        <span className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
                          {option.tagline}
                        </span>
                      </span>
                      <span className="mt-3 block text-sm font-semibold">{option.label}</span>
                      <span className="mt-1 block text-[11px] leading-relaxed text-text-muted">
                        {option.description}
                      </span>
                    </button>
                  )
                })}
              </div>
              </section>
              {meta.proxy ? (
                <p className="mt-4 rounded-xl border border-neon-amber/20 bg-neon-amber/[0.06] px-3.5 py-2.5 text-[11px] leading-relaxed text-neon-amber">
                  Proxy networking (backend grouping + forwarding secrets) is not configurable yet.
                  This creates a standalone proxy container that publishes its own port.
                </p>
              ) : null}

              <div className="mt-5">
                <label className="block text-sm font-medium">Game version
                  <select value={gameVersion} onChange={(event) => setGameVersion(event.target.value)} className="mt-2 w-full rounded-xl border border-white/10 bg-black/30 px-3 py-2.5 text-sm">
                    {versions.map((version) => <option key={version} value={version}>{version === 'LATEST' ? 'LATEST — newest stable release' : version}</option>)}
                  </select>
                </label>
                <p className="mt-2 text-[11px] leading-relaxed text-text-muted">
                  {versionsLoading ? 'Loading authenticated Mojang release versions…' : 'Release versions come live from Mojang’s official manifest.'} Geyser compatibility depends on its installed version and is checked after provisioning.
                </p>
              </div>
            </div>
          ) : null}

          {step === 1 ? (
            <div className="animate-rise space-y-4">
              <p className="text-xs text-text-muted">These values generate the world on its first server start.</p>
              <TextField label="World name" value={worldName} onChange={(event) => setWorldName(event.target.value)} placeholder="world" hint="Letters, numbers, underscores and dashes are recommended." />
              <TextField label="Seed (optional)" value={worldSeed} onChange={(event) => setWorldSeed(event.target.value)} placeholder="Leave blank for a random world" />
              <div className="rounded-xl border border-dashed border-white/15 bg-black/20 p-4"><p className="text-sm font-medium">Existing world archive</p><p className="mt-1 text-xs text-text-muted">Select a ZIP now; upload is staged after the server is created.</p><input ref={worldUpload} type="file" accept=".zip,application/zip" className="sr-only" onChange={(event) => setWorldArchive(event.target.files?.[0] ?? null)} /><Button type="button" size="sm" variant="secondary" className="mt-3" onClick={() => worldUpload.current?.click()}>{worldArchive ? worldArchive.name : 'Choose world ZIP'}</Button></div>
              <div className="grid gap-4 sm:grid-cols-3">
                <Select label="World type" value={worldType} onChange={setWorldType} options={['default', 'flat', 'largeBiomes', 'amplified']} />
                <Select label="Difficulty" value={difficulty} onChange={setDifficulty} options={['peaceful', 'easy', 'normal', 'hard']} />
                <Select label="Game mode" value={gamemode} onChange={setGamemode} options={['survival', 'creative', 'adventure', 'spectator']} />
              </div>
            </div>
          ) : null}

          {step === 2 ? (
            <div className="animate-rise space-y-7">
              <div>
                <div className="flex items-baseline justify-between">
                  <label htmlFor="ram" className="text-sm font-medium">
                    Memory limit
                  </label>
                  <span className="font-mono text-sm text-neon-emerald">{ramGb} GB</span>
                </div>
                <input
                  id="ram"
                  type="range"
                  min={MIN_RAM_GB}
                  max={MAX_RAM_GB}
                  step={1}
                  value={ramGb}
                  onChange={(event) => setRamGb(Number(event.target.value))}
                  className="range-neon mt-4"
                  style={sliderFill(((ramGb - MIN_RAM_GB) / (MAX_RAM_GB - MIN_RAM_GB)) * 100)}
                />
                <div className="mt-2 flex justify-between font-mono text-[10px] text-text-dim">
                  <span>{MIN_RAM_GB} GB</span>
                  <span className="text-text-muted">hard Docker limit · swap disabled</span>
                  <span>{MAX_RAM_GB} GB</span>
                </div>
              </div>

              <div>
                <div className="flex items-baseline justify-between">
                  <label htmlFor="cpu" className="text-sm font-medium">
                    CPU allocation
                  </label>
                  <span className="font-mono text-sm text-neon-cyan">
                    {cpu.toFixed(1)} vCPU
                  </span>
                </div>
                <input
                  id="cpu"
                  type="range"
                  min={MIN_CPU}
                  max={MAX_CPU}
                  step={0.5}
                  value={cpu}
                  onChange={(event) => setCpu(Number(event.target.value))}
                  className="range-neon mt-4"
                  style={sliderFill(((cpu - MIN_CPU) / (MAX_CPU - MIN_CPU)) * 100)}
                />
                <div className="mt-2 flex justify-between font-mono text-[10px] text-text-dim">
                  <span>{MIN_CPU} vCPU</span>
                  <span className="text-text-muted">nano-cpus enforced by the engine</span>
                  <span>{MAX_CPU} vCPU</span>
                </div>
              </div>

              <div className="rounded-xl border border-white/[0.07] bg-black/20 px-4 py-3 text-[11px] leading-relaxed text-text-muted">
                The container receives <span className="font-mono text-text-primary">{ramGb}G</span> of
                heap via <span className="font-mono text-text-primary">MEMORY</span> and a hard limit
                of <span className="font-mono text-text-primary">{ramGb * 1024} MiB</span>. PIDs are
                capped at 512 and the process cannot gain new privileges.
              </div>
            </div>
          ) : null}

          {step === 3 ? (
            <div className="animate-rise space-y-4">
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

              <TextField
                label="Host game port"
                type="number"
                min={1024}
                max={65535}
                value={port}
                onChange={(event) => setPort(event.target.value)}
                hint={serverType === 'bedrock' ? 'published as UDP → container 19132' : 'published on 0.0.0.0 → container 25565'}
                error={port.length > 0 && !portValid ? 'Use a port between 1024 and 65535.' : undefined}
                required
              />

              <div className="rounded-2xl border border-white/[0.07] bg-black/20 p-4">
                <p className="font-mono text-[10px] tracking-[0.18em] text-text-dim uppercase">
                  review
                </p>
                <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2.5 text-sm">
                  <div className="flex justify-between gap-3">
                    <dt className="text-text-dim">Runtime</dt>
                    <dd className={meta.accent}>{meta.label}</dd>
                  </div>
                  <div className="flex justify-between gap-3"><dt className="text-text-dim">World</dt><dd className="font-mono">{worldName || 'world'}</dd></div>
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
                    <dt className="text-text-dim">Port</dt>
                    <dd className="font-mono">{portValid ? parsedPort : '—'}</dd>
                  </div>
                </dl>
                <p className="mt-3 border-t border-white/[0.06] pt-3 text-[11px] leading-relaxed text-text-muted">
                  The daemon creates <span className="font-mono">mc-&lt;id&gt;</span>, a persistent
                  <span className="font-mono"> /data</span> volume, and starts it in the stopped state.
                </p>
              </div>
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
                (step === 0 && !versionValid) ||
                (step === 3 && (!nameValid || !portValid || !versionValid))
              }
            >
              {step === 3 ? 'Create server' : 'Continue'}
            </Button>
          </footer>
        </form>
      </div>
    </div>
  )
}

function Select({ label, value, onChange, options }: { label: string; value: string; onChange: (value: string) => void; options: readonly string[] }) {
  return <label className="block text-sm font-medium">{label}<select value={value} onChange={(event) => onChange(event.target.value)} className="mt-2 w-full rounded-xl border border-white/10 bg-black/30 px-3 py-2 text-sm">{options.map((option) => <option key={option} value={option}>{option}</option>)}</select></label>
}

function sliderFill(percent: number): { background: string } {
  return {
    background: `linear-gradient(to right, #3cf5a6 0%, #3cf5a6 ${percent}%, rgba(255,255,255,0.1) ${percent}%, rgba(255,255,255,0.1) 100%)`,
  }
}
