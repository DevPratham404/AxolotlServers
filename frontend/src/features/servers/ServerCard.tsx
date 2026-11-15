import { useState } from 'react'

import { cn } from '../../lib/cn'
import { formatCpu, formatDate, formatRam, serverTypeMeta } from '../../lib/servers'
import type { ServerView } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { GlassCard } from '../../components/ui/GlassCard'
import { StatusPill } from '../../components/ui/StatusPill'

export type ServerAction = 'start' | 'stop' | 'delete' | null

interface ServerCardProps {
  server: ServerView
  busy: ServerAction
  onOpen: (server: ServerView) => void
  onStart: (server: ServerView) => void
  onStop: (server: ServerView) => void
  onDelete: (server: ServerView) => void
}

export function ServerCard({ server, busy, onOpen, onStart, onStop, onDelete }: ServerCardProps) {
  const meta = serverTypeMeta(server.server_type)
  const [confirmingDelete, setConfirmingDelete] = useState(false)
  const pending = busy !== null

  const canStart = server.status === 'stopped' || server.status === 'error'
  const canStop = server.status === 'running' || server.status === 'starting'

  return (
    <GlassCard
      interactive
      className="animate-rise flex h-full cursor-pointer flex-col overflow-hidden p-5 focus-visible:ring-2 focus-visible:ring-neon-emerald/40 focus-visible:outline-none"
      role="button"
      tabIndex={0}
      aria-label={`Open ${server.name} console`}
      onClick={() => onOpen(server)}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          onOpen(server)
        }
      }}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <span
            className={cn(
              'grid size-11 shrink-0 place-items-center rounded-xl border border-white/10 bg-black/30 font-mono text-lg font-semibold',
              meta.accent,
              meta.glow,
            )}
          >
            {meta.monogram}
          </span>
          <div className="min-w-0">
            <h3 className="truncate text-base font-semibold tracking-tight">{server.name}</h3>
            <p className="mt-0.5 font-mono text-[10px] tracking-[0.14em] text-text-dim uppercase">
              {meta.label}
              {meta.proxy ? ' · proxy' : ''}
            </p>
          </div>
        </div>
        <StatusPill status={server.status} />
      </div>

      <dl className="mt-5 grid grid-cols-3 gap-2 rounded-xl border border-white/[0.06] bg-black/20 p-3">
        <div>
          <dt className="text-[10px] tracking-[0.14em] text-text-dim uppercase">RAM</dt>
          <dd className="mt-1 font-mono text-sm">{formatRam(server.ram_mb)}</dd>
        </div>
        <div>
          <dt className="text-[10px] tracking-[0.14em] text-text-dim uppercase">CPU</dt>
          <dd className="mt-1 font-mono text-sm">{formatCpu(server.cpu_limit)}</dd>
        </div>
        <div>
          <dt className="text-[10px] tracking-[0.14em] text-text-dim uppercase">Port</dt>
          <dd className="mt-1 font-mono text-sm">
            {server.is_behind_proxy ? 'private' : server.game_port}
          </dd>
        </div>
      </dl>

      <div className="mt-4 flex items-center justify-between gap-2 text-[10px] text-text-dim">
        <span className="inline-flex items-center gap-1.5">
          <span
            className={cn(
              'size-1.5 rounded-full',
              server.status === 'running' ? 'bg-neon-emerald' : 'bg-text-dim',
            )}
          />
          {server.is_behind_proxy ? 'Docker network only' : `0.0.0.0:${server.game_port}`}
        </span>
        <span title={server.updated_at}>updated {formatDate(server.updated_at)}</span>
      </div>

      <div
        className="mt-auto flex flex-wrap items-center gap-2 pt-5"
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => event.stopPropagation()}
      >
        {canStart ? (
          <Button
            size="sm"
            variant="primary"
            loading={busy === 'start'}
            disabled={pending}
            onClick={() => onStart(server)}
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
            size="sm"
            loading={busy === 'stop'}
            disabled={pending}
            onClick={() => onStop(server)}
            icon={
              <svg viewBox="0 0 16 16" className="size-3.5" fill="currentColor" aria-hidden="true">
                <rect x="3.5" y="3.5" width="9" height="9" rx="1.4" />
              </svg>
            }
          >
            Stop
          </Button>
        ) : null}

        {!canStart && !canStop ? (
          <span className="font-mono text-[10px] tracking-[0.12em] text-text-dim uppercase">
            reconciling…
          </span>
        ) : null}

        <div className="ml-auto">
          {confirmingDelete ? (
            <div className="flex items-center gap-1.5">
              <Button
                size="sm"
                variant="danger"
                loading={busy === 'delete'}
                disabled={pending}
                onClick={() => onDelete(server)}
              >
                Confirm
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={pending}
                onClick={() => setConfirmingDelete(false)}
              >
                Cancel
              </Button>
            </div>
          ) : (
            <Button
              size="sm"
              variant="ghost"
              disabled={pending}
              onClick={() => setConfirmingDelete(true)}
              aria-label={`Delete ${server.name}`}
            >
              Delete
            </Button>
          )}
        </div>
      </div>
    </GlassCard>
  )
}
