# AxolotlServers

A web-based manager for Minecraft servers. No terminal required: create servers, edit configs,
manage players and restore backups from a dark, glassmorphic dashboard.

> **Status: M1 complete.** Control-plane foundation (SQLite schema, JWT auth, admin bootstrap) is
> live; server management and the daemon link are next. Design:
> [`docs/architecture.md`](docs/architecture.md). Current state:
> [`docs/state.md`](docs/state.md). Plan: [`docs/todo.md`](docs/todo.md).

## Stack

| Layer          | Choice                                                |
| -------------- | ----------------------------------------------------- |
| Frontend       | React 19 + Vite 8 + Tailwind CSS 4 (TypeScript)       |
| Control plane  | Rust 2024 · axum · tokio · sqlx (SQLite)              |
| Node agent     | Rust · bollard (Docker Engine API)                    |
| Isolation      | One Docker container per game server, hard limits     |
| Persistence    | SQLite (control plane) + per-server bind mounts       |
| Realtime       | WebSockets (server → client streams); REST for commands |

## Layout

```
backend/               Rust workspace
  crates/api/          cluster-api   — REST + WS control plane
  crates/daemon/       cluster-daemon — Docker, files, RCON, backups
  crates/shared/       cluster-shared — wire types shared by both
frontend/              Vite React app
docs/                  architecture.md · state.md · todo.md  ← read these first
```

## Quick start

Requires Node 20+, Rust 1.85+ (stable), Docker.

```bash
cp .env.example .env

cd frontend && npm install && npm run dev   # http://localhost:5173

cd backend && cargo run -p cluster-api      # http://localhost:8080
cd backend && cargo run -p cluster-daemon   # connects to Docker, then exits (scaffold)
```

Or via `make help` for the short list of tasks.

## Working agreement

Before any non-trivial change, read `docs/state.md`. After every milestone, update all three docs
files so they describe reality, not intent.
