use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(rename_all = "SCREAMING_SNAKE_CASE")]
#[sqlx(type_name = "TEXT")]
pub enum PlaylistType {
    System,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub playlist_type: PlaylistType,
    pub created_at: NaiveDateTime,


    #[serde(default)]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTrackRef {
    pub playlist_id: String,
    pub track_id: String,
    pub position: Option<f64>,
    pub added_at: NaiveDateTime,
}