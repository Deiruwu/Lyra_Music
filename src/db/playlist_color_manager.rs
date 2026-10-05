//! [playlist-color] Persistencia del color de cada playlist (ver `ui::playlist_color`).

use sqlx::SqlitePool;

use crate::ui::playlist_color::PlaylistColor;

pub struct PlaylistColorManager {
    pool: SqlitePool,
}

impl PlaylistColorManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Colores elegidos a mano; saturación/brillo `None` = valores por defecto.
    pub async fn load_all(&self) -> Result<Vec<(String, f64, Option<f64>, Option<f64>)>, sqlx::Error> {
        sqlx::query_as::<_, (String, f64, Option<f64>, Option<f64>)>("SELECT playlist_id, hue, saturation, value FROM playlist_color")
            .fetch_all(&self.pool)
            .await
    }

    pub async fn set_color(&self, playlist_id: &str, color: PlaylistColor) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO playlist_color (playlist_id, hue, saturation, value) VALUES (?, ?, ?, ?)
             ON CONFLICT(playlist_id) DO UPDATE SET
                hue = excluded.hue, saturation = excluded.saturation, value = excluded.value",
        )
            .bind(playlist_id)
            .bind(color.hue as f64)
            .bind(color.saturation as f64)
            .bind(color.value as f64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
