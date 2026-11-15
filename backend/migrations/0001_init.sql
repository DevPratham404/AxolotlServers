-- Cluster Console — initial control-plane schema.
-- Version 1. Applied by `sqlx::migrate!` at startup.

CREATE TABLE users (
    id            TEXT PRIMARY KEY NOT NULL,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- A group of game servers reachable through one proxy (Velocity / BungeeCord).
-- The proxy itself is a row in `servers` with server_type velocity|bungeecord.
CREATE TABLE proxy_networks (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL UNIQUE,
    proxy_kind TEXT NOT NULL CHECK (proxy_kind IN ('velocity', 'bungeecord')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE servers (
    id               TEXT PRIMARY KEY NOT NULL,
    name             TEXT NOT NULL UNIQUE COLLATE NOCASE,
    game_port        INTEGER NOT NULL UNIQUE CHECK (game_port BETWEEN 1 AND 65535),
    server_type      TEXT NOT NULL
                     CHECK (server_type IN ('vanilla', 'paper', 'fabric', 'velocity', 'bungeecord')),
    ram_mb           INTEGER NOT NULL CHECK (ram_mb >= 512),
    cpu_limit        REAL NOT NULL CHECK (cpu_limit > 0.0),
    -- true => this backend is only reachable through its proxy network and must
    -- not publish a public game port.
    is_behind_proxy  INTEGER NOT NULL DEFAULT 0 CHECK (is_behind_proxy IN (0, 1)),
    proxy_network_id TEXT REFERENCES proxy_networks(id) ON DELETE SET NULL,
    status           TEXT NOT NULL DEFAULT 'stopped'
                     CHECK (status IN ('provisioning', 'stopped', 'starting', 'running', 'stopping', 'error')),
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_servers_proxy_network_id ON servers (proxy_network_id);
CREATE INDEX idx_servers_status ON servers (status);

-- Keep updated_at honest without relying on callers.
CREATE TRIGGER servers_touch_updated_at
AFTER UPDATE ON servers
FOR EACH ROW
WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE servers
       SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
     WHERE id = NEW.id;
END;
