import { useEffect, useMemo, useRef, useState } from 'react'

import { cn } from '../../lib/cn'
import { parseLog, type LogTone, type ParsedLog } from '../../lib/logs'
import type { LogLine, TelemetryStatus } from '../../lib/types'
import { Button } from '../../components/ui/Button'
import { GlassCard } from '../../components/ui/GlassCard'

type Filter = 'all' | 'info' | 'warn' | 'error' | 'players'

const FILTERS: Array<{ id: Filter; label: string }> = [
  { id: 'all', label: 'All' },
  { id: 'info', label: 'Info' },
  { id: 'warn', label: 'Warnings' },
  { id: 'error', label: 'Errors' },
  { id: 'players', label: 'Players' },
]

const TONE: Record<
  LogTone,
  { row: string; badge: string; time: string; message: string }
> = {
  info: {
    row: 'hover:bg-white/[0.03]',
    badge: 'border-white/10 bg-white/5 text-text-muted',
    time: 'text-text-dim',
    message: 'text-text-primary/90',
  },
  warn: {
    row: 'bg-neon-amber/[0.05] hover:bg-neon-amber/[0.08]',
    badge: 'border-neon-amber/30 bg-neon-amber/10 text-neon-amber',
    time: 'text-neon-amber/60',
    message: 'text-neon-amber/95',
  },
  error: {
    row: 'bg-neon-rose/[0.06] hover:bg-neon-rose/[0.1]',
    badge: 'border-neon-rose/35 bg-neon-rose/10 text-neon-rose',
    time: 'text-neon-rose/60',
    message: 'text-neon-rose/95',
  },
  join: {
    row: 'bg-neon-emerald/[0.06] hover:bg-neon-emerald/[0.1]',
    badge: 'border-neon-emerald/35 bg-neon-emerald/10 text-neon-emerald',
    time: 'text-neon-emerald/60',
    message: 'text-neon-emerald/95',
  },
  leave: {
    row: 'bg-white/[0.02] hover:bg-white/[0.04]',
    badge: 'border-white/15 bg-white/5 text-text-muted',
    time: 'text-text-dim',
    message: 'text-text-muted',
  },
  chat: {
    row: 'hover:bg-neon-cyan/[0.05]',
    badge: 'border-neon-cyan/30 bg-neon-cyan/10 text-neon-cyan',
    time: 'text-neon-cyan/60',
    message: 'text-neon-cyan/95',
  },
  debug: {
    row: 'hover:bg-white/[0.03]',
    badge: 'border-white/10 bg-white/5 text-text-dim',
    time: 'text-text-dim',
    message: 'text-text-dim',
  },
  system: {
    row: 'hover:bg-white/[0.03]',
    badge: 'border-white/10 bg-white/[0.03] text-text-dim',
    time: 'text-text-dim',
    message: 'text-text-muted',
  },
}

interface LogConsoleProps {
  logs: LogLine[]
  status: TelemetryStatus
  onClear: () => void
}

