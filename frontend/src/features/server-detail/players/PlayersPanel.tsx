import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type MouseEvent as ReactMouseEvent,
} from 'react'

import { cn } from '../../../lib/cn'
import { ApiError, serverApi } from '../../../lib/api'
import type {
  PlayerAccessLists,
  PlayerAction,
  PlayerHistoryEntry,
  PlayerSummary,
} from '../../../lib/types'
import { GlassCard } from '../../../components/ui/GlassCard'
import { PlayerAvatar, PingMeter } from './PlayerBits'
import { PlayerInspectionPanel } from './PlayerInspectionPanel'

/** RCON `list` is cheap, but 6 s keeps polling unobtrusive. */
const POLL_INTERVAL_MS = 6000

const EMPTY_ACCESS: PlayerAccessLists = { operators: [], whitelisted: [], banned: [] }

interface PlayersPanelProps {
  serverId: string
  running: boolean
}

export function PlayersPanel({ serverId, running }: PlayersPanelProps) {
  const [online, setOnline] = useState<string[]>([])
  const [history, setHistory] = useState<PlayerHistoryEntry[]>([])
  const [access, setAccess] = useState<PlayerAccessLists>(EMPTY_ACCESS)
  const [selected, setSelected] = useState<string | null>(null)
  const [busy, setBusy] = useState<{ username: string; action: PlayerAction } | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [rosterError, setRosterError] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)

  const refreshRoster = useCallback(async () => {
    setRefreshing(true)
    try {
      const [historyEntries, accessLists, usernames] = await Promise.all([
        serverApi.listPlayerHistory(serverId),
        serverApi.getPlayerAccess(serverId),
        running ? serverApi.listPlayers(serverId) : Promise.resolve<string[]>([]),
      ])
      setOnline(usernames)
      setHistory(historyEntries)
      setAccess(accessLists)
      setRosterError(null)
    } catch (caught) {
      setRosterError(
        caught instanceof ApiError ? caught.message : 'Could not refresh the player roster.',
      )
    } finally {
      setRefreshing(false)
    }
  }, [running, serverId])

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    void refreshRoster()
  }, [refreshRoster])

  useEffect(() => {
    if (!running) return
    const timer = window.setInterval(() => void refreshRoster(), POLL_INTERVAL_MS)
    return () => window.clearInterval(timer)
  }, [refreshRoster, running])

  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(null), 4500)
    return () => window.clearTimeout(timer)
  }, [notice])

  const players = useMemo(() => {
    const cached = new Map(history.map((entry) => [entry.name.toLowerCase(), entry]))
    const onlineNames = new Set(online.map((name) => name.toLowerCase()))
    const names = [
      ...online,
      ...history
        .map((entry) => entry.name)
        .filter((name) => !onlineNames.has(name.toLowerCase())),
    ]

    return names.map<PlayerSummary>((name) => {
      const key = name.toLowerCase()
      const entry = cached.get(key)
      return {
        username: name,
        online: onlineNames.has(key),
        uuid: entry?.uuid ?? null,
        operator: access.operators.some((value) => value.toLowerCase() === key),
        whitelisted: access.whitelisted.some((value) => value.toLowerCase() === key),
        banned: access.banned.some((value) => value.toLowerCase() === key),
        ping: null,
      }
    })
  }, [access, history, online])

  const selectedPlayer = selected
    ? (players.find((player) => player.username.toLowerCase() === selected.toLowerCase()) ?? null)
    : null

  async function kick(player: PlayerSummary) {
    if (!player.online) {
      setError('Only online players can be kicked.')
      return
    }

    setBusy({ username: player.username, action: 'kick' })
    setError(null)
    setNotice(null)
    try {
      const result = await serverApi.playerAction(
        serverId,
        player.username,
        'kick',
        'Removed by an operator',
      )
      setOnline((current) =>
        current.filter((name) => name.toLowerCase() !== player.username.toLowerCase()),
      )
      const output = result.output.trim()
      setNotice(`Kicked ${player.username}${output ? ` — ${output}` : ''}`)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : `Could not kick ${player.username}.`)
    } finally {
      setBusy(null)
    }
  }

  async function toggleAccess(player: PlayerSummary, field: 'operator' | 'banned') {
    const next = !player[field]
    setBusy({ username: player.username, action: field === 'operator' ? 'op' : 'ban' })
    setError(null)
    setNotice(null)
    try {
      const lists = await serverApi.setPlayerAccess(serverId, player.username, {
        [field]: next,
      })
      setAccess(lists)
      setNotice(
        field === 'operator'
          ? `${next ? 'Op granted' : 'Op revoked'} for ${player.username}`
          : `${next ? 'Banned' : 'Unbanned'} ${player.username}`,
      )
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : `Could not update ${player.username}'s access.`,
      )
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="space-y-4">
      <GlassCard className="animate-rise overflow-hidden">
        <header className="flex flex-wrap items-center justify-between gap-3 border-b border-white/[0.07] px-5 py-4">
          <div className="flex items-center gap-3">
            <span className="grid size-8 place-items-center rounded-lg border border-white/10 bg-black/25 text-neon-cyan">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" className="size-4">
                <circle cx="9" cy="8" r="3" />
                <path d="M3.5 20a5.5 5.5 0 0 1 11 0M16 5.5a3 3 0 0 1 0 5.8M18.5 20a5.2 5.2 0 0 0-2.2-4.2" strokeLinecap="round" />
              </svg>
            </span>
            <div>
              <h2 className="text-sm font-semibold">Player roster</h2>
              <p className="mt-0.5 font-mono text-[10px] text-text-dim">
                {online.length} online · {players.length} known
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <span
              className={cn(
                'rounded-full border px-2.5 py-1 font-mono text-[9px] tracking-[0.1em] uppercase',
                refreshing
                  ? 'border-neon-cyan/25 bg-neon-cyan/10 text-neon-cyan'
                  : 'border-neon-emerald/25 bg-neon-emerald/10 text-neon-emerald',
              )}
            >
              {refreshing ? 'refreshing' : running ? 'live rcon' : 'cache'}
            </span>
            <span
              className={cn(
                'rounded-full border px-2.5 py-1 font-mono text-[9px] tracking-[0.1em] uppercase',
                running
                  ? 'border-neon-emerald/30 bg-neon-emerald/10 text-neon-emerald'
                  : 'border-white/10 bg-white/5 text-text-dim',
              )}
            >
              {running ? 'server online' : 'server offline'}
            </span>
          </div>
        </header>

        {notice ? (
          <p className="border-b border-neon-emerald/15 bg-neon-emerald/[0.06] px-5 py-2.5 text-[11px] text-neon-emerald">
            {notice}
          </p>
        ) : null}
        {error ? (
          <p className="border-b border-neon-rose/20 bg-neon-rose/[0.07] px-5 py-2.5 text-[11px] text-neon-rose">
            {error}
          </p>
        ) : null}
        {rosterError ? (
          <p className="border-b border-neon-amber/20 bg-neon-amber/[0.07] px-5 py-2.5 text-[11px] text-neon-amber">
            {rosterError}
          </p>
        ) : null}

        <div className="p-5">
          {players.length === 0 ? (
            <div className="grid place-items-center rounded-xl border border-white/[0.07] bg-black/20 px-6 py-14 text-center">
              <p className="text-sm text-text-muted">No players known yet.</p>
              <p className="mt-1 text-[11px] text-text-dim">
                Online players appear over RCON; everyone who has joined is read from{' '}
                <span className="font-mono">usercache.json</span>.
              </p>
            </div>
          ) : (
            <ul className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-3">
              {players.map((player) => (
                <li key={player.username}>
                  <PlayerCard
                    player={player}
                    busy={busy?.username === player.username ? busy.action : null}
                    onInspect={() => setSelected(player.username)}
                    onKick={() => void kick(player)}
                    onToggleAccess={(field) => void toggleAccess(player, field)}
                  />
                </li>
              ))}
            </ul>
          )}
        </div>
      </GlassCard>

      {selectedPlayer ? (
        <PlayerInspectionPanel
          serverId={serverId}
          player={selectedPlayer}
          onAccessUpdated={setAccess}
          onClose={() => setSelected(null)}
        />
      ) : null}
    </div>
  )
}

