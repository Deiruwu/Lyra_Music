-- Contador acumulado por track: sin FK (los tracks nunca se guardan
-- localmente, solo se referencian por su id remoto, igual que
-- playlist_track.track_id). Sin tope de play_count.
CREATE TABLE play_history (
                               track_id TEXT PRIMARY KEY,
                               play_count INTEGER NOT NULL DEFAULT 0,
                               last_played_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_play_history_last_played ON play_history(last_played_at DESC);
CREATE INDEX idx_play_history_count ON play_history(play_count DESC);

-- Mismo esquema a nivel artista. Solo se escribe si el artista trae id
-- real -- un id null no es agrupable de forma confiable (ver track_hub_api.md).
CREATE TABLE artist_play_history (
                                      artist_id TEXT PRIMARY KEY,
                                      artist_name TEXT NOT NULL,
                                      play_count INTEGER NOT NULL DEFAULT 0,
                                      last_played_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_artist_play_history_last_played ON artist_play_history(last_played_at DESC);
CREATE INDEX idx_artist_play_history_count ON artist_play_history(play_count DESC);
