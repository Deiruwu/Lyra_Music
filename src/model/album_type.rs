use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlbumType {
    Album,
    Single,
    EP,
}

impl AlbumType {
    /// Etiqueta legible para la tarjeta de álbum/single.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Album => "Álbum",
            Self::Single => "Single",
            Self::EP => "EP",
        }
    }
}
