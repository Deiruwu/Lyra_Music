use sqlx::SqlitePool;
use crate::model::{ArtistPlayCount, Track, TrackPlayCount};

pub struct PlayHistoryManager {
    pool: SqlitePool,
}

impl PlayHistoryManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Registra una reproducción: incrementa `play_count`/actualiza
    /// `last_played_at` del track, y de cada artista con id real
    /// (`Artist.id: Some(..)` — un id null no es agrupable, ver
    /// track_hub_api.md). Todo en una transacción.
    pub async fn record_play(&self, track: &Track) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        sqlx::query!(
            r#"
            INSERT INTO play_history (track_id, play_count, last_played_at)
            VALUES (?, 1, CURRENT_TIMESTAMP)
            ON CONFLICT(track_id) DO UPDATE SET
                play_count = play_count + 1,
                last_played_at = CURRENT_TIMESTAMP
            "#,
            track.id
        )
            .execute(&mut *tx)
            .await?;

        for artist in &track.artists {
            let Some(artist_id) = artist.id.as_deref() else { continue };

            sqlx::query!(
                r#"
                INSERT INTO artist_play_history (artist_id, artist_name, play_count, last_played_at)
                VALUES (?, ?, 1, CURRENT_TIMESTAMP)
                ON CONFLICT(artist_id) DO UPDATE SET
                    artist_name = excluded.artist_name,
                    play_count = play_count + 1,
                    last_played_at = CURRENT_TIMESTAMP
                "#,
                artist_id,
                artist.name
            )
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn recent_plays(&self, limit: i64) -> Result<Vec<TrackPlayCount>, sqlx::Error> {
        sqlx::query_as!(
            TrackPlayCount,
            r#"
            SELECT
                track_id as "track_id!",
                play_count as "play_count!",
                last_played_at as "last_played_at!"
            FROM play_history
            ORDER BY last_played_at DESC
            LIMIT ?
            "#,
            limit
        )
            .fetch_all(&self.pool)
            .await
    }

    /// Sin consumidores todavía — queda listo para un futuro "top artistas".
    pub async fn top_artists(&self, limit: i64) -> Result<Vec<ArtistPlayCount>, sqlx::Error> {
        sqlx::query_as!(
            ArtistPlayCount,
            r#"
            SELECT
                artist_id as "artist_id!",
                artist_name as "artist_name!",
                play_count as "play_count!",
                last_played_at as "last_played_at!"
            FROM artist_play_history
            ORDER BY play_count DESC
            LIMIT ?
            "#,
            limit
        )
            .fetch_all(&self.pool)
            .await
    }
}
