CREATE TABLE followed_artist (
                                 artist_id TEXT PRIMARY KEY,
                                 name TEXT NOT NULL,
                                 photo_url TEXT,
                                 followed_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_followed_artist_followed_at ON followed_artist(followed_at DESC);
