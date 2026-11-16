// Minimal typed-ish client for the Cluster Console control-plane API.

export class ClusterApiError extends Error {
  constructor(message, { status = 0, code = 'api_error', cause } = {}) {
    super(message, cause ? { cause } : undefined)
    this.name = 'ClusterApiError'
    this.status = status
    this.code = code
  }
}

/**
 * @param {{ apiUrl: string, apiToken?: string | null, username?: string | null,
 *           password?: string | null, timeoutMs?: number }} config
 */
export class ClusterApi {
  #token
  #loginPromise = null

  constructor(config) {
    this.apiUrl = config.apiUrl
    this.username = config.username ?? null
    this.password = config.password ?? null
    this.timeoutMs = config.timeoutMs ?? 30_000
    this.#token = config.apiToken ?? null
  }

  get authenticated() {
    return Boolean(this.#token)
  }

  async #ensureToken() {
    if (this.#token) return this.#token
    if (!this.username || !this.password) {
      throw new ClusterApiError(
        'No API token available. Set CLUSTER_API_TOKEN or CLUSTER_API_USERNAME/CLUSTER_API_PASSWORD.',
        { status: 401, code: 'unauthenticated' },
      )
    }
    this.#loginPromise ??= this.#login().finally(() => {
      this.#loginPromise = null
    })
    return this.#loginPromise
  }

  async #login() {
    const response = await this.#rawRequest('POST', '/api/auth/login', {
      username: this.username,
      password: this.password,
    })
    if (!response?.token) {
      throw new ClusterApiError('login response did not contain a token', {
        status: 500,
        code: 'invalid_login_response',
      })
    }
    this.#token = response.token
    return this.#token
  }

  async request(method, path, body) {
    const token = await this.#ensureToken()
    try {
      return await this.#rawRequest(method, path, body, token)
    } catch (error) {
      // A rotated JWT should trigger exactly one transparent re-login.
      if (
        error instanceof ClusterApiError &&
        error.status === 401 &&
        this.username &&
        this.password
      ) {
        this.#token = null
        const fresh = await this.#ensureToken()
        return this.#rawRequest(method, path, body, fresh)
      }
      throw error
    }
  }

  async #rawRequest(method, path, body, token = null) {
    const headers = { accept: 'application/json' }
    if (token) headers.authorization = `Bearer ${token}`
    if (body !== undefined) headers['content-type'] = 'application/json'

    let response
    try {
      response = await fetch(`${this.apiUrl}${path}`, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(this.timeoutMs),
      })
    } catch (error) {
      throw new ClusterApiError(
        `Could not reach the control plane at ${this.apiUrl}: ${error.message}`,
        { code: 'network_error', cause: error },
      )
    }

    if (response.status === 204) return null

    const text = await response.text()
    let parsed = null
    if (text) {
      try {
        parsed = JSON.parse(text)
      } catch {
        parsed = text
      }
    }

    if (!response.ok) {
      const message =
        (parsed && typeof parsed === 'object' && parsed.message) ||
        (typeof parsed === 'string' && parsed) ||
        `${response.status} ${response.statusText}`
      throw new ClusterApiError(message, {
        status: response.status,
        code: (parsed && typeof parsed === 'object' && parsed.code) || 'api_error',
      })
    }
    return parsed
  }

  listServers() {
    return this.request('GET', '/api/v1/servers')
  }

  getServer(id) {
    return this.request('GET', `/api/v1/servers/${encodeURIComponent(id)}`)
  }

  createServer(spec) {
    return this.request('POST', '/api/v1/servers', spec)
  }

  power(id, action) {
    return this.request('POST', `/api/v1/servers/${encodeURIComponent(id)}/${action}`)
  }

  deleteServer(id) {
    return this.request('DELETE', `/api/v1/servers/${encodeURIComponent(id)}`)
  }

  listBackups(id) {
    return this.request('GET', `/api/v1/servers/${encodeURIComponent(id)}/backups`)
  }

  createBackup(id, name) {
    return this.request('POST', `/api/v1/servers/${encodeURIComponent(id)}/backups`, {
      name: name || null,
    })
  }

  restoreBackup(id, backupId) {
    return this.request(
      'POST',
      `/api/v1/servers/${encodeURIComponent(id)}/backups/${encodeURIComponent(backupId)}/restore`,
    )
  }

  deleteBackup(id, backupId) {
    return this.request(
      'DELETE',
      `/api/v1/servers/${encodeURIComponent(id)}/backups/${encodeURIComponent(backupId)}`,
    )
  }

  verifyBackup(id, backupId) {
    return this.request(
      'POST',
      `/api/v1/servers/${encodeURIComponent(id)}/backups/${encodeURIComponent(backupId)}/verify`,
    )
  }

  getBackupSettings(id) {
    return this.request('GET', `/api/v1/servers/${encodeURIComponent(id)}/backups/settings`)
  }

  updateBackupSettings(id, retentionCount) {
    return this.request('PATCH', `/api/v1/servers/${encodeURIComponent(id)}/backups/settings`, {
      retention_count: retentionCount,
    })
  }

  listSchedules(id) {
    return this.request('GET', `/api/v1/servers/${encodeURIComponent(id)}/schedules`)
  }

  createSchedule(id, schedule) {
    return this.request('POST', `/api/v1/servers/${encodeURIComponent(id)}/schedules`, schedule)
  }

  updateSchedule(id, scheduleId, patch) {
    return this.request(
      'PATCH',
      `/api/v1/servers/${encodeURIComponent(id)}/schedules/${encodeURIComponent(scheduleId)}`,
      patch,
    )
  }

  deleteSchedule(id, scheduleId) {
    return this.request(
      'DELETE',
      `/api/v1/servers/${encodeURIComponent(id)}/schedules/${encodeURIComponent(scheduleId)}`,
    )
  }

  runSchedule(id, scheduleId) {
    return this.request(
      'POST',
      `/api/v1/servers/${encodeURIComponent(id)}/schedules/${encodeURIComponent(scheduleId)}/run`,
    )
  }

  listAddons(id) {
    return this.request('GET', `/api/v1/servers/${encodeURIComponent(id)}/addons`)
  }

  whoami() {
    return this.request('GET', '/api/v1/me')
  }
}
