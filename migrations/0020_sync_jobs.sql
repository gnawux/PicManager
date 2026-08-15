-- Durable synchronization work. Provider checkpoints are advanced only after every
-- discovered source item and sync item has been committed.

CREATE TABLE sync_jobs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    kind              TEXT NOT NULL CHECK (LENGTH(kind) > 0),
    provider          TEXT CHECK (provider IS NULL OR LENGTH(provider) > 0),
    status            TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'paused', 'completed', 'failed', 'cancelled')),
    checkpoint_before BLOB,
    checkpoint_after  BLOB,
    total_items       INTEGER NOT NULL DEFAULT 0 CHECK (total_items >= 0),
    completed_items   INTEGER NOT NULL DEFAULT 0 CHECK (completed_items >= 0),
    failed_items      INTEGER NOT NULL DEFAULT 0 CHECK (failed_items >= 0),
    error             TEXT,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    started_at        TEXT,
    finished_at       TEXT,
    updated_at        TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (completed_items + failed_items <= total_items)
);

CREATE INDEX idx_sync_jobs_status_created ON sync_jobs(status, created_at);
CREATE INDEX idx_sync_jobs_provider_created ON sync_jobs(provider, created_at DESC);

CREATE TABLE sync_items (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id           INTEGER NOT NULL REFERENCES sync_jobs(id) ON DELETE CASCADE,
    source_id        INTEGER REFERENCES asset_sources(id) ON DELETE SET NULL,
    external_id      TEXT NOT NULL CHECK (LENGTH(external_id) > 0),
    operation        TEXT NOT NULL CHECK (LENGTH(operation) > 0),
    status           TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'leased', 'succeeded', 'failed', 'excluded', 'cancelled')),
    attempt_count    INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    max_attempts     INTEGER NOT NULL DEFAULT 5 CHECK (max_attempts > 0),
    available_at     TEXT NOT NULL DEFAULT (datetime('now')),
    lease_owner      TEXT,
    lease_expires_at TEXT,
    payload_json     TEXT,
    last_error       TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    started_at       TEXT,
    finished_at      TEXT,
    updated_at       TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(job_id, external_id, operation),
    CHECK (
        status != 'leased' OR
        (lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL)
    )
);

CREATE INDEX idx_sync_items_claim
    ON sync_items(status, available_at, lease_expires_at, id);
CREATE INDEX idx_sync_items_job_status ON sync_items(job_id, status);
CREATE INDEX idx_sync_items_source ON sync_items(source_id);

CREATE TABLE provider_checkpoints (
    provider            TEXT NOT NULL CHECK (LENGTH(provider) > 0),
    scope_key           TEXT NOT NULL DEFAULT 'default' CHECK (LENGTH(scope_key) > 0),
    token               BLOB,
    generation          INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    last_full_reconcile TEXT,
    updated_at          TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (provider, scope_key)
);
