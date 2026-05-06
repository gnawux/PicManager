-- Normalize geocache entries that were stored with inconsistent language names.
--
-- Problem 1: accept-language=zh caused Nominatim to concatenate zh-CN and zh-TW
--   variants with ";", producing entries like country="美国;美國". Keep only the
--   first segment before the semicolon.
--
-- Problem 2: Some entries were stored with English names (e.g. country="united states")
--   while newer entries for the same country are in Chinese ("美国"). This splits the
--   geo hierarchy into duplicate country nodes.
--   Detection: LENGTH(text) counts Unicode code points; LENGTH(CAST(text AS BLOB))
--   counts UTF-8 bytes. For pure ASCII strings the two are equal; for strings
--   containing CJK characters they differ (each CJK char is 3 bytes).
--   Reset all-ASCII country entries to NULL so group_by_location re-geocodes them
--   with the updated accept-language (zh-CN,zh,en) and returns Chinese names.

-- Step 1: null-out entries whose country name is pure ASCII (English).
--   city/state/county are reset too so the entry is "truly_empty" and will be
--   retried by cached_or_fetch on the next fill-geo run.
UPDATE geocache
SET city = NULL, state = NULL, county = NULL, country = NULL
WHERE country IS NOT NULL
  AND LENGTH(country) = LENGTH(CAST(country AS BLOB));

-- Step 2: for any remaining entries with semicolons, keep only the first segment.
UPDATE geocache
SET country = TRIM(SUBSTR(country, 1, INSTR(country, ';') - 1))
WHERE country LIKE '%;%';

UPDATE geocache
SET state = TRIM(SUBSTR(state, 1, INSTR(state, ';') - 1))
WHERE state LIKE '%;%';

UPDATE geocache
SET city = TRIM(SUBSTR(city, 1, INSTR(city, ';') - 1))
WHERE city LIKE '%;%';

UPDATE geocache
SET county = TRIM(SUBSTR(county, 1, INSTR(county, ';') - 1))
WHERE county LIKE '%;%';

-- Step 3: remove location albums whose names contain semicolons (artifact of the
--   old geocoding bug). They will be recreated with correct names after fill-geo.
DELETE FROM photo_albums
WHERE album_id IN (
    SELECT id FROM albums WHERE kind = 'location' AND name LIKE '%;%'
);
DELETE FROM albums WHERE kind = 'location' AND name LIKE '%;%';
