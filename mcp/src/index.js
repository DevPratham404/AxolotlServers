#!/usr/bin/env node
// Cluster Console MCP server.
//
// Exposes two things over the Model Context Protocol:
//   1. Resources — every markdown document in the repository, so the client can
//      ground answers in the project's own documentation.
//   2. Tools — typed wrappers around the control-plane REST API for servers,
//      backups, schedules and the App Store catalog.
//
// Nothing is written to stdout except MCP frames; logs go to stderr.

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import { z } from 'zod'

import { ClusterApi, ClusterApiError } from './api.js'
import { hasCredentials, loadConfig } from './config.js'
import {
  listMarkdownDocuments,
  readMarkdownDocument,
  repoRoot,
  resourceUri,
} from './docs.js'

const SERVER_TYPES = [
  'vanilla',
  'paper',
  'fabric',
  'forge',
  'velocity',
  'bungeecord',
  'bedrock',
  'neoforge',
  'quilt',
  'spigot',
  'purpur',
  'pufferfish',
  'folia',
  'sponge',
  'mohist',
  'arclight',
  'catserver',
  'magma',
  'waterfall',
]

const TASK_TYPES = ['backup', 'start', 'stop', 'restart', 'command']

const config = loadConfig()
const api = new ClusterApi({
  apiUrl: config.apiUrl,
  apiToken: config.apiToken,
  username: config.username,
  password: config.password,
  timeoutMs: config.timeoutMs,
})

const server = new McpServer({
  name: 'axolotlservers-mcp',
  version: '0.1.0',
})

function textResult(value) {
  const text = typeof value === 'string' ? value : JSON.stringify(value, null, 2)
  return { content: [{ type: 'text', text }] }
}

function errorResult(error, hint = null) {
  const message =
    error instanceof ClusterApiError
      ? `${error.message}${error.status ? ` (HTTP ${error.status}, ${error.code})` : ''}`
      : (error?.message ?? String(error))
  const lines = [`Error: ${message}`]
  if (!hasCredentials(config)) {
    lines.push(
      'Set CLUSTER_API_TOKEN (or CLUSTER_API_USERNAME/CLUSTER_API_PASSWORD) before starting this MCP server.',
    )
  } else if (hint) {
    lines.push(hint)
  }
  return { content: [{ type: 'text', text: lines.join('\n') }], isError: true }
}

function tool(handler) {
  return async (args) => {
    try {
      return await handler(args)
    } catch (error) {
      console.error('[axolotlservers-mcp] tool failed:', error)
      return errorResult(error)
    }
  }
}

// ---------------------------------------------------------------------------
// Documentation resources
// ---------------------------------------------------------------------------

const documents = await listMarkdownDocuments(repoRoot(config.repoRoot))
const documentsByPath = new Map(documents.map((document) => [document.path, document]))

for (const document of documents) {
  server.registerResource(
    `doc:${document.path}`,
    resourceUri(document.path),
    {
      title: document.title,
      description: `Repository documentation: ${document.path}`,
      mimeType: 'text/markdown',
    },
    async (uri) => ({
      contents: [
        {
          uri: uri.href,
          mimeType: 'text/markdown',
          text: readMarkdownDocument(document),
        },
      ],
    }),
  )
}

server.registerTool(
  'list_documents',
  {
    title: 'List project documents',
    description:
      'List the markdown documents exposed as MCP resources. Use this to find the right design, state or operations guide before acting.',
    inputSchema: {},
    annotations: { readOnlyHint: true },
  },
  tool(async () =>
    textResult(
      documents.map((document) => ({
        path: document.path,
        title: document.title,
        uri: resourceUri(document.path),
      })),
    ),
  ),
)

server.registerTool(
  'read_document',
  {
    title: 'Read a project document',
    description:
      'Read a markdown document by repository-relative path (for example docs/architecture.md). Prefer MCP resources when the client supports them.',
    inputSchema: {
      path: z
        .string()
        .min(1)
        .describe('Repository-relative path, e.g. docs/state.md or README.md'),
    },
    annotations: { readOnlyHint: true },
  },
  tool(async ({ path }) => {
    const document = documentsByPath.get(path)
    if (!document) {
      return {
        content: [
          {
            type: 'text',
            text: `Unknown document: ${path}\nAvailable documents:\n${documents
              .map((item) => `- ${item.path}`)
              .join('\n')}`,
          },
        ],
        isError: true,
      }
    }
    return textResult(readMarkdownDocument(document))
  }),
)

