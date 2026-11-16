# MCP server — markdown context protocol

This directory contains a [Model Context Protocol](https://modelcontextprotocol.io) server
for Cluster Console. In this repository the protocol is used as a **markdown context
protocol**: every markdown document is exposed as an MCP resource, and the project's
server, backup and scheduling operations are exposed as typed tools.

```
mcp/
├── package.json
├── src/
│   ├── index.js   # MCP registration: resources, tools, prompts
│   ├── api.js     # control-plane REST client (JWT + lazy login)
│   ├── config.js  # environment configuration
│   └── docs.js    # markdown discovery and resource URIs
└── test/          # node:test unit tests
```

## Requirements

- Node.js 20 or newer
- A running `cluster-api` (default `http://127.0.0.1:8080`) and a user account or JWT

## Install and run

```bash
cd mcp
npm install
npm test          # unit tests, no server required
npm start         # speaks MCP over stdio
```

The server writes **nothing to stdout** except MCP frames; diagnostics go to stderr.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `CLUSTER_API_URL` | `http://127.0.0.1:8080` | Control-plane base URL (no `/api` suffix) |
| `CLUSTER_API_TOKEN` | — | Existing JWT. Preferred for automation. |
| `CLUSTER_API_USERNAME` | — | Used with the password to log in lazily when no token is set |
| `CLUSTER_API_PASSWORD` | — | Password for lazy login |
| `CLUSTER_MCP_TIMEOUT_MS` | `30000` | Per-request timeout |
| `CLUSTER_MCP_REPO_ROOT` | repository root | Override where markdown resources are discovered |

Generate a token by logging in:

```bash
curl -s http://127.0.0.1:8080/api/auth/login \
  -H 'content-type: application/json' \
  -d '{"username":"admin","password":"..."}' | jq -r .token
```

Rotated or expired JWTs are re-minted once automatically when a username and password are
configured.

## Client configuration

Claude Desktop / Cursor style (`mcpServers`):

```json
{
  "mcpServers": {
    "axolotlservers": {
      "command": "node",
      "args": ["/absolute/path/to/minecraftServerManager/mcp/src/index.js"],
      "env": {
        "CLUSTER_API_URL": "http://127.0.0.1:8080",
        "CLUSTER_API_TOKEN": "eyJ..."
      }
    }
  }
}
```

OpenCode (`opencode.json`):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "axolotlservers": {
      "type": "local",
      "command": ["node", "/absolute/path/to/minecraftServerManager/mcp/src/index.js"],
      "enabled": true,
      "environment": {
        "CLUSTER_API_URL": "http://127.0.0.1:8080",
        "CLUSTER_API_TOKEN": "eyJ..."
      }
    }
  }
}
```

## Resources

Every `*.md` file in the repository is registered as `cluster://docs/<path>` with MIME type
`text/markdown`. Implementation directories (`node_modules`, `target`, `dist`, `.git`,
`data`) are skipped. The core set is:

- `cluster://docs/README.md`
- `cluster://docs/docs/architecture.md`
- `cluster://docs/docs/state.md`
- `cluster://docs/docs/todo.md`
- `cluster://docs/docs/wisehosting_features_roadmap.md`
- `cluster://docs/deployment/README.md`
- `cluster://docs/mcp/README.md`

Resources are re-read on every request, so documentation edits are visible without
restarting the server. `list_documents` and `read_document` provide the same content for
clients that do not implement resources.

## Tools

| Tool | Kind | Notes |
| --- | --- | --- |
| `list_servers` | read | All servers with status |
| `get_server` | read | One server by UUID |
| `create_server` | write | Validates runtime, port, RAM/CPU and world fields |
| `server_power` | destructive | `start`, `stop`, `restart` |
| `list_backups` | read | ZIP snapshots, newest first |
| `create_backup` | write | Flushes a running server with RCON first |
| `restore_backup` | destructive | Creates a safety backup by default |
| `delete_backup` | destructive | Irreversible |
| `verify_backup` | read | Re-hashes the archive against its manifest |
| `get_backup_retention` | read | 0 means keep every snapshot |
| `set_backup_retention` | write | Keeps the N newest; prunes after the next backup |
| `list_schedules` | read | Includes last status and next run |
| `create_schedule` | write | `backup`, `start`, `stop`, `restart`, `command` |
| `update_schedule` | write | Rename, change interval, pause/resume |
| `run_schedule` | destructive | Runs immediately |
| `delete_schedule` | destructive | Irreversible |
| `describe_runtime` | read | Plugin/mod/modpack capabilities and Modrinth loaders |
| `list_documents`, `read_document` | read | Markdown context |

Prompts:
`server_triage` and `disaster_recovery` guide a client through read-first investigation
and a safe restore drill.

## Security

- The MCP server has the same authority as the user behind its token. Treat
  `CLUSTER_API_TOKEN` as a password and do not commit it.
- Prefer a dedicated account with a long random password for automation.
- Destructive tools are annotated (`destructiveHint`, `readOnlyHint`) so clients can prompt
  before running them. `restore_backup` creates a safety backup unless explicitly disabled.
- The server binds no network port; stdio keeps it local to the client process.

## Tests

```bash
cd mcp && npm test
```

The tests cover markdown discovery, URI construction, lazy login/token caching, a single
re-authentication after a 401, API error mapping, and network failures.
