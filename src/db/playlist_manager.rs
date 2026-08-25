use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use uuid::Uuid;
use crate::model::playlist::{Playlist, PlaylistType};

pub struct PlaylistManager {
    pool: SqlitePool,
    system_playlist_id: String,
}

impl PlaylistManager {
    pub async fn new(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        let system_id = sqlx::query_scalar!(
            r#"SELECT id as "id!" FROM playlist WHERE type = 'SYSTEM'"#
        )
            .fetch_one(&pool)
            .await?;

        Ok(Self {
            pool,
            system_playlist_id: system_id,
        })
    }

    pub fn system_playlist_id(&self) -> &str {
        &self.system_playlist_id
    }

    // ── GESTIÓN DE PLAYLISTS CUSTOM ──────────────────────────────────────────

    pub async fn create_playlist(&self, name: &str) -> Result<String, sqlx::Error> {
        let id = Uuid::new_v4().to_string();

        // cover_url queda como NULL por defecto al omitirlo en el INSERT
        sqlx::query!(
            r#"INSERT INTO playlist (id, name, type) VALUES (?, ?, 'CUSTOM')"#,
            id,
            name
        )
            .execute(&self.pool)
            .await?;

        Ok(id)
    }

    pub async fn delete_playlist(&self, id: &str) -> Result<(), sqlx::Error> {
        if id == self.system_playlist_id {
            return Err(sqlx::Error::Protocol("Cannot delete SYSTEM playlist".into()));
        }

        sqlx::query!(r#"DELETE FROM playlist WHERE id = ?"#, id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn rename_playlist(&self, id: &str, new_name: &str) -> Result<(), sqlx::Error> {
        if id == self.system_playlist_id {
            return Err(sqlx::Error::Protocol("Cannot rename SYSTEM playlist".into()));
        }

        sqlx::query!(r#"UPDATE playlist SET name = ? WHERE id = ?"#, new_name, id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Permite actualizar la URL del cover agregado en la migración.
    pub async fn update_playlist_cover(&self, id: &str, cover_url: Option<&str>) -> Result<(), sqlx::Error> {
        if id == self.system_playlist_id {
            return Err(sqlx::Error::Protocol("Cannot update cover for SYSTEM playlist".into()));
        }

        sqlx::query!(r#"UPDATE playlist SET cover_url = ? WHERE id = ?"#, cover_url, id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_all_playlists(&self) -> Result<Vec<Playlist>, sqlx::Error> {
        let playlists = sqlx::query_as!(
            Playlist,
            r#"SELECT
                id as "id!",
                name as "name!",
                type as "playlist_type!: PlaylistType",
                created_at as "created_at!",
                cover_url
               FROM playlist
               ORDER BY created_at ASC"#
        )
            .fetch_all(&self.pool)
            .await?;

        Ok(playlists)
    }

    // ── LIKES (SYSTEM PLAYLIST) ──────────────────────────────────────────────

    pub async fn like_track(&self, track_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT OR IGNORE INTO playlist_track (playlist_id, track_id, position)
            VALUES (
                ?,
                ?,
                (SELECT COALESCE(MAX(position), 0.0) + 1024.0
                 FROM playlist_track
                 WHERE playlist_id = ?)
            )
            "#,
            self.system_playlist_id,
            track_id,
            self.system_playlist_id
        )
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn dislike_track(&self, track_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"DELETE FROM playlist_track WHERE playlist_id = ? AND track_id = ?"#,
            self.system_playlist_id,
            track_id
        )
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── OPERACIONES DE TRACKS EN CUSTOM PLAYLISTS ────────────────────────────

    /// Agrega un track a una playlist CUSTOM en la posición dada.
    pub async fn add_track(&self, playlist_id: &str, track_id: &str, position: f64) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"INSERT INTO playlist_track (playlist_id, track_id, position) VALUES (?, ?, ?)"#,
            playlist_id,
            track_id,
            position
        )
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn remove_tracks(&self, playlist_id: &str, track_ids: &[String]) -> Result<(), sqlx::Error> {
        if track_ids.is_empty() { return Ok(()); }

        if playlist_id == self.system_playlist_id {
            return Err(sqlx::Error::Protocol("Use dislike_track for SYSTEM playlist".into()));
        }

        let mut query_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "DELETE FROM playlist_track WHERE playlist_id = "
        );
        query_builder.push_bind(playlist_id);
        query_builder.push(" AND track_id IN (");

        let mut separated = query_builder.separated(", ");
        for id in track_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");

        query_builder.build().execute(&self.pool).await?;
        Ok(())
    }

    /// Actualiza la posición de un track.
    pub async fn update_position(&self, playlist_id: &str, track_id: &str, new_position: f64) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"UPDATE playlist_track SET position = ? WHERE playlist_id = ? AND track_id = ?"#,
            new_position,
            playlist_id,
            track_id
        )
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Renumera todas las posiciones de una playlist.
    pub async fn renumber_playlist(&self, playlist_id: &str, updates: &[(String, f64)]) -> Result<(), sqlx::Error> {
        if updates.is_empty() { return Ok(()); }

        let mut tx = self.pool.begin().await?;

        for (track_id, new_position) in updates {
            sqlx::query!(
                r#"UPDATE playlist_track SET position = ? WHERE playlist_id = ? AND track_id = ?"#,
                new_position,
                playlist_id,
                track_id
            )
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    // ── CONSULTAS ────────────────────────────────────────────────────────────

    pub async fn get_playlist_track_ids(&self, playlist_id: &str) -> Result<Vec<String>, sqlx::Error> {
        let ids = sqlx::query_scalar!(
            r#"SELECT track_id FROM playlist_track WHERE playlist_id = ? ORDER BY position ASC"#,
            playlist_id
        )
            .fetch_all(&self.pool)
            .await?;

        Ok(ids)
    }

    /// Igual a `get_playlist_track_ids`, pero incluye la posición de cada fila.
    pub async fn get_playlist_track_positions(&self, playlist_id: &str) -> Result<Vec<(String, f64)>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT track_id as "track_id!", position as "position!: f64"
               FROM playlist_track WHERE playlist_id = ? ORDER BY position ASC"#,
            playlist_id
        )
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|r| (r.track_id, r.position)).collect())
    }

    pub async fn get_all_playlist_track_positions(&self) -> Result<Vec<(String, Vec<(String, f64)>)>, sqlx::Error> {
        let playlists = self.get_all_playlists().await?;

        let mut result = Vec::with_capacity(playlists.len());
        for playlist in playlists {
            if playlist.id == self.system_playlist_id {
                continue;
            }
            let positions = self.get_playlist_track_positions(&playlist.id).await?;
            result.push((playlist.id, positions));
        }

        Ok(result)
    }
}