import { useCallback, useEffect, useRef, useState } from 'react'

import { telemetrySocketUrl } from '../../lib/api'
import type { LogLine, StatsSample, TelemetryMessage, TelemetryStatus } from '../../lib/types'

const MAX_LOG_LINES = 600
const MAX_RECONNECT_DELAY_MS = 10_000

export interface ServerTelemetry {
  sample: StatsSample | null
  logs: LogLine[]
  status: TelemetryStatus
  error: string | null
  clearLogs: () => void
}

/**
 * Owns the telemetry WebSocket for one server.
 *
 * Reconnects with exponential backoff while `enabled` (the server is running).
 * The log buffer is capped so a long-lived console cannot grow without bound.
 */
export function useServerTelemetry(serverId: string, enabled: boolean): ServerTelemetry {
  const [sample, setSample] = useState<StatsSample | null>(null)
  const [logs, setLogs] = useState<LogLine[]>([])
  const [status, setStatus] = useState<TelemetryStatus>(enabled ? 'connecting' : 'ended')
  const [error, setError] = useState<string | null>(null)
  const attempts = useRef(0)

  useEffect(() => {
    if (!enabled) return

    let disposed = false
    let socket: WebSocket | null = null
    let reconnectTimer: number | null = null

    const connect = () => {
      if (disposed) return
      socket = new WebSocket(telemetrySocketUrl(serverId))

      socket.onopen = () => {
        attempts.current = 0
        setStatus('live')
        setError(null)
      }

      socket.onmessage = (event) => {
        let message: TelemetryMessage
        try {
          message = JSON.parse(event.data as string) as TelemetryMessage
        } catch {
          return
        }

        switch (message.type) {
          case 'ready':
            setStatus('live')
            break
          case 'stats':
            setSample(message.sample)
            break
          case 'log':
            setLogs((current) => {
              const next =
                current.length >= MAX_LOG_LINES
                  ? [...current.slice(current.length - MAX_LOG_LINES + 1), message.line]
                  : [...current, message.line]
              return next
            })
            break
          case 'stream_ended':
            if (message.stream === 'stats') setStatus('ended')
            break
          case 'error':
            setError(message.message)
            break
        }
      }

      socket.onerror = () => {
        setError('Telemetry connection failed.')
      }

      socket.onclose = () => {
        socket = null
        if (disposed) return

        setStatus('ended')
        const delay = Math.min(1000 * 2 ** attempts.current, MAX_RECONNECT_DELAY_MS)
        attempts.current += 1
        reconnectTimer = window.setTimeout(connect, delay)
      }
    }

    connect()

    return () => {
      disposed = true
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer)
      socket?.close()
    }
  }, [enabled, serverId])

  const clearLogs = useCallback(() => setLogs([]), [])

  return {
    sample,
    logs,
    status: enabled ? status : 'ended',
    error,
    clearLogs,
  }
}
