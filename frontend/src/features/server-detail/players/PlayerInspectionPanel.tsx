import { useEffect, useState } from 'react'

import { cn } from '../../../lib/cn'
import { ApiError, serverApi } from '../../../lib/api'
import type { PlayerAccessLists, PlayerSummary } from '../../../lib/types'
import { Button } from '../../../components/ui/Button'
import { PlayerAvatar, PingMeter } from './PlayerBits'

type AccessField = 'operator' | 'whitelisted' | 'banned'

interface PlayerInspectionPanelProps {
  serverId: string
  player: PlayerSummary
  onAccessUpdated: (lists: PlayerAccessLists) => void
  onClose: () => void
}

/**
 * Slide-out inspection drawer with real, actionable access controls. All three
 * toggles work for offline players: the API edits the vanilla JSON lists when
 * the server is stopped and uses RCON when it is running.
 */
export function PlayerInspectionPanel({
  serverId,
  player,
  onAccessUpdated,
  onClose,
}: PlayerInspectionPanelProps) {
  const [busy, setBusy] = useState<AccessField | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(null), 3500)
    return () => window.clearTimeout(timer)
  }, [notice])

  async function toggle(field: AccessField, next: boolean) {
    setBusy(field)
    setError(null)
    setNotice(null)
    try {
      const lists = await serverApi.setPlayerAccess(serverId, player.username, {
        [field]: next,
      })
      onAccessUpdated(lists)
      setNotice(`${ACCESS_COPY[field].label} ${next ? 'enabled' : 'disabled'}.`)
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : `Could not update ${ACCESS_COPY[field].label.toLowerCase()}.`,
      )
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="fixed inset-0 z-50">
      <button
        type="button"
        aria-label="Close player inspection"
        onClick={onClose}
        className="animate-fade-in absolute inset-0 bg-void/70 backdrop-blur-sm"
      />

      <aside
        role="dialog"
        aria-modal="true"
        aria-label={`Inspect ${player.username}`}
        className="animate-slide-in absolute inset-y-0 right-0 flex w-full max-w-md flex-col overflow-y-auto border-l border-white/10 bg-abyss/95 shadow-glass"
      >
        <header className="flex items-start justify-between gap-4 border-b border-white/[0.07] px-6 py-5">
          <div className="flex min-w-0 items-center gap-4">
            <div className="relative">
              <PlayerAvatar username={player.username} size="lg" />
              <span
                className={cn(
                  'absolute -right-0.5 -bottom-0.5 size-3 rounded-full border-2 border-abyss',
                  player.online ? 'bg-neon-emerald' : 'bg-white/30',
                )}
              />
            </div>
            <div className="min-w-0">
              <h2 className="truncate text-xl font-semibold tracking-tight">{player.username}</h2>
              <div className="mt-1 flex flex-wrap items-center gap-2">
                <span
                  className={cn(
                    'inline-flex items-center gap-1.5 font-mono text-[10px] tracking-[0.12em] uppercase',
                    player.online ? 'text-neon-emerald' : 'text-text-dim',
                  )}
                >
                  <span
                    className={cn(
                      'size-1.5 rounded-full',
                      player.online ? 'bg-neon-emerald' : 'bg-white/40',
                    )}
                  />
                  {player.online ? 'online' : 'offline'}
                </span>
                {player.online ? (
                  <>
                    <span className="text-text-dim">·</span>
                    <PingMeter ping={player.ping} />
                  </>
                ) : null}
                {player.operator ? (
                  <span className="rounded border border-neon-violet/30 bg-neon-violet/10 px-1.5 py-px font-mono text-[9px] tracking-[0.1em] text-neon-violet uppercase">
                    op
                  </span>
                ) : null}
              </div>
            </div>
          </div>

          <Button variant="ghost" size="sm" aria-label="Close" onClick={onClose}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" className="size-4">
              <path d="m6 6 12 12M18 6 6 18" strokeLinecap="round" />
            </svg>
          </Button>
        </header>

        {notice ? (
          <p className="border-b border-neon-emerald/20 bg-neon-emerald/[0.07] px-6 py-2.5 text-[11px] text-neon-emerald">
            {notice}
          </p>
        ) : null}
        {error ? (
          <p role="alert" className="border-b border-neon-rose/20 bg-neon-rose/[0.07] px-6 py-2.5 text-[11px] text-neon-rose">
            {error}
          </p>
        ) : null}

        <div className="space-y-5 px-6 py-6">
          <section className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-4">
            <dl className="grid grid-cols-2 gap-4 text-xs">
              <div>
                <dt className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
                  Status
                </dt>
                <dd className="mt-1 text-text-primary">{player.online ? 'Online' : 'Offline'}</dd>
              </div>
              <div>
                <dt className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
                  UUID
                </dt>
                <dd className="mt-1 truncate font-mono text-[10px] text-text-primary" title={player.uuid ?? undefined}>
                  {player.uuid ?? 'Unknown'}
                </dd>
              </div>
              {player.online ? (
                <>
                  <div>
                    <dt className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
                      Gamemode
                    </dt>
                    <dd className="mt-1 text-text-primary">{player.gamemode ?? 'Unknown'}</dd>
                  </div>
                  <div>
                    <dt className="font-mono text-[9px] tracking-[0.14em] text-text-dim uppercase">
                      Session
                    </dt>
                    <dd className="mt-1 text-text-primary">{player.joinedAgo ?? 'Unknown'}</dd>
                  </div>
                </>
              ) : null}
            </dl>
          </section>

          <section className="rounded-xl border border-white/[0.07] bg-white/[0.02] p-4">
            <div className="mb-3 flex items-center justify-between gap-3">
              <div>
                <h3 className="text-sm font-semibold">Access & moderation</h3>
                <p className="mt-0.5 text-[10px] text-text-dim">
                  Works online via RCON and offline via server JSON files.
                </p>
              </div>
            </div>

            <div className="space-y-2">
              <AccessToggle
                label={ACCESS_COPY.operator.label}
                description={ACCESS_COPY.operator.description}
                enabled={player.operator}
                busy={busy === 'operator'}
                tone="violet"
                onToggle={(next) => void toggle('operator', next)}
              />
              <AccessToggle
                label={ACCESS_COPY.whitelisted.label}
                description={ACCESS_COPY.whitelisted.description}
                enabled={player.whitelisted}
                busy={busy === 'whitelisted'}
                tone="cyan"
                onToggle={(next) => void toggle('whitelisted', next)}
              />
              <AccessToggle
                label={ACCESS_COPY.banned.label}
                description={ACCESS_COPY.banned.description}
                enabled={player.banned}
                busy={busy === 'banned'}
                tone="rose"
                onToggle={(next) => void toggle('banned', next)}
              />
            </div>
          </section>

          {player.online ? (
            <p className="text-[11px] leading-relaxed text-text-dim">
              Kick is available from the roster card. Access changes on a running server are applied
              immediately over RCON; while stopped they edit{' '}
              <span className="font-mono">ops.json</span>,{' '}
              <span className="font-mono">whitelist.json</span> and{' '}
              <span className="font-mono">banned-players.json</span> in the server volume.
            </p>
          ) : (
            <p className="text-[11px] leading-relaxed text-text-dim">
              This player is offline. Their identity comes from{' '}
              <span className="font-mono">usercache.json</span>, and access changes are written
              directly to the vanilla JSON lists.
            </p>
          )}
        </div>
      </aside>
    </div>
  )
}

