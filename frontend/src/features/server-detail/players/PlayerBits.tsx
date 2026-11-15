import { useState } from 'react'

import { cn } from '../../../lib/cn'

/**
 * Player skin from Crafatar. Falls back to a monogram when the avatar service is
 * unreachable (offline development, rate limit, unknown offline-mode name).
 */
export function PlayerAvatar({
  username,
  size = 'md',
  className,
}: {
  username: string
  size?: 'sm' | 'md' | 'lg'
  className?: string
}) {
  const [failed, setFailed] = useState(false)
  const dimensions = size === 'lg' ? 'size-14' : size === 'sm' ? 'size-8' : 'size-11'

  return (
    <span
      className={cn(
        'relative grid shrink-0 place-items-center overflow-hidden rounded-xl border border-white/10 bg-gradient-to-br from-neon-emerald/25 via-neon-cyan/15 to-neon-violet/25 font-mono font-semibold text-text-primary',
        dimensions,
        size === 'lg' ? 'text-lg' : 'text-sm',
        className,
      )}
      aria-hidden="true"
    >
      {username.slice(0, 2).toUpperCase()}
      {!failed ? (
        <img
          src={`https://crafatar.com/avatars/${encodeURIComponent(username)}?size=96&overlay`}
          alt=""
          loading="lazy"
          className="absolute inset-0 size-full object-cover [image-rendering:pixelated]"
          onError={() => setFailed(true)}
        />
      ) : null}
    </span>
  )
}

/** Four-bar latency meter. Lower is better; thresholds mirror common server UIs. */
export function PingMeter({ ping }: { ping: number | null }) {
  if (ping === null) {
    return (
      <span className="inline-flex items-center gap-1.5" title="Latency is provided by the companion plugin">
        <span className="flex items-end gap-0.5" aria-hidden="true">
          {[1, 2, 3, 4].map((bar) => (
            <span
              key={bar}
              className="w-0.5 rounded-full bg-white/15"
              style={{ height: `${3 + bar * 2}px` }}
            />
          ))}
        </span>
        <span className="font-mono text-[10px] text-text-dim">— ms</span>
      </span>
    )
  }

  const bars = ping < 40 ? 4 : ping < 90 ? 3 : ping < 160 ? 2 : 1
  const tone =
    bars >= 4
      ? 'bg-neon-emerald'
      : bars === 3
        ? 'bg-neon-cyan'
        : bars === 2
          ? 'bg-neon-amber'
          : 'bg-neon-rose'

  return (
    <span className="inline-flex items-center gap-1.5" title={`${ping} ms`}>
      <span className="flex items-end gap-0.5" aria-hidden="true">
        {[1, 2, 3, 4].map((bar) => (
          <span
            key={bar}
            className={cn(
              'w-0.5 rounded-full transition-colors',
              bar <= bars ? tone : 'bg-white/15',
            )}
            style={{ height: `${3 + bar * 2}px` }}
          />
        ))}
      </span>
      <span className="font-mono text-[10px] text-text-muted tabular-nums">{ping} ms</span>
    </span>
  )
}
