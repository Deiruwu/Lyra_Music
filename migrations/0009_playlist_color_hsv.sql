-- [playlist-color] Saturación y brillo (0-1) además del tono; NULL = valores por defecto.
ALTER TABLE playlist_color ADD COLUMN saturation REAL;
ALTER TABLE playlist_color ADD COLUMN value REAL;
