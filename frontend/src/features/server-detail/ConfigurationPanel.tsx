import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import { cn } from '../../lib/cn'
import { ApiError, serverApi } from '../../lib/api'
import {
  FIELDS,
  FIELDS_BY_SECTION,
  FIELD_BY_KEY,
  parseNumber,
  type PropertyField,
} from '../../lib/properties'
import type { PropertyEntry, PropertiesResponse, ServerView } from '../../lib/types'
import { formatCpu, formatRam } from '../../lib/servers'
import { Button } from '../../components/ui/Button'
import { EmptyState } from '../../components/ui/Feedback'
import { GlassCard } from '../../components/ui/GlassCard'

interface ConfigurationPanelProps {
  serverId: string
  running: boolean
  ramMb: number
  cpuLimit: number
  onServerUpdated: (server: ServerView) => void
}

export function ConfigurationPanel({
  serverId,
  running,
  ramMb,
  cpuLimit,
  onServerUpdated,
}: ConfigurationPanelProps) {
  const [data, setData] = useState<PropertiesResponse | null>(null)
  const [entries, setEntries] = useState<PropertyEntry[]>([])
  const [baseline, setBaseline] = useState('')
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [restartOpen, setRestartOpen] = useState(false)
  const requestId = useRef(0)

  const load = useCallback(async () => {
    const id = ++requestId.current
    setLoading(true)
    setError(null)
    try {
      const response = await serverApi.getProperties(serverId)
      if (id !== requestId.current) return
      setData(response)
      setEntries(response.entries)
      setBaseline(JSON.stringify(response.entries))
    } catch (caught) {
      if (id !== requestId.current) return
      setData(null)
      setError(caught instanceof ApiError ? caught.message : 'Could not load server.properties.')
    } finally {
      if (id === requestId.current) setLoading(false)
    }
  }, [serverId])

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    void load()
    return () => {
      requestId.current += 1
    }
  }, [load])

  const values = useMemo(() => {
    const map = new Map<string, string>()
    for (const entry of entries) {
      if (entry.kind === 'property' && !map.has(entry.key)) map.set(entry.key, entry.value)
    }
    return map
  }, [entries])

  const dirty = useMemo(() => JSON.stringify(entries) !== baseline, [entries, baseline])

  const otherProperties = useMemo(() => {
    const seen = new Set<string>()
    const result: { key: string; value: string }[] = []
    for (const entry of entries) {
      if (entry.kind !== 'property' || FIELD_BY_KEY.has(entry.key) || seen.has(entry.key)) continue
      seen.add(entry.key)
      result.push({ key: entry.key, value: entry.value })
    }
    return result
  }, [entries])

  const missingCount = useMemo(() => {
    const present = new Set(values.keys())
    return FIELDS.filter((field) => !present.has(field.key)).length
  }, [values])

  const setProperty = useCallback((key: string, value: string) => {
    setEntries((current) => {
      const index = current.findIndex((entry) => entry.kind === 'property' && entry.key === key)
      if (index === -1) return [...current, { kind: 'property', key, value }]
      return current.map((entry, position) =>
        position === index && entry.kind === 'property' ? { ...entry, value } : entry,
      )
    })
  }, [])

  async function save() {
    if (!data) return

    for (const field of FIELDS) {
      if (field.kind !== 'number') continue
      const raw = values.get(field.key)
      if (raw === undefined || raw.trim() === '') continue
      if (parseNumber(field, raw) === null) {
        setError(`${field.label} must be a number between ${field.min ?? '-∞'} and ${field.max ?? '∞'}.`)
        return
      }
    }

    setSaving(true)
    setError(null)
    try {
      const response = await serverApi.updateProperties(serverId, entries, data.hash)
      setData(response)
      setEntries(response.entries)
      setBaseline(JSON.stringify(response.entries))
      setRestartOpen(true)
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 409) {
        setError('server.properties changed on disk. Reload before saving again.')
      } else {
        setError(caught instanceof ApiError ? caught.message : 'Could not save server.properties.')
      }
    } finally {
      setSaving(false)
    }
  }

  function reload() {
    if (dirty && !window.confirm('Discard your unsaved changes?')) return
    void load()
  }

  if (loading && !data) {
    return <div className="glass-deep h-[540px] animate-pulse rounded-glass" aria-hidden="true" />
  }

  if (!data) {
    return (
      <div className="flex flex-col gap-6">
        <ResourcesSection
          serverId={serverId}
          ramMb={ramMb}
          cpuLimit={cpuLimit}
          running={running}
          onUpdated={onServerUpdated}
        />
        <GlassCard className="animate-rise p-6">
          <EmptyState
            title="No server.properties yet"
            description={
              error ??
              'The file is generated the first time the container starts. Start the server once, then reload this tab.'
            }
            action={
              <Button variant="secondary" onClick={() => void load()}>
                Reload
              </Button>
            }
          />
        </GlassCard>
      </div>
    )
  }

  const presentFields = FIELDS.filter((field) => values.has(field.key))

  return (
    <div className="flex flex-col gap-6">
      <ResourcesSection
        serverId={serverId}
        ramMb={ramMb}
        cpuLimit={cpuLimit}
        running={running}
        onUpdated={onServerUpdated}
      />

      <GlassCard className="animate-rise sticky top-3 z-20 overflow-hidden">
        <div className="flex flex-wrap items-center justify-between gap-3 px-5 py-4">
          <div className="flex min-w-0 items-center gap-3">
            <span className="grid size-9 shrink-0 place-items-center rounded-xl border border-neon-emerald/25 bg-neon-emerald/10 text-neon-emerald">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" className="size-4">
                <path d="M4 6h16M4 12h16M4 18h10" strokeLinecap="round" />
                <circle cx="18" cy="18" r="2.4" />
              </svg>
            </span>
            <div className="min-w-0">
              <h2 className="flex flex-wrap items-center gap-2 text-sm font-semibold">
                server.properties
                {dirty ? (
                  <span className="rounded-full border border-neon-amber/30 bg-neon-amber/10 px-2 py-0.5 font-mono text-[9px] tracking-[0.12em] text-neon-amber uppercase">
                    unsaved
                  </span>
                ) : (
                  <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-[9px] tracking-[0.12em] text-text-dim uppercase">
                    synced
                  </span>
                )}
              </h2>
              <p className="mt-0.5 truncate font-mono text-[10px] text-text-dim">
                {presentFields.length} managed · {otherProperties.length} custom · hash{' '}
                {data.hash.slice(0, 10)}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={reload} disabled={saving}>
              Reload
            </Button>
            <Button
              variant="primary"
              size="sm"
              loading={saving}
              disabled={!dirty}
              onClick={() => void save()}
              data-testid="config-save"
              icon={
                <svg viewBox="0 0 16 16" className="size-3.5" fill="currentColor" aria-hidden="true">
                  <path d="M2.5 2.5h8.4l2.6 2.6v8.4h-11zM4.5 3.5v3h6v-3zM4.5 9h7v4h-7z" />
                </svg>
              }
            >
              Save changes
            </Button>
          </div>
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

      {!running ? (
        <div className="rounded-xl border border-neon-amber/20 bg-neon-amber/[0.06] px-4 py-3 text-[11px] leading-relaxed text-neon-amber/90">
          This server is stopped. Changes are written immediately and take effect on the next start.
        </div>
      ) : null}

      <div className="grid items-start gap-8 xl:grid-cols-2">
        {FIELDS_BY_SECTION.map(({ section, fields }) => {
          const visible = fields.filter((field) => values.has(field.key))
          if (visible.length === 0) return null
          return (
            <GlassCard key={section.id} className="animate-rise overflow-hidden p-1">
              <header className="flex items-start gap-3 border-b border-white/[0.07] px-5 py-4">
                <span
                  className={cn(
                    'mt-0.5 grid size-2.5 shrink-0 rounded-full border border-white/20 bg-white/10',
                    section.accent,
                    section.glow,
                  )}
                  aria-hidden="true"
                />
                <div>
                  <h3 className={cn('text-sm font-semibold', section.accent)}>{section.title}</h3>
                  <p className="mt-0.5 text-[11px] text-text-dim">{section.description}</p>
                </div>
              </header>
              <div className="divide-y divide-white/[0.05]">
                {visible.map((field) => (
                  <FieldRow
                    key={field.key}
                    field={field}
                    value={values.get(field.key) ?? ''}
                    redacted={field.kind === 'secret' && data.redacted.includes(field.key)}
                    onChange={(value) => setProperty(field.key, value)}
                  />
                ))}
              </div>
            </GlassCard>
          )
        })}

        {otherProperties.length > 0 ? (
          <GlassCard className="animate-rise overflow-hidden xl:col-span-2">
            <header className="border-b border-white/[0.07] px-5 py-4">
              <h3 className="text-sm font-semibold">Custom properties</h3>
              <p className="mt-0.5 text-[11px] text-text-dim">
                Keys outside the curated schema. Comments and ordering are preserved on save.
              </p>
            </header>
            <div className="grid gap-4 p-5 sm:grid-cols-2">
              {otherProperties.map((property) => (
                <div key={property.key}>
                  <label
                    htmlFor={`custom-${property.key}`}
                    className="mb-1.5 block truncate font-mono text-[10px] tracking-[0.04em] text-text-dim"
                  >
                    {property.key}
                  </label>
                  <input
                    id={`custom-${property.key}`}
                    value={property.value}
                    onChange={(event) => setProperty(property.key, event.target.value)}
                    className="h-10 w-full rounded-xl border border-white/10 bg-black/25 px-3 font-mono text-xs text-text-primary outline-none transition focus:border-neon-emerald/50 focus:bg-black/35 focus:ring-2 focus:ring-neon-emerald/15"
                  />
                </div>
              ))}
            </div>
          </GlassCard>
        ) : null}
      </div>

      {missingCount > 0 ? (
        <p className="px-1 text-[11px] text-text-dim">
          {missingCount} curated {missingCount === 1 ? 'property is' : 'properties are'} not present in
          this file. Only keys already on disk are shown so saving never injects unexpected defaults.
        </p>
      ) : null}

      {restartOpen ? <RestartModal onClose={() => setRestartOpen(false)} /> : null}
    </div>
  )
}