// ---------------------------------------------------------------------------
// Server tools
// ---------------------------------------------------------------------------

server.registerTool(
  'list_servers',
  {
    title: 'List Minecraft servers',
    description: 'List every server known to the control plane, including status and resources.',
    inputSchema: {},
    annotations: { readOnlyHint: true },
  },
  tool(async () => textResult(await api.listServers())),
)

server.registerTool(
  'get_server',
  {
    title: 'Get a server',
    description: 'Fetch one server by UUID, including runtime, world settings and port.',
    inputSchema: { id: z.string().uuid().describe('Server UUID') },
    annotations: { readOnlyHint: true },
  },
  tool(async ({ id }) => textResult(await api.getServer(id))),
)

server.registerTool(
  'create_server',
  {
    title: 'Create a server',
    description:
      'Create a Minecraft server container. The server is provisioned stopped; start it with server_power. RAM is in MiB and CPU is a fraction of one host core.',
    inputSchema: {
      name: z.string().min(1).max(64),
      server_type: z.enum(SERVER_TYPES).describe('Runtime to provision'),
      game_port: z.number().int().min(1024).max(65535),
      ram_mb: z.number().int().min(512),
      cpu_limit: z.number().positive().max(64),
      game_version: z.string().default('LATEST'),
      world_name: z.string().min(1).max(128).default('world'),
      world_seed: z.string().max(128).default(''),
      world_type: z
        .enum(['default', 'flat', 'largeBiomes', 'amplified', 'normal', 'large_biomes'])
        .default('default'),
      difficulty: z.enum(['peaceful', 'easy', 'normal', 'hard']).default('normal'),
      gamemode: z.enum(['survival', 'creative', 'adventure', 'spectator']).default('survival'),
    },
    annotations: { readOnlyHint: false, destructiveHint: false },
  },
  tool(async (spec) => {
    const created = await api.createServer({ ...spec, is_behind_proxy: false })
    return textResult(created)
  }),
)

server.registerTool(
  'server_power',
  {
    title: 'Start, stop or restart a server',
    description: 'Send a lifecycle action to a server container.',
    inputSchema: {
      id: z.string().uuid(),
      action: z.enum(['start', 'stop', 'restart']),
    },
    annotations: { readOnlyHint: false, destructiveHint: true },
  },
  tool(async ({ id, action }) => textResult(await api.power(id, action))),
)

// ---------------------------------------------------------------------------
// Backup tools
// ---------------------------------------------------------------------------

server.registerTool(
  'list_backups',
  {
    title: 'List backups',
    description: 'List ZIP snapshots for a server, newest first, with size and SHA-256.',
    inputSchema: { id: z.string().uuid() },
    annotations: { readOnlyHint: true },
  },
  tool(async ({ id }) => textResult(await api.listBackups(id))),
)

server.registerTool(
  'create_backup',
  {
    title: 'Create a backup',
    description:
      'Snapshot the whole data volume to a ZIP on the daemon host. Running servers are flushed with RCON first.',
    inputSchema: {
      id: z.string().uuid(),
      name: z.string().min(1).max(120).optional().describe('Optional human-readable label'),
    },
    annotations: { readOnlyHint: false, destructiveHint: false },
  },
  tool(async ({ id, name }) => textResult(await api.createBackup(id, name))),
)

server.registerTool(
  'restore_backup',
  {
    title: 'Restore a backup',
    description:
      'Replace the live data volume with a verified backup. By default a safety backup is created first; the container is stopped during the restore and restarted if it was running.',
    inputSchema: {
      id: z.string().uuid(),
      backup_id: z.string().uuid(),
      create_safety_backup: z
        .boolean()
        .default(true)
        .describe('Take a fresh backup before restoring (recommended)'),
    },
    annotations: { readOnlyHint: false, destructiveHint: true },
  },
  tool(async ({ id, backup_id, create_safety_backup }) => {
    if (create_safety_backup) {
      await api.createBackup(id, `Before restore ${new Date().toISOString()}`)
    }
    return textResult(await api.restoreBackup(id, backup_id))
  }),
)

server.registerTool(
  'delete_backup',
  {
    title: 'Delete a backup',
    description: 'Delete a backup archive and its manifest. This cannot be undone.',
    inputSchema: {
      id: z.string().uuid(),
      backup_id: z.string().uuid(),
    },
    annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: true },
  },
  tool(async ({ id, backup_id }) => {
    await api.deleteBackup(id, backup_id)
    return textResult(`Deleted backup ${backup_id}.`)
  }),
)

