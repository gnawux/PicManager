-- Proximity-derived rows must never become new anchors. Without provenance, a
-- dense coordinate sequence can relay one administrative label for kilometres.
ALTER TABLE geocache ADD COLUMN resolution_method TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE geocache ADD COLUMN anchor_lat REAL;
ALTER TABLE geocache ADD COLUMN anchor_lon REAL;

CREATE INDEX idx_geocache_provider_anchor
    ON geocache(resolution_method, anchor_lat, anchor_lon)
    WHERE resolution_method = 'provider';
