import { useCallback, useEffect, useRef, useState } from 'react'

import { ApiError, serverApi } from '../../lib/api'
import type { ServerView } from '../../lib/types'

export interface ServersState {
  servers: ServerView[]
  loading: boolean
  error: string | null
  reload: () => Promise<void>
}

/** Fetch the server list; every mutation calls `reload` afterwards. */
export function useServers(): ServersState {
  const [servers, setServers] = useState<ServerView[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const requestId = useRef(0)

  const reload = useCallback(async () => {
    const id = ++requestId.current
    setLoading(true)
    setError(null)
    try {
      const next = await serverApi.list()
      if (id === requestId.current) setServers(next)
    } catch (caught) {
      if (id !== requestId.current) return
      setError(caught instanceof ApiError ? caught.message : 'Could not load servers.')
    } finally {
      if (id === requestId.current) setLoading(false)
    }
  }, [])

  useEffect(() => {
    // Initial fetch; `loading` already starts true, and reload owns the state.
    // oxlint-disable-next-line react/set-state-in-effect
    void reload()
    return () => {
      requestId.current += 1
    }
  }, [reload])

  return { servers, loading, error, reload }
}
