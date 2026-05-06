-- Supplement to 0015: handle two remaining geocache inconsistency patterns.
--
-- Pattern 1: " / " separator (e.g. "荷兰 / 荷蘭", "韩国 / 南韷").
--   OSM sometimes stores both Simplified and Traditional Chinese as a single
--   combined field value with " / " as the separator. Keep only the first part.
--
-- Pattern 2: Non-ASCII, non-CJK country names (e.g. "Österreich" in German).
--   0015 only null-ed out pure-ASCII entries. European names with accented
--   characters (é, ö, ü …) slipped through because they are non-ASCII.
--   Detection: UTF-8 encodes ASCII chars as 1 byte, Latin-supplement as 2 bytes,
--   CJK ideographs as 3 bytes. If avg bytes/char ≤ 2 there are no CJK characters
--   and the name is not Chinese. Reset these entries so fill-geo re-geocodes them
--   with accept-language=zh-CN,zh,en and returns consistent Chinese names.
--   Example: "Österreich" (9 chars, 10 bytes): 10 ≤ 9×2=18 → reset.
--   Counter-example: "美国" (2 chars, 6 bytes): 6 ≤ 2×2=4 → false → keep.

-- Step 1: null-out entries whose country has no CJK characters.
UPDATE geocache
SET city = NULL, state = NULL, county = NULL, country = NULL
WHERE country IS NOT NULL
  AND LENGTH(CAST(country AS BLOB)) <= LENGTH(country) * 2;

-- Step 2: normalize " / " separators in remaining entries.
UPDATE geocache
SET country = TRIM(SUBSTR(country, 1, INSTR(country, ' / ') - 1))
WHERE country LIKE '% / %';

UPDATE geocache
SET state = TRIM(SUBSTR(state, 1, INSTR(state, ' / ') - 1))
WHERE state LIKE '% / %';

UPDATE geocache
SET city = TRIM(SUBSTR(city, 1, INSTR(city, ' / ') - 1))
WHERE city LIKE '% / %';

UPDATE geocache
SET county = TRIM(SUBSTR(county, 1, INSTR(county, ' / ') - 1))
WHERE county LIKE '% / %';

-- Step 3: remove location albums whose names contain " / " (recreated by fill-geo).
DELETE FROM photo_albums
WHERE album_id IN (
    SELECT id FROM albums WHERE kind = 'location' AND name LIKE '% / %'
);
DELETE FROM albums WHERE kind = 'location' AND name LIKE '% / %';
