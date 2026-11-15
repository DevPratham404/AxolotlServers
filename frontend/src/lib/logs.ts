import type { LogStreamKind } from './types'

/**
 * Tones drive the console colours. `join`/`leave` are player lifecycle events,
 * `chat` is in-game chat, and `system` covers anything without a Minecraft
 * level prefix (image init output, JVM stack traces, …).
 */
export type LogTone = 'info' | 'warn' | 'error' | 'join' | 'leave' | 'chat' | 'debug' | 'system'

export interface ParsedLog {
  time: string
  source: string | null
  level: string
  tone: LogTone
  message: string
  stream: LogStreamKind
}

const LEVEL_PATTERN = /^\[([^\]/]+)\/([A-Za-z]+)\]:?\s?(.*)$/
const TIME_PATTERN = /^\[(\d{2}:\d{2}:\d{2})\]\s*/
const PLAYER_JOIN = /(joined the game|has connected|is connecting)/i
const PLAYER_LEAVE = /(left the game|has disconnected|lost connection)/i

/**
 * Turn one daemon log line into structured fields for the graphical console.
 *
 * Handles `[12:00:00] [Server thread/INFO]: …` (Vanilla/Paper), proxy lines
 * such as `[server connection] …`, and raw image/JVM output. Unknown formats
 * degrade to a `system` row rather than being dropped.
 */
export function parseLog(message: string, stream: LogStreamKind, timestampMs: number): ParsedLog {
  let rest = message.trimEnd()
  let time = formatClock(timestampMs)

  const timeMatch = TIME_PATTERN.exec(rest)
  if (timeMatch) {
    time = timeMatch[1]
    rest = rest.slice(timeMatch[0].length)
  }

  const levelMatch = LEVEL_PATTERN.exec(rest)
  if (levelMatch) {
    const [, source, rawLevel, body] = levelMatch
    const level = rawLevel.toUpperCase()
    return {
      time,
      source,
      level,
      tone: toneFor(level, body),
      message: body,
      stream,
    }
  }

  return {
    time,
    source: null,
    level: stream === 'stderr' ? 'STDERR' : 'LOG',
    tone: toneFor(undefined, rest, stream),
    message: rest,
    stream,
  }
}

function toneFor(level: string | undefined, message: string, stream?: LogStreamKind): LogTone {
  const body = message.trim()

  if (PLAYER_JOIN.test(body)) return 'join'
  if (PLAYER_LEAVE.test(body)) return 'leave'
  if (body.startsWith('<') && body.includes('>')) return 'chat'

  switch (level) {
    case 'ERROR':
    case 'FATAL':
    case 'SEVERE':
      return 'error'
    case 'WARN':
    case 'WARNING':
      return 'warn'
    case 'DEBUG':
    case 'TRACE':
      return 'debug'
    case 'INFO':
      return 'info'
    default:
      break
  }

  // No recognised level: infer from stack traces and common failure wording.
  if (/^(at\s|\.\.\.\s*\d+ more|Caused by:)/.test(body)) return 'error'
  if (/\b(error|exception|failed|unable to)\b/i.test(body)) return 'error'
  if (/\bwarn(ing)?\b/i.test(body)) return 'warn'
  if (stream === 'stderr') return 'warn'
  if (body.startsWith('[')) return 'system'
  return 'info'
}

export function formatClock(timestampMs: number): string {
  if (!Number.isFinite(timestampMs) || timestampMs <= 0) return '--:--:--'
  const date = new Date(timestampMs)
  if (Number.isNaN(date.getTime())) return '--:--:--'
  return new Intl.DateTimeFormat(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).format(date)
}
