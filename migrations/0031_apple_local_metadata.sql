-- Record completion of local EXIF extraction separately from provider inventory metadata.
ALTER TABLE asset_sources ADD COLUMN local_metadata_at TEXT;
ALTER TABLE asset_sources ADD COLUMN local_metadata_error TEXT;
