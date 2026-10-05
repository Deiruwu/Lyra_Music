/// Etiqueta definida por el usuario para agrupar artistas seguidos.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ArtistTag {
    pub id: String,
    pub name: String,
    pub position: f64,
}