server.registerTool(
  'verify_backup',
  {
    title: 'Verify a backup',
    description:
      'Re-hash a stored archive and compare it with its SHA-256 manifest. Reports whether the archive is still restorable.',
    inputSchema: {
      id: z.string().uuid(),
      backup_id: z.string().uuid(),
    },
    annotations: { readOnlyHint: true },
  },
  tool(async ({ id, backup_id }) => textResult(await api.verifyBackup(id, backup_id))),
)

server.registerTool(
  'get_backup_retention',
  {
    title: 'Get backup retention',
    description:
      'Read how many of the newest snapshots are kept for a server. Zero means every backup is retained.',
    inputSchema: { id: z.string().uuid() },
    annotations: { readOnlyHint: true },
  },
  tool(async ({ id }) => textResult(await api.getBackupSettings(id))),
)

server.registerTool(
  'set_backup_retention',
  {
    title: 'Set backup retention',
    description:
      'Keep only the N newest backups (0–500; 0 keeps all). Pruning happens after the next successful backup, so older archives are not deleted immediately.',
    inputSchema: {
      id: z.string().uuid(),
      retention_count: z.number().int().min(0).max(500),
    },
    annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
  },
  tool(async ({ id, retention_count }) =>
    textResult(await api.updateBackupSettings(id, retention_count)),
  ),
)

// ---------------------------------------------------------------------------
// Schedule tools
// ---------------------------------------------------------------------------

server.registerTool(
  'list_schedules',
  {
    title: 'List scheduled tasks',
    description:
      'List recurring backup, power or RCON tasks for a server, including the last result and next run time.',
    inputSchema: { id: z.string().uuid() },
    annotations: { readOnlyHint: true },
  },
  tool(async ({ id }) => textResult(await api.listSchedules(id))),
)

server.registerTool(
  'create_schedule',
  {
    title: 'Create a scheduled task',
    description:
      'Schedule a recurring task. The worker checks every 20 seconds. Command tasks require an RCON command.',
    inputSchema: {
      id: z.string().uuid(),
      name: z.string().min(1).max(80),
      task_type: z.enum(TASK_TYPES),
      interval_minutes: z.number().int().min(1).max(525_600),
      command: z.string().min(1).max(1024).optional(),
      enabled: z.boolean().default(true),
      run_now: z.boolean().default(false).describe('Run once immediately as well'),
    },
    annotations: { readOnlyHint: false, destructiveHint: false },
  },
  tool(async ({ id, ...schedule }) => textResult(await api.createSchedule(id, schedule))),
)

server.registerTool(
  'update_schedule',
  {
    title: 'Update or pause a scheduled task',
    description: 'Change a schedule name, interval, enabled flag, or command.',
    inputSchema: {
      id: z.string().uuid(),
      schedule_id: z.string().uuid(),
      name: z.string().min(1).max(80).optional(),
      interval_minutes: z.number().int().min(1).max(525_600).optional(),
      command: z.string().min(1).max(1024).optional(),
      enabled: z.boolean().optional(),
    },
    annotations: { readOnlyHint: false, destructiveHint: false },
  },
  tool(async ({ id, schedule_id, ...patch }) =>
    textResult(await api.updateSchedule(id, schedule_id, patch)),
  ),
)

server.registerTool(
  'run_schedule',
  {
    title: 'Run a scheduled task now',
    description: 'Execute a schedule immediately and return its recorded result.',
    inputSchema: {
      id: z.string().uuid(),
      schedule_id: z.string().uuid(),
    },
    annotations: { readOnlyHint: false, destructiveHint: true },
  },
  tool(async ({ id, schedule_id }) => textResult(await api.runSchedule(id, schedule_id))),
)

server.registerTool(
  'delete_schedule',
  {
    title: 'Delete a scheduled task',
    description: 'Delete a recurring task. This cannot be undone.',
    inputSchema: {
      id: z.string().uuid(),
      schedule_id: z.string().uuid(),
    },
    annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: true },
  },
  tool(async ({ id, schedule_id }) => {
    await api.deleteSchedule(id, schedule_id)
    return textResult(`Deleted schedule ${schedule_id}.`)
  }),
)

// ---------------------------------------------------------------------------
// App Store helpers
// ---------------------------------------------------------------------------

