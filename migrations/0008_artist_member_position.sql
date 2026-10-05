-- Orden manual de los artistas dentro de cada etiqueta (arrastrar en la grilla).
ALTER TABLE artist_tag_member ADD COLUMN position REAL;

-- Los existentes conservan el orden en que se agregaron.
UPDATE artist_tag_member
SET position = (
    SELECT COUNT(*)
    FROM artist_tag_member AS earlier
    WHERE earlier.tag_id = artist_tag_member.tag_id
      AND (earlier.added_at < artist_tag_member.added_at
           OR (earlier.added_at = artist_tag_member.added_at AND earlier.rowid < artist_tag_member.rowid))
);