function FieldRow({
  field,
  value,
  redacted,
  onChange,
}: {
  field: PropertyField
  value: string
  redacted: boolean
  onChange: (value: string) => void
}) {
  return (
    <div className="grid gap-3 px-5 py-3.5 sm:grid-cols-[1fr_minmax(150px,0.9fr)] sm:items-center">
      <div className="min-w-0">
        <p className="text-xs font-medium text-text-primary">{field.label}</p>
        <p className="mt-0.5 font-mono text-[9px] tracking-[0.04em] text-text-dim">{field.key}</p>
        {field.hint ? <p className="mt-1 text-[10px] leading-relaxed text-text-dim">{field.hint}</p> : null}
      </div>
      <div className="flex min-w-0 items-center justify-end gap-2">
        <FieldControl field={field} value={value} redacted={redacted} onChange={onChange} />
      </div>
    </div>
  )
}

function FieldControl({
  field,
  value,
  redacted,
  onChange,
}: {
  field: PropertyField
  value: string
  redacted: boolean
  onChange: (value: string) => void
}) {
  if (field.kind === 'secret') {
    return (
      <span
        className="flex h-9 min-w-0 flex-1 items-center justify-between gap-2 rounded-lg border border-white/10 bg-black/25 px-3 font-mono text-[11px] text-text-dim"
        title="Managed by Cluster Console"
      >
        <span className="truncate">{redacted ? '••••••••••••' : 'unset'}</span>
        <span className="shrink-0 rounded border border-neon-violet/25 bg-neon-violet/10 px-1.5 py-0.5 text-[8px] tracking-[0.08em] text-neon-violet uppercase">
          locked
        </span>
      </span>
    )
  }

  if (field.kind === 'boolean') {
    const enabled = value.toLowerCase() === 'true'
    return (
      <button
        type="button"
        role="switch"
        aria-checked={enabled}
        aria-label={field.label}
        data-testid={`config-field-${field.key}`}
        onClick={() => onChange(enabled ? 'false' : 'true')}
        className={cn(
          'relative inline-flex h-7 w-12 shrink-0 items-center rounded-full border px-0.5 transition-all duration-200 focus-visible:ring-2 focus-visible:ring-neon-emerald/50 focus-visible:outline-none',
          enabled
            ? 'border-neon-emerald/50 bg-gradient-to-r from-neon-emerald/80 to-neon-cyan/70 shadow-glow-emerald'
            : 'border-white/15 bg-white/[0.06]',
        )}
      >
        <span
          className={cn(
            'size-5 rounded-full bg-white shadow transition-transform duration-200',
            enabled ? 'translate-x-[22px]' : 'translate-x-0 bg-white/60',
          )}
        />
      </button>
    )
  }

  if (field.kind === 'enum') {
    return (
      <div className="relative min-w-0 flex-1">
        <select
          value={value}
          aria-label={field.label}
          data-testid={`config-field-${field.key}`}
          onChange={(event) => onChange(event.target.value)}
          className="h-9 w-full appearance-none rounded-lg border border-white/10 bg-black/30 pl-3 pr-8 text-xs text-text-primary outline-none transition focus:border-neon-emerald/50 focus:ring-2 focus:ring-neon-emerald/15"
        >
          {!field.options?.some((option) => option.value === value) ? (
            <option value={value}>{value || '—'}</option>
          ) : null}
          {field.options?.map((option) => (
            <option key={option.value} value={option.value} className="bg-surface">
              {option.label}
            </option>
          ))}
        </select>
        <svg
          viewBox="0 0 16 16"
          className="pointer-events-none absolute top-1/2 right-2.5 size-3 -translate-y-1/2 text-text-dim"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          aria-hidden="true"
        >
          <path d="m4 6 4 4 4-4" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </div>
    )
  }

  if (field.kind === 'number' && field.slider) {
    const min = field.min ?? 0
    const max = field.max ?? 100
    const numeric = Number(value)
    const safe = Number.isFinite(numeric) ? Math.min(max, Math.max(min, numeric)) : min
    return (
      <div className="w-full min-w-0">
        <div className="flex items-center justify-end gap-2">
          <span className="rounded-md border border-neon-emerald/20 bg-neon-emerald/10 px-2 py-0.5 font-mono text-[10px] text-neon-emerald">
            {value || safe}
            {field.unit ? ` ${field.unit}` : ''}
          </span>
        </div>
        <input
          type="range"
          min={min}
          max={max}
          step={field.step ?? 1}
          value={safe}
          aria-label={field.label}
          data-testid={`config-field-${field.key}`}
          onChange={(event) => onChange(event.target.value)}
          className="range-neon mt-2"
        />
      </div>
    )
  }

  return (
    <input
      type={field.kind === 'number' ? 'number' : 'text'}
      value={value}
      min={field.min}
      max={field.max}
      step={field.step}
      aria-label={field.label}
      data-testid={`config-field-${field.key}`}
      onChange={(event) => onChange(event.target.value)}
      className="h-9 w-full min-w-0 rounded-lg border border-white/10 bg-black/30 px-3 text-xs text-text-primary outline-none transition focus:border-neon-emerald/50 focus:ring-2 focus:ring-neon-emerald/15"
    />
  )
}

