use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPlayCount {
    pub track_id: String,
    pub play_count: i64,
    pub last_played_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistPlayCount {
    pub artist_id: String,
    pub artist_name: String,
    pub play_count: i64,
    pub last_played_at: NaiveDateTime,
}
