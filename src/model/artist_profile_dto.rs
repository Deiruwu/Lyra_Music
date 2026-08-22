use serde::{Deserialize, Serialize};

/// Respuesta de la acción `artist_profile` del microservicio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistProfileDto {
    pub id: String,
    pub name: String,
    pub thumbnail_small: Option<String>,
    pub thumbnail_large: Option<String>,
}
