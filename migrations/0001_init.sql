-- 1. Tabla de Playlists
CREATE TABLE playlist (
                          id TEXT PRIMARY KEY,
                          name TEXT NOT NULL,
                          type TEXT NOT NULL CHECK(type IN ('SYSTEM', 'CUSTOM')) DEFAULT 'CUSTOM',
                          created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 2. Constraint de Sistema Único
-- SQLite permite índices parciales. Esto asegura que solo pueda existir
-- UNA fila con type = 'SYSTEM' en toda la tabla.
CREATE UNIQUE INDEX idx_single_system_playlist ON playlist(type) WHERE type = 'SYSTEM';

-- 3. Relación Playlist <-> Track
CREATE TABLE playlist_track (
                                playlist_id TEXT NOT NULL,
                                track_id TEXT NOT NULL, -- El ID de la youtube. 11 caracteres
                                position REAL,          -- REAL en SQLite equivale a FLOAT/DOUBLE
                                added_at DATETIME DEFAULT CURRENT_TIMESTAMP,

                                PRIMARY KEY (playlist_id, track_id),
                                FOREIGN KEY (playlist_id) REFERENCES playlist(id) ON DELETE CASCADE
);

-- 4. Índices de consulta rápida
CREATE INDEX idx_playlist_track_pos ON playlist_track(playlist_id, position);
CREATE INDEX idx_playlist_track_added ON playlist_track(playlist_id, added_at DESC);

-- 5. Seed / Instanciación base
-- Este insert garantiza que la playlist del sistema nazca con la BD.
INSERT OR IGNORE INTO playlist (id, name, type)
VALUES ('00000000-0000-0000-0000-000000000000', 'Likes', 'SYSTEM');