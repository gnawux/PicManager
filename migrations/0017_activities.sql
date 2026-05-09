CREATE TABLE IF NOT EXISTS activities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sha256 TEXT UNIQUE NOT NULL,
    source_path TEXT NOT NULL,
    file_format TEXT NOT NULL,
    title TEXT,
    activity_type TEXT NOT NULL DEFAULT 'other',
    start_time TEXT,
    end_time TEXT,
    duration_seconds INTEGER,
    distance_meters REAL,
    elevation_gain_meters REAL,
    avg_heart_rate INTEGER,
    max_heart_rate INTEGER,
    calories INTEGER,
    device TEXT,
    import_status TEXT NOT NULL DEFAULT 'imported'
);

CREATE TABLE IF NOT EXISTS activity_track_points (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_id INTEGER NOT NULL REFERENCES activities(id) ON DELETE CASCADE,
    ts TEXT NOT NULL,
    lat REAL NOT NULL,
    lon REAL NOT NULL,
    elevation REAL,
    heart_rate INTEGER,
    cadence INTEGER,
    speed REAL
);

CREATE INDEX IF NOT EXISTS idx_activities_start_time ON activities(start_time DESC);
CREATE INDEX IF NOT EXISTS idx_track_points_activity ON activity_track_points(activity_id);
