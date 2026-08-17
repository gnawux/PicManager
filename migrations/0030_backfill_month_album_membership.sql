-- Repair imported photos that predate incremental month-album maintenance.
-- This is derived metadata only: no photo, source, or media path is rewritten.
INSERT INTO albums (name, kind)
SELECT months.month, 'time'
FROM (
    SELECT DISTINCT substr(taken_at, 1, 7) AS month
    FROM photos
    WHERE import_status = 'imported'
      AND taken_at IS NOT NULL
      AND length(taken_at) >= 7
) AS months
WHERE NOT EXISTS (
    SELECT 1 FROM albums a WHERE a.name = months.month AND a.kind = 'time'
);

INSERT OR IGNORE INTO photo_albums (photo_id, album_id)
SELECT p.id,
       (SELECT MIN(a.id)
        FROM albums a
        WHERE a.kind = 'time' AND a.name = substr(p.taken_at, 1, 7))
FROM photos p
WHERE p.import_status = 'imported'
  AND p.taken_at IS NOT NULL
  AND length(p.taken_at) >= 7;
