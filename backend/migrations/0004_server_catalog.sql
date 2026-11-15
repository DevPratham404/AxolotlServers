-- Expand the supported runtime catalog and add first-boot world settings.
CREATE TABLE servers_new (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    game_port INTEGER NOT NULL UNIQUE CHECK (game_port BETWEEN 1 AND 65535),
    server_type TEXT NOT NULL CHECK (server_type IN ('vanilla','paper','fabric','forge','velocity','bungeecord','bedrock','neoforge','quilt','spigot','purpur','pufferfish','folia','sponge','mohist','arclight','catserver','magma','waterfall')),
    game_version TEXT NOT NULL DEFAULT 'LATEST',
    ram_mb INTEGER NOT NULL CHECK (ram_mb >= 512),
    cpu_limit REAL NOT NULL CHECK (cpu_limit > 0.0),
    is_behind_proxy INTEGER NOT NULL DEFAULT 0 CHECK (is_behind_proxy IN (0,1)),
    proxy_network_id TEXT REFERENCES proxy_networks(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'stopped' CHECK (status IN ('provisioning','stopped','starting','running','stopping','error')),
    world_name TEXT NOT NULL DEFAULT 'world',
    world_seed TEXT NOT NULL DEFAULT '',
    world_type TEXT NOT NULL DEFAULT 'default',
    difficulty TEXT NOT NULL DEFAULT 'normal',
    gamemode TEXT NOT NULL DEFAULT 'survival',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO servers_new (id,name,game_port,server_type,game_version,ram_mb,cpu_limit,is_behind_proxy,proxy_network_id,status,created_at,updated_at)
SELECT id,name,game_port,server_type,game_version,ram_mb,cpu_limit,is_behind_proxy,proxy_network_id,status,created_at,updated_at FROM servers;
DROP TABLE servers;
ALTER TABLE servers_new RENAME TO servers;
CREATE INDEX idx_servers_proxy_network_id ON servers(proxy_network_id);
CREATE INDEX idx_servers_status ON servers(status);
CREATE TRIGGER servers_touch_updated_at AFTER UPDATE ON servers FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at BEGIN
 UPDATE servers SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id;
END;