function ResourcesSection({
  serverId,
  ramMb,
  cpuLimit,
  running,
  onUpdated,
}: {
  serverId: string
  ramMb: number
  cpuLimit: number
  running: boolean
  onUpdated: (server: ServerView) => void
}) {
  const [ram, setRam] = useState(ramMb)
  const [cpu, setCpu] = useState(cpuLimit)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    setRam(ramMb)
    setCpu(cpuLimit)
  }, [ramMb, cpuLimit])

  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(null), 6000)
    return () => window.clearTimeout(timer)
  }, [notice])

  const dirty = ram !== ramMb || Math.abs(cpu - cpuLimit) > 0.001

  async function save() {
    setSaving(true)
    setError(null)
    setNotice(null)
    try {
      const server = await serverApi.updateResources(serverId, {
        ram_mb: ram,
        cpu_limit: cpu,
      })
      onUpdated(server)
      setNotice(
        running
          ? 'Resource limits saved. Restart the server to apply them to the running container.'
          : 'Resource limits saved. They will be applied on the next start.',
      )
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not save resource limits.')
    } finally {
      setSaving(false)
    }
  }

  return (
    <GlassCard className="animate-rise overflow-hidden">
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-white/[0.07] px-5 py-4">
        <div className="flex min-w-0 items-center gap-3">
          <span className="grid size-9 shrink-0 place-items-center rounded-xl border border-neon-violet/25 bg-neon-violet/10 text-neon-violet">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" className="size-4">
              <path d="M4 19V9M10 19V5M16 19v-7M22 19V8" strokeLinecap="round" />
            </svg>
          </span>
          <div className="min-w-0">
            <h2 className="flex flex-wrap items-center gap-2 text-sm font-semibold">
              Resources &amp; scaling
              {dirty ? (
                <span className="rounded-full border border-neon-amber/30 bg-neon-amber/10 px-2 py-0.5 font-mono text-[9px] tracking-[0.12em] text-neon-amber uppercase">
                  unsaved
                </span>
              ) : null}
            </h2>
            <p className="mt-0.5 text-[11px] text-text-dim">
              Hard Docker limits, stored in the control plane and applied on start.
            </p>
          </div>
        </div>

        <Button
          variant="primary"
          size="sm"
          loading={saving}
          disabled={!dirty}
          onClick={() => void save()}
          data-testid="resources-save"
        >
          Save Resources
        </Button>
      </header>

      {error ? (
        <p role="alert" className="border-b border-neon-rose/20 bg-neon-rose/[0.07] px-5 py-2.5 text-[11px] text-neon-rose">
          {error}
        </p>
      ) : notice ? (
        <p className="border-b border-neon-emerald/20 bg-neon-emerald/[0.07] px-5 py-2.5 text-[11px] text-neon-emerald">
          {notice}
        </p>
      ) : null}

      <div className="grid gap-6 p-5 sm:grid-cols-2">
        <label className="block">
          <span className="flex items-baseline justify-between gap-3">
            <span className="text-xs font-medium text-text-primary">Memory</span>
            <span className="rounded-md border border-neon-violet/20 bg-neon-violet/10 px-2 py-0.5 font-mono text-[10px] text-neon-violet">
              {formatRam(ram)}
            </span>
          </span>
          <input
            type="range"
            min={512}
            max={65536}
            step={512}
            value={ram}
            aria-label="Memory in megabytes"
            onChange={(event) => setRam(Number(event.target.value))}
            className="range-neon mt-3"
          />
          <span className="mt-1 flex justify-between font-mono text-[9px] text-text-dim">
            <span>512 MB</span>
            <span>64 GB</span>
          </span>
        </label>

        <label className="block">
          <span className="flex items-baseline justify-between gap-3">
            <span className="text-xs font-medium text-text-primary">CPU</span>
            <span className="rounded-md border border-neon-violet/20 bg-neon-violet/10 px-2 py-0.5 font-mono text-[10px] text-neon-violet">
              {formatCpu(cpu)}
            </span>
          </span>
          <input
            type="range"
            min={0.5}
            max={32}
            step={0.5}
            value={cpu}
            aria-label="CPU limit in virtual cores"
            onChange={(event) => setCpu(Number(event.target.value))}
            className="range-neon mt-3"
          />
          <span className="mt-1 flex justify-between font-mono text-[9px] text-text-dim">
            <span>0.5 vCPU</span>
            <span>32 vCPU</span>
          </span>
        </label>
      </div>

      <p className="border-t border-white/[0.06] px-5 py-2.5 text-[10px] leading-relaxed text-text-dim">
        {running
          ? 'This server is running with its previous limits. Restart it to apply the new values.'
          : 'The daemon applies these limits to Docker HostConfig before every start.'}
      </p>
    </GlassCard>
  )
}

