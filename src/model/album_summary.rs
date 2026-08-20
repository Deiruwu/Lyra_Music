use crate::model::AlbumType;
use serde::{Deserialize, Serialize};

/// Entrada de la discografía de un artista, sin sus tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumSummary {
    pub id: String,
    pub name: String,
    pub thumbnail_small: Option<String>,
    pub thumbnail_large: Option<String>,
    #[serde(rename = "type")]
    pub album_type: AlbumType,
    pub year: Option<String>,
}
