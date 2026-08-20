use crate::model::{AlbumType, Track};
use serde::{Deserialize, Serialize};

/// Respuesta completa de la acción `album` del microservicio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumDto {
    pub id: String,
    pub name: String,
    pub thumbnail_small: Option<String>,
    pub thumbnail_large: Option<String>,
    #[serde(rename = "type")]
    pub album_type: AlbumType,
    pub year: Option<String>,
    pub tracks: Vec<Track>,
}