function PlayerCard({
  player,
  busy,
  onInspect,
  onKick,
  onToggleAccess,
}: {
  player: PlayerSummary
  busy: PlayerAction | null
  onInspect: () => void
  onKick: () => void
  onToggleAccess: (field: 'operator' | 'banned') => void
}) {
  const pending = busy !== null
  const statusLabel = player.banned ? 'banned' : player.whitelisted ? 'whitelisted' : null

  return (
    <GlassCard
      interactive
      className="group animate-rise relative cursor-pointer p-4 focus-visible:ring-2 focus-visible:ring-neon-emerald/40 focus-visible:outline-none"
      role="button"
      tabIndex={0}
      aria-label={`Inspect ${player.username}`}
      onClick={onInspect}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          onInspect()
        }
      }}
    >
      <div className="flex items-center gap-3">
        <div className="relative">
          <PlayerAvatar username={player.username} />
          <span
            className={cn(
              'absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full border-2 border-abyss',
              player.online ? 'bg-neon-emerald' : 'bg-white/30',
            )}
            title={player.online ? 'Online' : 'Offline'}
          />
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <p className="truncate text-sm font-semibold">{player.username}</p>
            <span
              className={cn(
                'rounded border px-1 py-px font-mono text-[8px] tracking-[0.1em] uppercase',
                player.online
                  ? 'border-neon-emerald/30 bg-neon-emerald/10 text-neon-emerald'
                  : 'border-white/10 bg-white/5 text-text-dim',
              )}
            >
              {player.online ? 'online' : 'offline'}
            </span>
            {player.operator ? (
              <span
                className="rounded border border-neon-violet/30 bg-neon-violet/10 px-1 py-px font-mono text-[8px] tracking-[0.1em] text-neon-violet uppercase"
                title="Server operator"
              >
                op
              </span>
            ) : null}
            {statusLabel ? (
              <span
                className={cn(
                  'rounded border px-1 py-px font-mono text-[8px] tracking-[0.1em] uppercase',
                  player.banned
                    ? 'border-neon-rose/30 bg-neon-rose/10 text-neon-rose'
                    : 'border-neon-cyan/25 bg-neon-cyan/10 text-neon-cyan',
                )}
              >
                {statusLabel}
              </span>
            ) : null}
          </div>
          <p className="mt-0.5 font-mono text-[10px] text-text-dim">
            {player.online
              ? player.gamemode
                ? `${player.gamemode} · ${player.joinedAgo ?? 'session unknown'}`
                : 'Online · RCON roster'
              : 'Offline · usercache.json'}
          </p>
        </div>

        {player.online ? <PingMeter ping={player.ping} /> : null}
      </div>

      <div className="mt-3 flex items-center gap-1.5 border-t border-white/[0.06] pt-3 transition-opacity sm:opacity-0 sm:group-focus-within:opacity-100 sm:group-hover:opacity-100">
        <QuickAction
          label="Kick"
          title={player.online ? `Kick ${player.username}` : 'Only online players can be kicked'}
          disabled={!player.online || pending}
          loading={busy === 'kick'}
          tone="text-neon-amber hover:bg-neon-amber/10 border-neon-amber/25"
          onClick={(event) => {
            event.stopPropagation()
            onKick()
          }}
        />
        <QuickAction
          label={player.banned ? 'Unban' : 'Ban'}
          title={
            player.banned ? `Unban ${player.username}` : `Ban ${player.username} (works offline)`
          }
          disabled={pending}
          loading={busy === 'ban'}
          tone="text-neon-rose hover:bg-neon-rose/10 border-neon-rose/25"
          onClick={(event) => {
            event.stopPropagation()
            onToggleAccess('banned')
          }}
        />
        <QuickAction
          label={player.operator ? 'Deop' : 'Op'}
          title={
            player.operator
              ? `Remove ${player.username}'s operator status`
              : `Make ${player.username} an operator (works offline)`
          }
          disabled={pending}
          loading={busy === 'op'}
          tone="text-neon-violet hover:bg-neon-violet/10 border-neon-violet/25"
          onClick={(event) => {
            event.stopPropagation()
            onToggleAccess('operator')
          }}
        />
        <span className="ml-auto font-mono text-[9px] text-text-dim">inspect →</span>
      </div>
    </GlassCard>
  )
}

function QuickAction({
  label,
  title,
  disabled,
  loading,
  tone,
  onClick,
}: {
  label: string
  title: string
  disabled: boolean
  loading: boolean
  tone: string
  onClick: (event: ReactMouseEvent<HTMLButtonElement>) => void
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'rounded-md border px-2 py-1 font-mono text-[9px] tracking-[0.08em] uppercase transition',
        'disabled:cursor-not-allowed disabled:opacity-40',
        tone,
      )}
    >
      {loading ? '…' : label}
    </button>
  )
}
