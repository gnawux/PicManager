CREATE TABLE filesystem_intents (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    kind          TEXT NOT NULL CHECK (LENGTH(kind) > 0),
    owner_kind    TEXT,
    owner_id      INTEGER,
    target_path   TEXT NOT NULL CHECK (LENGTH(target_path) > 0),
    staging_path  TEXT NOT NULL CHECK (LENGTH(staging_path) > 0),
    status        TEXT NOT NULL DEFAULT 'planned'
        CHECK (status IN ('planned', 'staged', 'committed', 'failed')),
    error         TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at  TEXT
);

CREATE INDEX idx_filesystem_intents_status
    ON filesystem_intents(status, id);
