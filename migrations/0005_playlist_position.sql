-- Orden manual de las playlists en el sidebar (drag & drop).
ALTER TABLE playlist ADD COLUMN position REAL;

-- Las existentes conservan el orden que tenían (por fecha de creación).
UPDATE playlist
SET position = (
    SELECT COUNT(*)
    FROM playlist AS earlier
    WHERE earlier.created_at < playlist.created_at
       OR (earlier.created_at = playlist.created_at AND earlier.id < playlist.id)
);
