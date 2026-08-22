use crate::model::{AlbumSummary, Track};
use serde::{Deserialize, Serialize};

/// Respuesta completa de la acción `artist` del microservicio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistDto {
    pub id: String,
    pub name: String,
    pub banner: Option<String>,
    pub views: Option<i64>,
    pub songs: Vec<Track>,
    pub albums: Vec<AlbumSummary>,
}
