-- Backup retention policy.
--
-- The daemon owns archive files; this table stores how many of the newest
-- snapshots the control plane should keep for a server. `retention_count = 0`
-- means "keep every backup" (the safe default). Pruning runs after each
-- successful manual or scheduled backup, never on a timer, so an operator can
-- always see the newest snapshot before older ones rotate out.

CREATE TABLE backup_settings (
    server_id       TEXT PRIMARY KEY NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    retention_count INTEGER NOT NULL DEFAULT 0
                    CHECK (retention_count BETWEEN 0 AND 500),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
