-- Backups catalog and scheduled automation.
--
-- The daemon owns the archive files; this table is the control-plane catalog so
-- the UI can list backups without a daemon round-trip. Scheduled tasks are
-- executed by a background worker in cluster-api.

CREATE TABLE backups (
    id                TEXT PRIMARY KEY NOT NULL,
    server_id         TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    size_bytes        INTEGER NOT NULL,
    sha256            TEXT NOT NULL,
    server_was_running INTEGER NOT NULL DEFAULT 0 CHECK (server_was_running IN (0,1)),
    created_at_ms     INTEGER NOT NULL,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX idx_backups_server_created ON backups(server_id, created_at_ms DESC);

CREATE TABLE scheduled_tasks (
    id               TEXT PRIMARY KEY NOT NULL,
    server_id        TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    task_type        TEXT NOT NULL CHECK (task_type IN ('backup','start','stop','restart','command')),
    command          TEXT,
    interval_minutes INTEGER NOT NULL CHECK (interval_minutes >= 1 AND interval_minutes <= 525600),
    enabled          INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    next_run_at      TEXT NOT NULL,
    last_run_at      TEXT,
    last_status      TEXT CHECK (last_status IS NULL OR last_status IN ('success','error')),
    last_error       TEXT,
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX idx_scheduled_tasks_due ON scheduled_tasks(enabled, next_run_at);
CREATE INDEX idx_scheduled_tasks_server ON scheduled_tasks(server_id);
