#!/usr/bin/env bash
#
# start-dev.sh — boot the full Cluster Console dev stack.
#
#   cluster-daemon  :9090   (Docker lifecycle, RCON, file I/O)
#   cluster-api     :8080   (REST + WebSocket control plane)
#   Vite dev server :5173   (React SPA, proxies /api and /ws to :8080)
#
# Press Ctrl+C to stop all three processes. Logs are also written to
# data/dev-logs/{daemon,api,vite}.log.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

DAEMON_PID=""
API_PID=""
VITE_PID=""
TAIL_PIDS=""

kill_tree() {
  local pid="$1"
  local child
  for child in $(pgrep -P "$pid" 2>/dev/null || true); do
    kill_tree "$child"
  done
  kill "$pid" 2>/dev/null || true
}

shutdown() {
  trap - EXIT INT TERM
  echo
  echo "Stopping Cluster Console dev stack..."
  for pid in $TAIL_PIDS; do kill "$pid" 2>/dev/null || true; done
  for pid in $VITE_PID $API_PID $DAEMON_PID; do
    [ -n "$pid" ] && kill_tree "$pid"
  done
  wait 2>/dev/null || true
  echo "Stopped."
  exit 0
}

trap shutdown EXIT INT TERM

fail() {
  echo "error: $*" >&2
  exit 1
}

# --- Prerequisites ---------------------------------------------------------

command -v cargo >/dev/null 2>&1 || fail "cargo is not installed (need Rust 1.85+)."
command -v npm >/dev/null 2>&1 || fail "npm is not installed (need Node 20+)."
command -v docker >/dev/null 2>&1 || fail "the docker CLI is not installed."

echo "Checking Docker Engine..."
if ! docker info >/dev/null 2>&1; then
  fail "Docker is not reachable. Start Docker Desktop (or the Docker daemon), then re-run this script."
fi
echo "Docker is running."

for port in 8080 9090 5173; do
  if command -v lsof >/dev/null 2>&1 && lsof -ti "tcp:$port" >/dev/null 2>&1; then
    fail "port $port is already in use. Stop the old process (or the previous dev stack) and retry."
  fi
done

# --- Local configuration ---------------------------------------------------

if [ ! -f .env ]; then
  echo "No .env found — creating one from .env.example with development secrets."
  cp .env.example .env

  secret="$(openssl rand -hex 32)"
  jwt_secret="$(openssl rand -base64 48 | tr -d '\n')"
  admin_password="${DEV_ADMIN_PASSWORD:-cluster-dev-admin}"

  JWT_SECRET_VALUE="$jwt_secret" \
  DAEMON_SECRET_VALUE="$secret" \
  ADMIN_PASSWORD_VALUE="$admin_password" \
  python3 - <<'PY'
import os
import re

path = ".env"
text = open(path, encoding="utf-8").read()
replacements = {
    "JWT_SECRET": os.environ["JWT_SECRET_VALUE"],
    "DAEMON_SHARED_SECRET": os.environ["DAEMON_SECRET_VALUE"],
    "ADMIN_PASSWORD": os.environ["ADMIN_PASSWORD_VALUE"],
}
for key, value in replacements.items():
    text = re.sub(rf"^{key}=.*$", f"{key}={value}", text, flags=re.MULTILINE)
open(path, "w", encoding="utf-8").write(text)
PY
fi

set -a
# shellcheck disable=SC1091
. ./.env
set +a

[ -n "${DAEMON_SHARED_SECRET:-}" ] || fail "DAEMON_SHARED_SECRET is empty in .env."
[ -n "${JWT_SECRET:-}" ] || fail "JWT_SECRET is empty in .env."

if [ "$DAEMON_SHARED_SECRET" = "change-me" ]; then
  echo "warning: DAEMON_SHARED_SECRET is still 'change-me'; fine for local testing only."
fi

# --- Build / install -------------------------------------------------------

echo "Building Rust binaries (incremental)..."
(cd backend && cargo build -p cluster-daemon -p cluster-api)

if [ ! -d frontend/node_modules ]; then
  echo "Installing frontend dependencies..."
  (cd frontend && npm install)
fi

# --- Launch ----------------------------------------------------------------

mkdir -p data/dev-logs
: > data/dev-logs/daemon.log
: > data/dev-logs/api.log
: > data/dev-logs/vite.log

echo "Starting cluster-daemon..."
(cd backend && exec ./target/debug/cluster-daemon) >data/dev-logs/daemon.log 2>&1 &
DAEMON_PID=$!

echo "Starting cluster-api..."
(cd backend && exec ./target/debug/cluster-api) >data/dev-logs/api.log 2>&1 &
API_PID=$!

echo "Starting Vite..."
(cd frontend && exec npm run dev -- --host 127.0.0.1) >data/dev-logs/vite.log 2>&1 &
VITE_PID=$!

prefix_stream() {
  local name="$1" file="$2"
  tail -n +1 -F "$file" 2>/dev/null |
    awk -v tag="[$name]" '{ print tag " " $0; fflush() }' &
  TAIL_PIDS="$TAIL_PIDS $!"
}

prefix_stream daemon data/dev-logs/daemon.log
prefix_stream api data/dev-logs/api.log
prefix_stream vite data/dev-logs/vite.log

echo
echo "Waiting for the control plane..."
for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:8080/healthz" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$API_PID" 2>/dev/null; then
    echo
    echo "cluster-api exited during startup. Last log lines:"
    tail -n 40 data/dev-logs/api.log
    exit 1
  fi
  sleep 0.5
done

if ! curl -fsS "http://127.0.0.1:8080/healthz" >/dev/null 2>&1; then
  echo "warning: API did not answer /healthz yet; check data/dev-logs/api.log."
fi

cat <<EOF

  ┌──────────────────────────────────────────────────────────────┐
  │  Cluster Console dev stack is running                        │
  └──────────────────────────────────────────────────────────────┘

  Web UI          http://localhost:5173
  API             http://localhost:8080
  Daemon          http://127.0.0.1:9090  (HMAC only)
  Logs            data/dev-logs/
EOF

if [ -n "${ADMIN_PASSWORD:-}" ]; then
  cat <<EOF
  Login           ${ADMIN_USERNAME:-admin} / ${ADMIN_PASSWORD}
                  (seeded only while the users table is empty; a password
                  change is forced on first login)

EOF
else
  cat <<EOF
  Login           use the admin account already in the database. A fresh
                  database with an empty ADMIN_PASSWORD logs a generated
                  password once in data/dev-logs/api.log.

EOF
fi

cat <<EOF
  Press Ctrl+C to stop all three processes.

EOF

# Keep the supervisor alive; if a child dies, surface the relevant log tail.
while :; do
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo
    echo "cluster-daemon exited unexpectedly. Last log lines:"
    tail -n 40 data/dev-logs/daemon.log
    exit 1
  fi
  if ! kill -0 "$API_PID" 2>/dev/null; then
    echo
    echo "cluster-api exited unexpectedly. Last log lines:"
    tail -n 40 data/dev-logs/api.log
    exit 1
  fi
  if ! kill -0 "$VITE_PID" 2>/dev/null; then
    echo
    echo "Vite exited unexpectedly. Last log lines:"
    tail -n 40 data/dev-logs/vite.log
    exit 1
  fi
  sleep 2
done
