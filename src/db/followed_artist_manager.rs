use sqlx::SqlitePool;
use crate::model::FollowedArtist;

pub struct FollowedArtistManager {
    pool: SqlitePool,
}

impl FollowedArtistManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn follow(&self, artist_id: &str, name: &str, photo_url: Option<&str>) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO followed_artist (artist_id, name, photo_url)
            VALUES (?, ?, ?)
            ON CONFLICT(artist_id) DO UPDATE SET
                name = excluded.name,
                photo_url = excluded.photo_url
            "#,
            artist_id,
            name,
            photo_url
        )
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn unfollow(&self, artist_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(r#"DELETE FROM followed_artist WHERE artist_id = ?"#, artist_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn is_followed(&self, artist_id: &str) -> Result<bool, sqlx::Error> {
        let count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!" FROM followed_artist WHERE artist_id = ?"#,
            artist_id
        )
            .fetch_one(&self.pool)
            .await?;
        Ok(count > 0)
    }

    /// Trae los últimos `limit` seguidos, del más antiguo al más reciente.
    pub async fn list_followed(&self, limit: i64) -> Result<Vec<FollowedArtist>, sqlx::Error> {
        let artists = sqlx::query_as!(
            FollowedArtist,
            r#"
            SELECT
                artist_id as "artist_id!",
                name as "name!",
                photo_url,
                followed_at as "followed_at!"
            FROM followed_artist
            ORDER BY followed_at DESC
            LIMIT ?
            "#,
            limit
        )
            .fetch_all(&self.pool)
            .await?;

        Ok(artists.into_iter().rev().collect())
    }
}
