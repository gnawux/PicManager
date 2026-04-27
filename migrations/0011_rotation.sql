-- Photo display rotation and flip (stored in DB, original file not modified)
ALTER TABLE photos ADD COLUMN rotation INTEGER NOT NULL DEFAULT 0;
-- Clockwise degrees: 0 / 90 / 180 / 270
ALTER TABLE photos ADD COLUMN flip_h INTEGER NOT NULL DEFAULT 0;
-- Horizontal mirror: 0 = no, 1 = yes
ALTER TABLE photos ADD COLUMN flip_v INTEGER NOT NULL DEFAULT 0;
-- Vertical flip: 0 = no, 1 = yes
