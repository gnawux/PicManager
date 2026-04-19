CREATE TABLE photo_stats (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    active_count INTEGER NOT NULL DEFAULT 0
);

-- Seed from existing data so the counter is correct after the migration.
INSERT INTO photo_stats (id, active_count)
SELECT 1, COUNT(*) FROM photos WHERE import_status = 'imported';
