-- Track whether derived pixels and analysis correspond to the selected display rendition.
-- Existing rows remain revision zero and therefore keep their current cache compatibility.

ALTER TABLE photos ADD COLUMN render_revision INTEGER NOT NULL DEFAULT 0
    CHECK (render_revision >= 0);

ALTER TABLE faces ADD COLUMN render_revision INTEGER NOT NULL DEFAULT 0
    CHECK (render_revision >= 0);

CREATE TABLE derived_media_state (
    photo_id          INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
    render_revision   INTEGER NOT NULL CHECK (render_revision >= 0),
    thumbnail_status  TEXT NOT NULL DEFAULT 'pending'
        CHECK (thumbnail_status IN ('pending', 'ready', 'failed')),
    face_status       TEXT NOT NULL DEFAULT 'pending'
        CHECK (face_status IN ('pending', 'processing', 'ready', 'failed')),
    last_error        TEXT,
    invalidated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    thumbnail_at      TEXT,
    face_at           TEXT,
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_derived_media_pending
    ON derived_media_state(thumbnail_status, face_status, updated_at);
