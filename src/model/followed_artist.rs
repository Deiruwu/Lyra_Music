use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowedArtist {
    pub artist_id: String,
    pub name: String,
    pub photo_url: Option<String>,
    pub followed_at: NaiveDateTime,
}