const ACCESS_COPY: Record<AccessField, { label: string; description: string }> = {
  operator: {
    label: 'Operator',
    description: 'Full command access via the server console.',
  },
  whitelisted: {
    label: 'Whitelist',
    description: 'Allowed to join when the whitelist is enforced.',
  },
  banned: {
    label: 'Ban',
    description: 'Blocked from connecting to this server.',
  },
}

const TONES = {
  violet: {
    on: 'border-neon-violet/40 bg-neon-violet/15 text-neon-violet',
    dot: 'bg-neon-violet',
  },
  cyan: {
    on: 'border-neon-cyan/40 bg-neon-cyan/15 text-neon-cyan',
    dot: 'bg-neon-cyan',
  },
  rose: {
    on: 'border-neon-rose/40 bg-neon-rose/15 text-neon-rose',
    dot: 'bg-neon-rose',
  },
}

function AccessToggle({
  label,
  description,
  enabled,
  busy,
  tone,
  onToggle,
}: {
  label: string
  description: string
  enabled: boolean
  busy: boolean
  tone: keyof typeof TONES
  onToggle: (next: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-white/[0.06] bg-black/20 px-3 py-2.5">
      <div className="min-w-0">
        <p className="text-xs font-medium text-text-primary">{label}</p>
        <p className="mt-0.5 text-[10px] leading-snug text-text-dim">{description}</p>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={enabled}
        aria-label={`${label} ${enabled ? 'enabled' : 'disabled'}`}
        disabled={busy}
        onClick={() => onToggle(!enabled)}
        className={cn(
          'relative inline-flex h-6 w-11 shrink-0 items-center rounded-full border transition',
          enabled ? TONES[tone].on : 'border-white/10 bg-white/5',
          busy && 'cursor-wait opacity-60',
        )}
      >
        <span
          className={cn(
            'absolute size-4 rounded-full transition-transform',
            enabled ? cn('translate-x-6', TONES[tone].dot) : 'translate-x-1 bg-white/40',
          )}
        />
      </button>
    </div>
  )
}
