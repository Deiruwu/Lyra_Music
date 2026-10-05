use crate::model::Track;

/// Playlist temporal armada a partir de radios (no se persiste).
#[derive(Debug, Clone, PartialEq)]
pub struct Mix {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub cover_url: Option<String>,
    pub tracks: Vec<Track>,
}
