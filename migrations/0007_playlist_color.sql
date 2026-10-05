-- [playlist-color] Tono elegido para cada playlist (0-360). Sin fila = tono derivado del id.
-- Para quitar la función: nueva migración con `DROP TABLE playlist_color;`.
CREATE TABLE playlist_color (
    playlist_id TEXT PRIMARY KEY REFERENCES playlist(id) ON DELETE CASCADE,
    hue REAL NOT NULL
);
