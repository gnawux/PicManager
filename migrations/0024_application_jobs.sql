-- Generic durable application jobs. Provider-specific sync_jobs remain intact during
-- the compatibility period and are migrated by services in later milestones.

CREATE TABLE application_jobs (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    kind                TEXT NOT NULL CHECK (LENGTH(kind) > 0),
    payload_version     INTEGER NOT NULL DEFAULT 1 CHECK (payload_version > 0),
    payload_json        TEXT NOT NULL DEFAULT '{}'
        CHECK (json_valid(payload_json)),
    status              TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN (
            'queued', 'running', 'retry_wait', 'succeeded', 'failed', 'cancelled'
        )),
    priority            INTEGER NOT NULL DEFAULT 0,
    progress_total      INTEGER CHECK (progress_total IS NULL OR progress_total >= 0),
    progress_completed  INTEGER NOT NULL DEFAULT 0 CHECK (progress_completed >= 0),
    progress_stage      TEXT,
    cancel_requested_at TEXT,
    max_attempts        INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
    attempt_count       INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_run_at         TEXT,
    lease_owner         TEXT,
    lease_expires_at    TEXT,
    error_code          TEXT,
    error_message       TEXT,
    error_details_json  TEXT CHECK (
        error_details_json IS NULL OR json_valid(error_details_json)
    ),
    correlation_id      TEXT,
    idempotency_key     TEXT,
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    started_at          TEXT,
    finished_at         TEXT,
    updated_at          TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (progress_total IS NULL OR progress_completed <= progress_total),
    CHECK (
        (status = 'running' AND lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL)
        OR status != 'running'
    )
);

CREATE UNIQUE INDEX idx_application_jobs_idempotency
    ON application_jobs(kind, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
CREATE INDEX idx_application_jobs_ready
    ON application_jobs(status, priority DESC, next_run_at, id);
CREATE INDEX idx_application_jobs_created
    ON application_jobs(created_at DESC, id DESC);

CREATE TABLE application_job_attempts (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id              INTEGER NOT NULL REFERENCES application_jobs(id) ON DELETE CASCADE,
    attempt_number      INTEGER NOT NULL CHECK (attempt_number > 0),
    worker_id           TEXT NOT NULL CHECK (LENGTH(worker_id) > 0),
    status              TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'succeeded', 'failed', 'interrupted', 'cancelled')),
    error_code          TEXT,
    error_message       TEXT,
    error_details_json  TEXT CHECK (
        error_details_json IS NULL OR json_valid(error_details_json)
    ),
    started_at          TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at         TEXT,
    UNIQUE(job_id, attempt_number)
);

CREATE INDEX idx_application_job_attempts_job
    ON application_job_attempts(job_id, attempt_number DESC);

CREATE TABLE application_job_events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id       INTEGER NOT NULL REFERENCES application_jobs(id) ON DELETE CASCADE,
    attempt_id   INTEGER REFERENCES application_job_attempts(id) ON DELETE SET NULL,
    level        TEXT NOT NULL DEFAULT 'info'
        CHECK (level IN ('debug', 'info', 'warning', 'error')),
    code         TEXT NOT NULL CHECK (LENGTH(code) > 0),
    message      TEXT NOT NULL,
    details_json TEXT CHECK (details_json IS NULL OR json_valid(details_json)),
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_application_job_events_job
    ON application_job_events(job_id, id DESC);
