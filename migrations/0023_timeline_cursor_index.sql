-- Support keyset timeline scans without sorting the complete imported library.
CREATE INDEX idx_photos_timeline_cursor
    ON photos(import_status, taken_at, id);
