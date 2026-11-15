import { useCallback, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'

import { AuthContext, type AuthStatus } from './auth-context'
import { authApi, ApiError, getToken, setToken } from '../lib/api'
import { decodeUser } from '../lib/jwt'
import type { AuthUser } from '../lib/types'

interface AuthState {
  status: AuthStatus
  user: AuthUser | null
}

/**
 * Owns the user session for the whole SPA.
 *
 * - The JWT is persisted in `localStorage` and mirrored in the API module so
 *   every fetch is authenticated centrally.
 * - The token's claims boot the UI immediately; `GET /api/v1/me` then confirms
 *   the session (and is the only route the backend allows while a password
 *   change is pending).
 * - A 401 or `password_change_required` response anywhere in the app emits a
 *   DOM event that this provider turns into the matching auth screen.
 */
export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>(initialState)

  useEffect(() => {
    const token = getToken()
    if (!token) return

    const decoded = decodeUser(token)
    if (!decoded) {
      setToken(null)
      return
    }

    let cancelled = false

    authApi
      .me()
      .then((user) => {
        if (!cancelled) setState({ status: 'authenticated', user })
      })
      .catch((error: unknown) => {
        if (cancelled) return
        if (error instanceof ApiError && error.isPasswordChangeRequired) {
          setState({
            status: 'authenticated',
            user: { ...decoded, requires_password_change: true },
          })
          return
        }
        if (error instanceof ApiError && error.status === 401) {
          setToken(null)
          setState({ status: 'anonymous', user: null })
          return
        }
        // Control plane unreachable: keep the decoded identity so the user can
        // retry from the UI, but show the pending screen if the claim says so.
        setState({ status: 'authenticated', user: decoded })
      })

    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    const onUnauthorized = () => {
      setToken(null)
      setState({ status: 'anonymous', user: null })
    }

    const onPasswordChangeRequired = () => {
      setState((current) =>
        current.user
          ? { status: 'authenticated', user: { ...current.user, requires_password_change: true } }
          : current,
      )
    }

    window.addEventListener('cluster:unauthorized', onUnauthorized)
    window.addEventListener('cluster:password-change-required', onPasswordChangeRequired)
    return () => {
      window.removeEventListener('cluster:unauthorized', onUnauthorized)
      window.removeEventListener('cluster:password-change-required', onPasswordChangeRequired)
    }
  }, [])

  const login = useCallback(async (username: string, password: string) => {
    const response = await authApi.login(username, password)
    setToken(response.token)
    setState({ status: 'authenticated', user: response.user })
  }, [])

  const changePassword = useCallback(async (currentPassword: string, newPassword: string) => {
    const response = await authApi.changePassword(currentPassword, newPassword)
    setToken(response.token)
    setState({ status: 'authenticated', user: response.user })
  }, [])

  const logout = useCallback(() => {
    setToken(null)
    setState({ status: 'anonymous', user: null })
  }, [])

  const value = useMemo(
    () => ({ status: state.status, user: state.user, login, changePassword, logout }),
    [state, login, changePassword, logout],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

/** Synchronous first paint: trust the signed claims only to choose the screen. */
function initialState(): AuthState {
  const token = getToken()
  if (!token) return { status: 'anonymous', user: null }
  const decoded = decodeUser(token)
  if (!decoded) return { status: 'anonymous', user: null }
  return { status: 'booting', user: decoded }
}
