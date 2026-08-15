-- Separate archival quality from display appearance. The compatibility is_primary flag
-- continues to mirror the selected master while new reads can select the display variant.

ALTER TABLE assets ADD COLUMN master_variant_id INTEGER
    REFERENCES asset_variants(id) ON DELETE SET NULL;
ALTER TABLE assets ADD COLUMN display_variant_id INTEGER
    REFERENCES asset_variants(id) ON DELETE SET NULL;
ALTER TABLE assets ADD COLUMN display_revision INTEGER NOT NULL DEFAULT 0
    CHECK (display_revision >= 0);

UPDATE assets
SET master_variant_id = (
        SELECT id FROM asset_variants
        WHERE asset_variants.asset_id = assets.id AND is_primary = 1
        LIMIT 1
    ),
    display_variant_id = (
        SELECT id FROM asset_variants
        WHERE asset_variants.asset_id = assets.id AND is_primary = 1
        LIMIT 1
    );

CREATE TABLE variant_renditions (
    variant_id          INTEGER PRIMARY KEY REFERENCES asset_variants(id) ON DELETE CASCADE,
    provenance          TEXT NOT NULL CHECK (LENGTH(provenance) > 0),
    byte_preserved      INTEGER NOT NULL DEFAULT 0 CHECK (byte_preserved IN (0, 1)),
    orientation_mode    TEXT NOT NULL
        CHECK (orientation_mode IN ('metadata', 'baked_pixels', 'legacy_unknown')),
    source_orientation  INTEGER CHECK (source_orientation BETWEEN 1 AND 8),
    display_orientation INTEGER CHECK (display_orientation BETWEEN 1 AND 8),
    color_space         TEXT,
    color_profile       TEXT,
    source_fingerprint  TEXT,
    generated_at        TEXT,
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_variant_renditions_fingerprint
    ON variant_renditions(source_fingerprint);

CREATE TABLE asset_display_revisions (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id            INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    revision            INTEGER NOT NULL CHECK (revision > 0),
    previous_variant_id INTEGER REFERENCES asset_variants(id) ON DELETE SET NULL,
    display_variant_id  INTEGER REFERENCES asset_variants(id) ON DELETE SET NULL,
    reason              TEXT NOT NULL CHECK (LENGTH(reason) > 0),
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(asset_id, revision)
);

CREATE INDEX idx_asset_display_revisions_asset
    ON asset_display_revisions(asset_id, revision DESC);