function RestartModal({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return createPortal(
    <div
      className="animate-fade-in fixed inset-0 z-50 grid place-items-center bg-void/80 px-4 backdrop-blur-sm"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="restart-title"
        data-testid="config-restart-modal"
        className="animate-rise w-full max-w-md rounded-glass border border-white/10 bg-abyss/95 p-6 shadow-glass backdrop-blur-2xl"
      >
        <div className="flex items-start gap-4">
          <span className="grid size-11 shrink-0 place-items-center rounded-2xl border border-neon-amber/30 bg-neon-amber/10 text-neon-amber shadow-glow-amber">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" className="size-5">
              <path d="M20 11a8 8 0 1 0-2.3 5.7" strokeLinecap="round" />
              <path d="M20 4v7h-7" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </span>
          <div>
            <h2 id="restart-title" className="text-base font-semibold">
              Saved — restart required
            </h2>
            <p className="mt-2 text-sm leading-relaxed text-text-muted">
              server.properties has been written to disk. Minecraft only reads it during startup, so
              stop and start the container (or use <span className="font-mono text-text-primary">/reload</span>{' '}
              where supported) before the new values take effect.
            </p>
          </div>
        </div>
        <div className="mt-6 flex justify-end">
          <Button variant="primary" onClick={onClose}>
            Got it
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  )
}
