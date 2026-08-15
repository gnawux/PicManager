-- Multi-source catalog foundation. Existing photos remain authoritative during the
-- compatibility period; the new tables are additive and do not move media files.

CREATE TABLE assets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    photo_id    INTEGER REFERENCES photos(id) ON DELETE SET NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(photo_id)
);

CREATE TABLE asset_sources (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id          INTEGER REFERENCES assets(id) ON DELETE SET NULL,
    provider          TEXT NOT NULL CHECK (LENGTH(provider) > 0),
    external_id       TEXT,
    original_filename TEXT,
    media_type        TEXT,
    width             INTEGER CHECK (width IS NULL OR width > 0),
    height            INTEGER CHECK (height IS NULL OR height > 0),
    taken_at          TEXT,
    metadata_json     TEXT,
    sync_status       TEXT NOT NULL DEFAULT 'discovered'
        CHECK (sync_status IN (
            'discovered', 'queued', 'downloading', 'downloaded', 'importing',
            'ready', 'failed', 'excluded', 'missing'
        )),
    exclusion_reason  TEXT,
    last_error        TEXT,
    last_seen_at      TEXT,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (external_id IS NULL OR LENGTH(external_id) > 0)
);

CREATE UNIQUE INDEX idx_asset_sources_external_identity
    ON asset_sources(provider, external_id)
    WHERE external_id IS NOT NULL;
CREATE INDEX idx_asset_sources_asset ON asset_sources(asset_id);
CREATE INDEX idx_asset_sources_provider_status ON asset_sources(provider, sync_status);

CREATE TABLE asset_variants (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id       INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    source_id      INTEGER REFERENCES asset_sources(id) ON DELETE SET NULL,
    role           TEXT NOT NULL
        CHECK (role IN ('original', 'current', 'imported', 'preview', 'thumbnail', 'raw_companion')),
    path           TEXT,
    content_sha256 TEXT,
    mime_type      TEXT,
    width          INTEGER CHECK (width IS NULL OR width > 0),
    height         INTEGER CHECK (height IS NULL OR height > 0),
    byte_size      INTEGER CHECK (byte_size IS NULL OR byte_size >= 0),
    generation_key TEXT,
    is_primary     INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    created_at     TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX idx_asset_variants_path
    ON asset_variants(path)
    WHERE path IS NOT NULL;
CREATE UNIQUE INDEX idx_asset_variants_identity
    ON asset_variants(
        asset_id,
        role,
        COALESCE(source_id, -1),
        COALESCE(generation_key, '')
    );
CREATE UNIQUE INDEX idx_asset_variants_primary
    ON asset_variants(asset_id)
    WHERE is_primary = 1;
CREATE INDEX idx_asset_variants_hash ON asset_variants(content_sha256);
CREATE INDEX idx_asset_variants_source ON asset_variants(source_id);

CREATE TABLE asset_links (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id     INTEGER NOT NULL REFERENCES asset_sources(id) ON DELETE CASCADE,
    photo_id      INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
    method        TEXT NOT NULL CHECK (LENGTH(method) > 0),
    confidence    REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    status        TEXT NOT NULL DEFAULT 'candidate'
        CHECK (status IN ('candidate', 'accepted', 'rejected', 'conflict')),
    evidence_json TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    reviewed_at   TEXT,
    UNIQUE(source_id, photo_id, method)
);

CREATE INDEX idx_asset_links_source_status ON asset_links(source_id, status);
CREATE INDEX idx_asset_links_photo ON asset_links(photo_id);

CREATE TABLE migration_runs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    kind          TEXT NOT NULL CHECK (LENGTH(kind) > 0),
    status        TEXT NOT NULL DEFAULT 'planned'
        CHECK (status IN ('planned', 'running', 'completed', 'failed')),
    dry_run       INTEGER NOT NULL DEFAULT 0 CHECK (dry_run IN (0, 1)),
    checkpoint    TEXT,
    summary_json  TEXT,
    error         TEXT,
    started_at    TEXT,
    finished_at   TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_migration_runs_kind_created ON migration_runs(kind, created_at DESC);
