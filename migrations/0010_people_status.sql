ALTER TABLE people ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
    CHECK (status IN ('active', 'ignored', 'not_a_person'));
