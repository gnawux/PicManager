-- Track which language-selection policy produced each cached geographic name.
-- Existing rows remain revision 0 and can be refreshed explicitly without making
-- application startup perform network requests or discarding usable old names.
ALTER TABLE geocache ADD COLUMN name_policy_revision INTEGER NOT NULL DEFAULT 0;
