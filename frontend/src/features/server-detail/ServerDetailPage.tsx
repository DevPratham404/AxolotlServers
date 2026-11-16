import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'

import { cn } from '../../lib/cn'
import { ApiError, serverApi } from '../../lib/api'
import { formatCpu, formatRam, serverTypeMeta } from '../../lib/servers'
import type { ServerView } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { ErrorBanner } from '../../components/ui/Feedback'
import { GlassCard } from '../../components/ui/GlassCard'
import { StatusPill } from '../../components/ui/StatusPill'
import { LogConsole } from './LogConsole'
import { AppStorePanel } from './AppStorePanel'
import { AutomationPanel } from './AutomationPanel'
import { ConfigurationPanel } from './ConfigurationPanel'
import { FileManagerPanel } from './FileManagerPanel'
import { StatsPanel } from './StatsPanel'
import { useServerTelemetry } from './useServerTelemetry'
import { PlayersPanel } from './players/PlayersPanel'

type PowerAction = 'start' | 'stop' | 'restart' | null

type DetailTab = 'console' | 'players' | 'configuration' | 'files' | 'addons' | 'automation'

export function ServerDetailPage({
  serverId,
  onBack,
}: {
  serverId: string
  onBack: () => void
}) {
  const [server, setServer] = useState<ServerView | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<PowerAction>(null)
  const [tab, setTab] = useState<DetailTab>('console')
  const requestId = useRef(0)

  const load = useCallback(async () => {
    const id = ++requestId.current
    setLoading(true)
    setError(null)
    try {
      const next = await serverApi.get(serverId)
      if (id === requestId.current) setServer(next)
    } catch (caught) {
      if (id !== requestId.current) return
      setError(caught instanceof ApiError ? caught.message : 'Could not load this server.')
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

  const telemetry = useServerTelemetry(serverId, server?.status === 'running')

  async function power(action: Exclude<PowerAction, null>) {
    if (!server) return
    setBusy(action)
    setError(null)
    try {
      const next =
        action === 'start'
          ? await serverApi.start(server.id)
          : action === 'stop'
            ? await serverApi.stop(server.id)
            : await serverApi.restart(server.id)
      setServer(next)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'The power action failed.')
    } finally {
      setBusy(null)
    }
  }

  if (loading && !server) return <DetailSkeleton onBack={onBack} />

  if (!server) {
    return (
      <div>
        <BackButton onBack={onBack} />
        <div className="mt-6">
          <ErrorBanner>
            {error ?? 'Server not found.'}{' '}
            <button type="button" onClick={() => void load()} className="underline">
              Retry
            </button>
          </ErrorBanner>
        </div>
      </div>
    )
  }

  const meta = serverTypeMeta(server.server_type)
  const canStart = server.status === 'stopped' || server.status === 'error'
  const canStop = server.status === 'running' || server.status === 'starting'
  const canRestart = server.status === 'running'

  return (
    <div className="animate-rise">
      <BackButton onBack={onBack} />

      <header className="mt-4 mb-6 flex flex-wrap items-start justify-between gap-4">
        <div className="flex min-w-0 items-center gap-4">
          <span
            className={cn(
              'grid size-14 shrink-0 place-items-center rounded-2xl border border-white/10 bg-black/30 font-mono text-2xl font-semibold',
              meta.accent,
              meta.glow,
            )}
          >
            {meta.monogram}
          </span>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-3">
              <h1 className="truncate text-3xl font-semibold tracking-tight">{server.name}</h1>
              <StatusPill status={server.status} />
            </div>
            <p className="mt-1.5 font-mono text-[10px] tracking-[0.16em] text-text-dim uppercase">
              {meta.label}
              {meta.proxy ? ' · proxy' : ''} · {server.game_version} · {formatRam(server.ram_mb)} ·{' '}
              {formatCpu(server.cpu_limit)} ·{' '}
              {server.is_behind_proxy ? 'docker network' : `0.0.0.0:${server.game_port}`}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {canStart ? (
            <Button
              variant="primary"
              loading={busy === 'start'}
              disabled={busy !== null}
              onClick={() => void power('start')}
              icon={
                <svg viewBox="0 0 16 16" className="size-3.5" fill="currentColor" aria-hidden="true">
                  <path d="M4 2.8v10.4c0 .5.55.8.97.54l8.2-5.2a.64.64 0 0 0 0-1.08l-8.2-5.2A.64.64 0 0 0 4 2.8Z" />
                </svg>
              }
            >
              Start
            </Button>
          ) : null}
          {canStop ? (
            <Button
              loading={busy === 'stop'}
              disabled={busy !== null}
              onClick={() => void power('stop')}
              icon={
                <svg viewBox="0 0 16 16" className="size-3.5" fill="currentColor" aria-hidden="true">
                  <rect x="3.5" y="3.5" width="9" height="9" rx="1.4" />
                </svg>
              }
            >
              Stop
            </Button>
          ) : null}
          {canRestart ? (
            <Button
              variant="warning"
              loading={busy === 'restart'}
              disabled={busy !== null}
              onClick={() => void power('restart')}
              data-testid="server-restart"
              icon={
                <svg viewBox="0 0 16 16" className="size-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                  <path d="M13.5 7A5.5 5.5 0 1 0 12 11.2" strokeLinecap="round" />
                  <path d="M13.5 3v4h-4" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              }
            >
              Restart
            </Button>
          ) : null}
          <Button variant="ghost" onClick={() => void load()} loading={loading}>
            Refresh
          </Button>
        </div>
      </header>

      {error ? (
        <div className="mb-5">
          <ErrorBanner>{error}</ErrorBanner>
        </div>
      ) : null}

      {telemetry.error ? (
        <div className="mb-5">
          <ErrorBanner>{telemetry.error}</ErrorBanner>
        </div>
      ) : null}

      {server.status !== 'running' ? (
        <div className="mb-5 rounded-xl border border-neon-amber/20 bg-neon-amber/[0.06] px-4 py-3 text-[11px] leading-relaxed text-neon-amber/90">
          The container is not running, so Docker has no live samples to stream. Start the server to
          attach the resource and console streams.
        </div>
      ) : null}

      <div className="mb-4 flex items-center gap-1 border-b border-white/[0.07]">
        <TabButton active={tab === 'console'} onClick={() => setTab('console')}>
          Console
        </TabButton>
        <TabButton active={tab === 'players'} onClick={() => setTab('players')}>
          Players
        </TabButton>
        <TabButton active={tab === 'configuration'} onClick={() => setTab('configuration')}>
          Configuration
        </TabButton>
        <TabButton active={tab === 'files'} onClick={() => setTab('files')}>
          File Manager
        </TabButton>
        <TabButton active={tab === 'addons'} onClick={() => setTab('addons')}>
          App Store
        </TabButton>
        <TabButton active={tab === 'automation'} onClick={() => setTab('automation')}>
          Automation
        </TabButton>
      </div>

      {tab === 'players' ? (
        <PlayersPanel serverId={server.id} running={server.status === 'running'} />
      ) : tab === 'configuration' ? (
        <ConfigurationPanel
          serverId={server.id}
          running={server.status === 'running'}
          ramMb={server.ram_mb}
          cpuLimit={server.cpu_limit}
          onServerUpdated={setServer}
        />
      ) : tab === 'files' ? (
        <FileManagerPanel serverId={server.id} />
      ) : tab === 'addons' ? (
        <AppStorePanel
          serverId={server.id}
          serverType={server.server_type}
          gameVersion={server.game_version}
        />
      ) : tab === 'automation' ? (
        <AutomationPanel
          serverId={server.id}
          status={server.status}
          onStatusChanged={() => void load()}
        />
      ) : (
        <div className="grid items-start gap-4 xl:grid-cols-[minmax(340px,0.9fr)_1.6fr]">
          <div className="space-y-4">
            <StatsPanel
              sample={telemetry.sample}
              cpuLimit={server.cpu_limit}
              ramMb={server.ram_mb}
              live={telemetry.status === 'live'}
            />

            <GlassCard className="animate-rise p-5">
              <h2 className="text-sm font-semibold">Container</h2>
              <dl className="mt-4 space-y-3 text-xs">
                <DetailRow label="Runtime" value={meta.label} />
                <DetailRow label="Version" value={server.game_version} mono />
                <DetailRow label="Memory limit" value={formatRam(server.ram_mb)} />
                <DetailRow label="CPU limit" value={formatCpu(server.cpu_limit)} />
                <DetailRow
                  label="Host binding"
                  value={
                    server.is_behind_proxy ? 'Docker network only' : `0.0.0.0:${server.game_port}`
                  }
                />
                <DetailRow label="Container" value={`mc-${server.id.slice(0, 8)}…`} mono />
              </dl>
            </GlassCard>
          </div>

          <LogConsole logs={telemetry.logs} status={telemetry.status} onClear={telemetry.clearLogs} />
        </div>
      )}
    </div>
  )
}

function TabButton({
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
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={cn(
        'relative -mb-px flex items-center rounded-t-lg px-3.5 py-2.5 text-sm font-medium transition',
        active
          ? 'text-text-primary'
          : 'text-text-dim hover:bg-white/[0.03] hover:text-text-muted',
      )}
    >
      {children}
      {active ? (
        <span className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-neon-emerald shadow-glow-emerald" />
      ) : null}
    </button>
  )
}

function BackButton({ onBack }: { onBack: () => void }) {
  return (
    <button
      type="button"
      onClick={onBack}
      className="inline-flex items-center gap-1.5 rounded-lg px-1.5 py-1 text-xs text-text-muted transition hover:bg-white/5 hover:text-text-primary"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" className="size-4">
        <path d="m15 18-6-6 6-6" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      All servers
    </button>
  )
}

function DetailRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-white/[0.06] pb-3 last:border-0 last:pb-0">
      <dt className="text-text-dim">{label}</dt>
      <dd className={cn('text-right text-text-muted', mono && 'font-mono text-[10px]')}>{value}</dd>
    </div>
  )
}

function DetailSkeleton({ onBack }: { onBack: () => void }) {
  return (
    <div>
      <BackButton onBack={onBack} />
      <div className="mt-6 grid gap-4 xl:grid-cols-[minmax(340px,0.9fr)_1.6fr]" aria-hidden="true">
        <div className="space-y-4">
          <div className="glass-deep h-64 animate-pulse rounded-glass" />
          <div className="glass-deep h-52 animate-pulse rounded-glass" />
        </div>
        <div className="glass-deep h-[520px] animate-pulse rounded-glass" />
      </div>
    </div>
  )
}
