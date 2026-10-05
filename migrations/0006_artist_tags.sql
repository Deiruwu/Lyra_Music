-- Etiquetas para organizar artistas seguidos ("Top artistas", "Rock que me mama"...).
-- Un artista puede estar en varias; al dejar de seguirlo sale de todas.
CREATE TABLE artist_tag (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    position REAL NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE artist_tag_member (
    tag_id TEXT NOT NULL REFERENCES artist_tag(id) ON DELETE CASCADE,
    artist_id TEXT NOT NULL REFERENCES followed_artist(artist_id) ON DELETE CASCADE,
    added_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (tag_id, artist_id)
);

CREATE INDEX idx_artist_tag_member_artist ON artist_tag_member(artist_id);
