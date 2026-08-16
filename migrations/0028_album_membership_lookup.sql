-- Album browsing starts from an album and then resolves its photos. The primary key
-- has photo_id first, so it cannot efficiently serve this direction of the relation.
CREATE INDEX IF NOT EXISTS idx_photo_albums_album_photo
    ON photo_albums(album_id, photo_id);