server.registerTool(
  'describe_runtime',
  {
    title: 'Describe runtime capabilities',
    description:
      'Explain which add-on types a runtime supports (plugins, mods, modpacks) and which Modrinth loaders the App Store searches. Useful before installing add-ons.',
    inputSchema: { server_type: z.enum(SERVER_TYPES) },
    annotations: { readOnlyHint: true },
  },
  tool(async ({ server_type }) => {
    const capabilities = runtimeCapabilities(server_type)
    return textResult({ server_type, ...capabilities })
  }),
)

function runtimeCapabilities(serverType) {
  const modded = new Set(['fabric', 'forge', 'neoforge', 'quilt'])
  const hybrids = new Set(['mohist', 'arclight', 'catserver', 'magma'])
  const plugins = new Set([
    'paper',
    'spigot',
    'purpur',
    'pufferfish',
    'folia',
    'sponge',
    'velocity',
    'bungeecord',
    'waterfall',
  ])
  const supportsMods = modded.has(serverType) || hybrids.has(serverType)
  const supportsPlugins = plugins.has(serverType) || hybrids.has(serverType)
  let loaders = []
  if (serverType === 'fabric') loaders = ['fabric']
  else if (serverType === 'forge') loaders = ['forge']
  else if (serverType === 'neoforge') loaders = ['neoforge']
  else if (serverType === 'quilt') loaders = ['quilt']
  else if (hybrids.has(serverType)) loaders = ['forge', 'neoforge', 'paper', 'spigot', 'bukkit']
  else if (serverType === 'paper') loaders = ['paper', 'spigot', 'bukkit', 'purpur', 'folia']
  else if (serverType === 'spigot') loaders = ['spigot', 'bukkit']
  else if (serverType === 'purpur') loaders = ['purpur', 'paper', 'spigot', 'bukkit']
  else if (serverType === 'pufferfish' || serverType === 'folia') {
    loaders = [serverType, 'paper', 'spigot', 'bukkit']
  } else if (serverType === 'sponge') loaders = ['sponge']
  else if (serverType === 'velocity') loaders = ['velocity']
  else if (serverType === 'bungeecord' || serverType === 'waterfall') loaders = ['bungeecord']
  return {
    supports_plugins: supportsPlugins,
    supports_mods: supportsMods,
    supports_modpacks: supportsMods,
    modrinth_loaders: loaders,
  }
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

server.registerPrompt(
  'server_triage',
  {
    title: 'Triage a server',
    description:
      'Investigate one server using its docs, status, backups, schedules and installed add-ons.',
    argsSchema: {
      server_id: z.string().describe('Server UUID to investigate'),
      symptom: z.string().optional().describe('What the operator observed'),
    },
  },
  ({ server_id, symptom }) => ({
    messages: [
      {
        role: 'user',
        content: {
          type: 'text',
          text: [
            `Triage server ${server_id}${symptom ? `: ${symptom}` : '.'}`,
            '',
            'Steps:',
            '1. Read the `docs/architecture.md` and `docs/state.md` resources for context.',
            '2. Call get_server, list_backups and list_schedules for the server.',
            '3. If it is running, inspect the console/logs and installed add-ons via the web UI;',
            '   the MCP server intentionally exposes read-only add-on listing only.',
            '4. Report the likely cause, the evidence, and a safe next action.',
            '5. Never restore a backup or delete anything without explicit operator confirmation.',
          ].join('\n'),
        },
      },
    ],
  }),
)

server.registerPrompt(
  'disaster_recovery',
  {
    title: 'Plan a backup and restore drill',
    description: 'Use the project docs to propose a backup schedule and a restore drill.',
    argsSchema: {
      server_id: z.string().describe('Server UUID'),
    },
  },
  ({ server_id }) => ({
    messages: [
      {
        role: 'user',
        content: {
          type: 'text',
          text: [
            `Draft a backup and recovery plan for server ${server_id}.`,
            'Read the MCP resources `docs/architecture.md` and `docs/state.md` first.',
            'Use list_backups and list_schedules to see the current state.',
            'Recommend a schedule using create_schedule only after explaining the trade-offs.',
            'Describe the exact restore drill, including creating a safety backup.',
          ].join('\n'),
        },
      },
    ],
  }),
)

// ---------------------------------------------------------------------------

console.error(
  `[axolotlservers-mcp] ${documents.length} markdown resources, API ${config.apiUrl}, auth ${
    hasCredentials(config) ? 'configured' : 'missing'
  }`,
)

const transport = new StdioServerTransport()
await server.connect(transport)
