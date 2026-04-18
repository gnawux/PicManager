CREATE TABLE photos (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    path          TEXT    NOT NULL UNIQUE,
    sha256        TEXT    NOT NULL,
    phash         TEXT,
    taken_at      TEXT,
    gps_lat       REAL,
    gps_lon       REAL,
    camera        TEXT,
    format        TEXT    NOT NULL,
    import_status TEXT    NOT NULL DEFAULT 'pending',
    imported_at   TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_photos_sha256        ON photos(sha256);
CREATE INDEX idx_photos_import_status ON photos(import_status);
CREATE INDEX idx_photos_taken_at      ON photos(taken_at);

CREATE TABLE albums (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,  -- 'time' | 'location' | 'camera' | 'manual'
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE photo_albums (
    photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    PRIMARY KEY (photo_id, album_id)
);

CREATE TABLE dedup_groups (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    status TEXT NOT NULL DEFAULT 'pending'  -- 'pending' | 'resolved'
);

CREATE TABLE dedup_members (
    group_id INTEGER NOT NULL REFERENCES dedup_groups(id) ON DELETE CASCADE,
    photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
    keep     INTEGER NOT NULL DEFAULT 0,  -- 1 = keep, 0 = undecided / delete
    PRIMARY KEY (group_id, photo_id)
);

CREATE TABLE import_sessions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    source_dir TEXT    NOT NULL,
    started_at TEXT    NOT NULL DEFAULT (datetime('now')),
    ended_at   TEXT,
    total      INTEGER NOT NULL DEFAULT 0,
    imported   INTEGER NOT NULL DEFAULT 0,
    skipped    INTEGER NOT NULL DEFAULT 0,
    errors     INTEGER NOT NULL DEFAULT 0
);
