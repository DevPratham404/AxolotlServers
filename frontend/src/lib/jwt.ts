import type { AuthUser } from './types'

interface JwtClaims {
  sub?: string
  username?: string
  requires_password_change?: boolean
  exp?: number
}

/**
 * Read the (already server-signed) claims for bootstrapping the UI before the
 * `/me` round-trip completes. This is display-only state — every privileged
 * action is still authorised by the Rust API.
 */
export function decodeUser(token: string): AuthUser | null {
  const claims = decodeClaims(token)
  if (!claims?.sub || !claims.username) return null
  if (typeof claims.exp === 'number' && claims.exp * 1000 <= Date.now()) return null

  return {
    id: claims.sub,
    username: claims.username,
    requires_password_change: claims.requires_password_change === true,
  }
}

function decodeClaims(token: string): JwtClaims | null {
  const payload = token.split('.')[1]
  if (!payload) return null

  try {
    const normalized = payload.replace(/-/g, '+').replace(/_/g, '/')
    const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=')
    const binary = window.atob(padded)
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0))
    const json = new TextDecoder().decode(bytes)
    return JSON.parse(json) as JwtClaims
  } catch {
    return null
  }
}
