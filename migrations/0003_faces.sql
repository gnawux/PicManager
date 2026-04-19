CREATE TABLE IF NOT EXISTS faces (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    photo_id    INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
    x           INTEGER NOT NULL,
    y           INTEGER NOT NULL,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    confidence  REAL,
    embedding   BLOB,
    embed_model TEXT,
    detected_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_faces_photo_id ON faces(photo_id);

CREATE TABLE IF NOT EXISTS face_jobs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    status      TEXT NOT NULL DEFAULT 'running',
    scope       TEXT,
    total       INTEGER,
    processed   INTEGER NOT NULL DEFAULT 0,
    started_at  TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT
);
