use sqlx::SqlitePool;

use crate::model::ArtistTag;

/// Persistencia de las etiquetas de artistas y sus miembros (SQLite local).
pub struct ArtistTagManager {
    pool: SqlitePool,
}

impl ArtistTagManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Etiquetas en orden y pares `(tag_id, artist_id)` en el orden de cada etiqueta.
    pub async fn load(&self) -> Result<(Vec<ArtistTag>, Vec<(String, String)>), sqlx::Error> {
        let tags = sqlx::query_as::<_, ArtistTag>("SELECT id, name, position FROM artist_tag ORDER BY position, created_at")
            .fetch_all(&self.pool)
            .await?;
        let members = sqlx::query_as::<_, (String, String)>(
            "SELECT tag_id, artist_id FROM artist_tag_member ORDER BY tag_id, COALESCE(position, 1e18), added_at, rowid",
        )
            .fetch_all(&self.pool)
            .await?;
        Ok((tags, members))
    }

    pub async fn create_tag(&self, tag: &ArtistTag) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO artist_tag (id, name, position) VALUES (?, ?, ?)")
            .bind(&tag.id)
            .bind(&tag.name)
            .bind(tag.position)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn rename_tag(&self, tag_id: &str, name: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE artist_tag SET name = ? WHERE id = ?")
            .bind(name)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_tag_positions(&self, positions: &[(String, f64)]) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for (tag_id, position) in positions {
            sqlx::query("UPDATE artist_tag SET position = ? WHERE id = ?")
                .bind(position)
                .bind(tag_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }

    pub async fn delete_tag(&self, tag_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM artist_tag WHERE id = ?")
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Agrega al artista al final de la etiqueta.
    pub async fn add_member(&self, tag_id: &str, artist_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR IGNORE INTO artist_tag_member (tag_id, artist_id, position)
             VALUES (?, ?, (SELECT COALESCE(MAX(position), -1) + 1 FROM artist_tag_member WHERE tag_id = ?))",
        )
            .bind(tag_id)
            .bind(artist_id)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Guarda el orden de la etiqueta: `artist_ids[i]` queda en la posición `i`.
    pub async fn set_member_order(&self, tag_id: &str, artist_ids: &[String]) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for (position, artist_id) in artist_ids.iter().enumerate() {
            sqlx::query("UPDATE artist_tag_member SET position = ? WHERE tag_id = ? AND artist_id = ?")
                .bind(position as f64)
                .bind(tag_id)
                .bind(artist_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }

    pub async fn remove_member(&self, tag_id: &str, artist_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM artist_tag_member WHERE tag_id = ? AND artist_id = ?")
            .bind(tag_id)
            .bind(artist_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