export function LogConsole({ logs, status, onClear }: LogConsoleProps) {
  const [filter, setFilter] = useState<Filter>('all')
  const [pinned, setPinned] = useState(true)
  const viewport = useRef<HTMLDivElement>(null)

  const parsed = useMemo(
    () => logs.map((line) => parseLog(line.message, line.stream, line.timestamp_ms)),
    [logs],
  )
  const counts = useMemo(() => countTones(parsed), [parsed])
  const visible = useMemo(() => parsed.filter((line) => matchesFilter(line, filter)), [parsed, filter])

  useEffect(() => {
    if (!pinned) return
    const element = viewport.current
    if (element) element.scrollTop = element.scrollHeight
  }, [visible.length, pinned])

  function onScroll() {
    const element = viewport.current
    if (!element) return
    const distance = element.scrollHeight - element.scrollTop - element.clientHeight
    setPinned(distance < 48)
  }

  function jumpToLatest() {
    const element = viewport.current
    if (element) element.scrollTop = element.scrollHeight
    setPinned(true)
  }

  return (
    <GlassCard className="animate-rise flex min-h-0 flex-col overflow-hidden">
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-white/[0.07] px-5 py-4">
        <div className="flex items-center gap-3">
          <span className="grid size-8 place-items-center rounded-lg border border-white/10 bg-black/25 text-neon-emerald">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" className="size-4">
              <path d="m5 7 5 5-5 5M13 17h6" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </span>
          <div>
            <h2 className="text-sm font-semibold">Graphical console</h2>
            <p className="mt-0.5 font-mono text-[10px] text-text-dim">
              {parsed.length} lines · {statusLabel(status)}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <Button size="sm" variant="ghost" onClick={onClear} disabled={logs.length === 0}>
            Clear
          </Button>
          <span
            className={cn(
              'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 font-mono text-[10px] tracking-[0.12em] uppercase',
              status === 'live'
                ? 'border-neon-emerald/30 bg-neon-emerald/10 text-neon-emerald'
                : 'border-white/10 bg-white/5 text-text-dim',
            )}
          >
            <span
              className={cn(
                'size-1.5 rounded-full',
                status === 'live' ? 'bg-neon-emerald' : 'bg-text-dim',
              )}
              style={
                status === 'live' ? { animation: 'pulse-glow 1.4s ease-in-out infinite' } : undefined
              }
            />
            {status === 'live' ? 'live' : status}
          </span>
        </div>
      </header>

      <div className="flex flex-wrap items-center gap-1.5 border-b border-white/[0.05] px-4 py-2.5">
        {FILTERS.map((option) => {
          const active = filter === option.id
          return (
            <button
              key={option.id}
              type="button"
              onClick={() => setFilter(option.id)}
              aria-pressed={active}
              className={cn(
                'rounded-full px-2.5 py-1 font-mono text-[10px] tracking-[0.1em] uppercase transition',
                active
                  ? 'bg-white/10 text-text-primary'
                  : 'text-text-dim hover:bg-white/5 hover:text-text-muted',
              )}
            >
              {option.label}
              {option.id !== 'all' ? (
                <span className="ml-1.5 text-[9px] opacity-60">{countFor(option.id, counts)}</span>
              ) : null}
            </button>
          )
        })}
        <span className="ml-auto font-mono text-[10px] text-text-dim">
          {visible.length} shown
        </span>
      </div>

      <div className="relative min-h-[260px] flex-1">
        <div
          ref={viewport}
          onScroll={onScroll}
          role="log"
          aria-live="polite"
          aria-label="Server console output"
          className="absolute inset-0 overflow-y-auto px-2 py-2 font-mono text-xs leading-relaxed"
        >
          {visible.length === 0 ? (
            <div className="grid h-full place-items-center px-6 text-center">
              <div>
                <p className="text-text-muted">
                  {logs.length === 0
                    ? 'Waiting for container output…'
                    : 'No lines match this filter.'}
                </p>
                <p className="mt-1.5 text-[10px] text-text-dim">
                  {status === 'live'
                    ? 'The log stream is connected.'
                    : 'Start the server to attach the log stream.'}
                </p>
                <span className="mt-4 inline-block h-3 w-1.5 animate-pulse rounded-full bg-neon-emerald/70" />
              </div>
            </div>
          ) : (
            <ul className="space-y-px">
              {visible.map((line, index) => (
                <LogRow key={`${line.time}-${index}-${line.message}`} line={line} />
              ))}
            </ul>
          )}
        </div>

        {!pinned && visible.length > 0 ? (
          <button
            type="button"
            onClick={jumpToLatest}
            className="absolute inset-x-0 bottom-3 mx-auto w-max rounded-full border border-neon-emerald/30 bg-void/90 px-3.5 py-1.5 font-mono text-[10px] text-neon-emerald shadow-glow-emerald backdrop-blur transition hover:bg-void"
          >
            ↓ Jump to latest
          </button>
        ) : null}
      </div>
    </GlassCard>
  )
}

function LogRow({ line }: { line: ParsedLog }) {
  const tone = TONE[line.tone]

  return (
    <li
      className={cn(
        'flex items-start gap-2.5 rounded-lg px-2.5 py-1.5 transition-colors',
        tone.row,
      )}
    >
      <span className={cn('mt-px shrink-0 tabular-nums', tone.time)}>{line.time}</span>
      <span
        className={cn(
          'mt-px w-[66px] shrink-0 truncate rounded border px-1.5 py-px text-center text-[9px] tracking-[0.08em] uppercase',
          tone.badge,
        )}
        title={line.source ?? line.stream}
      >
        {line.level}
      </span>
      <span className={cn('min-w-0 flex-1 break-words whitespace-pre-wrap', tone.message)}>
        {line.message}
      </span>
    </li>
  )
}

function matchesFilter(line: ParsedLog, filter: Filter): boolean {
  switch (filter) {
    case 'all':
      return true
    case 'warn':
      return line.tone === 'warn'
    case 'error':
      return line.tone === 'error'
    case 'players':
      return line.tone === 'join' || line.tone === 'leave' || line.tone === 'chat'
    case 'info':
      return line.tone === 'info' || line.tone === 'debug' || line.tone === 'system'
  }
}

function countTones(lines: ParsedLog[]) {
  return lines.reduce(
    (counts, line) => {
      counts[line.tone] += 1
      return counts
    },
    { info: 0, warn: 0, error: 0, join: 0, leave: 0, chat: 0, debug: 0, system: 0 },
  )
}

function countFor(filter: Exclude<Filter, 'all'>, counts: ReturnType<typeof countTones>): number {
  switch (filter) {
    case 'warn':
      return counts.warn
    case 'error':
      return counts.error
    case 'players':
      return counts.join + counts.leave + counts.chat
    case 'info':
      return counts.info + counts.debug + counts.system
  }
}

function statusLabel(status: TelemetryStatus): string {
  switch (status) {
    case 'live':
      return 'streaming'
    case 'connecting':
      return 'connecting'
    case 'error':
      return 'stream error'
    case 'ended':
      return 'detached'
  }
}
