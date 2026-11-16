// Configuration for the Cluster Console MCP server.

function trimTrailingSlashes(value) {
  return value.replace(/\/+$/, '')
}

function positiveInteger(value, fallback) {
  const parsed = Number.parseInt(value ?? '', 10)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback
}

/**
 * Read configuration from the environment.
 *
 * Authentication is optional at construction time: `CLUSTER_API_TOKEN` is used
 * when present, otherwise the server logs in lazily with
 * `CLUSTER_API_USERNAME` / `CLUSTER_API_PASSWORD`.
 */
export function loadConfig(env = process.env) {
  return {
    apiUrl: trimTrailingSlashes(env.CLUSTER_API_URL ?? 'http://127.0.0.1:8080'),
    apiToken: env.CLUSTER_API_TOKEN?.trim() || null,
    username: env.CLUSTER_API_USERNAME?.trim() || null,
    password: env.CLUSTER_API_PASSWORD || null,
    timeoutMs: positiveInteger(env.CLUSTER_MCP_TIMEOUT_MS, 30_000),
    repoRoot: env.CLUSTER_MCP_REPO_ROOT?.trim() || null,
  }
}

export function hasCredentials(config) {
  return Boolean(
    config.apiToken ||
      (config.username && config.password),
  )
}
