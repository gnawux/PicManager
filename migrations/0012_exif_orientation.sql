-- EXIF Orientation tag (0x0112), values 1-8 per EXIF standard
-- 1 = normal; 3 = 180°; 6 = 90° CW; 8 = 270° CW; others include mirror variants
-- image crate ignores this tag when decoding; we store and apply it manually
ALTER TABLE photos ADD COLUMN exif_orientation INTEGER NOT NULL DEFAULT 1;
