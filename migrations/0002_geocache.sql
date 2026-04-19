CREATE TABLE IF NOT EXISTS geocache (
    lat_key   TEXT NOT NULL,
    lon_key   TEXT NOT NULL,
    city      TEXT,
    cached_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (lat_key, lon_key)
);
