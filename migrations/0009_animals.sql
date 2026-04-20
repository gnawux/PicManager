CREATE TABLE IF NOT EXISTS animals (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    photo_id    INTEGER NOT NULL REFERENCES photos(id),
    species     TEXT    NOT NULL,
    confidence  REAL    NOT NULL,
    x           INTEGER NOT NULL,
    y           INTEGER NOT NULL,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    detected_at TEXT    NOT NULL DEFAULT (datetime('now'))
);
