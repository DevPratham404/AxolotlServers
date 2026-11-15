import { cn } from '../../lib/cn'
import type { StatsSample } from '../../lib/types'
import { GlassCard } from '../../components/ui/GlassCard'

interface StatsPanelProps {
  sample: StatsSample | null
  cpuLimit: number
  ramMb: number
  live: boolean
}

export function StatsPanel({ sample, cpuLimit, ramMb, live }: StatsPanelProps) {
  const cpuPercent = sample?.cpu_percent ?? 0
  const cpuOfAllocation = clamp((cpuPercent / Math.max(cpuLimit, 0.1) / 100) * 100)
  const memoryLimit = sample?.memory_limit_bytes || ramMb * 1024 * 1024
  const memoryUsed = sample?.memory_used_bytes ?? 0
  const memoryPercent = sample ? clamp(sample.memory_percent) : 0
  const coresUsed = cpuPercent / 100

  return (
    <GlassCard className="animate-rise p-5 sm:p-6">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold">Live resources</h2>
          <p className="mt-1 text-xs text-text-muted">
            Docker Engine samples, refreshed roughly once per second.
          </p>
        </div>
        <span
          className={cn(
            'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 font-mono text-[10px] tracking-[0.14em] uppercase',
            live
              ? 'border-neon-emerald/30 bg-neon-emerald/10 text-neon-emerald'
              : 'border-white/10 bg-white/5 text-text-dim',
          )}
        >
          <span
            className={cn('size-1.5 rounded-full', live ? 'bg-neon-emerald' : 'bg-text-dim')}
            style={live ? { animation: 'pulse-glow 1.4s ease-in-out infinite' } : undefined}
          />
          {live ? 'streaming' : 'offline'}
        </span>
      </div>

      <div className="mt-6 grid items-center gap-6 sm:grid-cols-[150px_1fr]">
        <Ring
          value={cpuOfAllocation}
          accent={cpuOfAllocation > 85 ? '#ff5d7a' : cpuOfAllocation > 65 ? '#ffb84d' : '#3cf5a6'}
          label="CPU"
          valueLabel={sample ? `${Math.round(cpuOfAllocation)}%` : '—'}
          caption={sample ? `${coresUsed.toFixed(2)} / ${cpuLimit} cores` : 'waiting'}
        />

        <div className="space-y-5">
          <Meter
            label="Memory"
            valueLabel={sample ? `${formatBytes(memoryUsed)} / ${formatBytes(memoryLimit)}` : '—'}
            percent={memoryPercent}
            accent={memoryPercent > 90 ? '#ff5d7a' : memoryPercent > 75 ? '#ffb84d' : '#35d6f5'}
            detail={sample ? `${memoryLabel(memoryPercent)} of the hard limit` : 'waiting for sample'}
          />

          <div className="grid grid-cols-3 gap-2">
            <Metric
              label="RX"
              value={sample ? formatBytes(sample.network_rx_bytes) : '—'}
              accent="text-neon-cyan"
            />
            <Metric
              label="TX"
              value={sample ? formatBytes(sample.network_tx_bytes) : '—'}
              accent="text-neon-violet"
            />
            <Metric label="PIDs" value={sample ? String(sample.pids) : '—'} accent="text-text-primary" />
          </div>
        </div>
      </div>
    </GlassCard>
  )
}

function Ring({
  value,
  accent,
  label,
  valueLabel,
  caption,
}: {
  value: number
  accent: string
  label: string
  valueLabel: string
  caption: string
}) {
  const radius = 52
  const circumference = 2 * Math.PI * radius
  const offset = circumference - (clamp(value) / 100) * circumference

  return (
    <div className="flex flex-col items-center">
      <div className="relative size-[132px]">
        <svg viewBox="0 0 132 132" className="size-full -rotate-90" aria-hidden="true">
          <circle
            cx="66"
            cy="66"
            r={radius}
            fill="none"
            stroke="rgba(255,255,255,0.07)"
            strokeWidth="10"
          />
          <circle
            cx="66"
            cy="66"
            r={radius}
            fill="none"
            stroke={accent}
            strokeWidth="10"
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={offset}
            style={{
              transition: 'stroke-dashoffset 600ms cubic-bezier(0.22,1,0.36,1), stroke 300ms ease',
              filter: `drop-shadow(0 0 6px ${accent}66)`,
            }}
          />
        </svg>
        <div className="absolute inset-0 flex flex-col items-center justify-center">
          <span className="font-mono text-2xl font-medium tracking-tight">{valueLabel}</span>
          <span className="mt-0.5 font-mono text-[10px] tracking-[0.18em] text-text-dim uppercase">
            {label}
          </span>
        </div>
      </div>
      <p className="mt-2 font-mono text-[10px] text-text-dim">{caption}</p>
    </div>
  )
}

function Meter({
  label,
  valueLabel,
  percent,
  accent,
  detail,
}: {
  label: string
  valueLabel: string
  percent: number
  accent: string
  detail: string
}) {
  return (
    <div>
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-xs font-medium">{label}</span>
        <span className="font-mono text-xs text-text-muted">{valueLabel}</span>
      </div>
      <div className="mt-2 h-2.5 overflow-hidden rounded-full bg-white/[0.07]">
        <div
          className="h-full rounded-full transition-[width] duration-700 ease-out"
          style={{
            width: `${clamp(percent)}%`,
            background: `linear-gradient(90deg, ${accent}99, ${accent})`,
            boxShadow: `0 0 14px ${accent}66`,
          }}
        />
      </div>
      <p className="mt-1.5 text-[10px] text-text-dim">{detail}</p>
    </div>
  )
}

function Metric({ label, value, accent }: { label: string; value: string; accent: string }) {
  return (
    <div className="rounded-xl border border-white/[0.06] bg-black/20 px-3 py-2.5">
      <p className="font-mono text-[9px] tracking-[0.16em] text-text-dim uppercase">{label}</p>
      <p className={cn('mt-1 truncate font-mono text-[10px]', accent)}>{value}</p>
    </div>
  )
}

function clamp(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.min(100, Math.max(0, value))
}

function memoryLabel(percent: number): string {
  if (percent > 0 && percent < 1) return '<1%'
  return `${Math.round(percent)}%`
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB']
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** exponent
  return `${value >= 100 || exponent === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[exponent]}`
}
