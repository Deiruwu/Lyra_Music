//! # catalog_search_input — caja de búsqueda compartida por las vistas
//! de catálogo (Explorer, Favorites, Playlists detail).
//!
//! Distinto de `search_feature/` (que es la búsqueda global de
//! canciones): esto es únicamente el `text_input` visual que cada vista
//! usa para filtrar SU propia lista ya cargada (`SearchQuery` sigue
//! viviendo en `ui::utils::search` y no cambia). Antes cada vista
//! reconstruía el mismo `text_input` con el mismo estilo a mano —
//! ~10 líneas idénticas × 3 vistas, solo el placeholder cambiaba.
//!
//! ## Uso
//!
//! ```ignore
//! use crate::ui::widgets::catalog_search_input::catalog_search_input;
//!
//! let search_bar = catalog_search_input(
//!     "Buscar por título, artista o álbum...",
//!     &self.search_query,
//!     ExplorerViewMessage::SearchChanged,
//! );
//! ```

use iced::widget::{text_input, TextInput};
use iced::{Length, Padding};

/// Construye el `text_input` de filtro con el estilo compartido
/// (padding, tamaño, radio de borde) usado por Explorer/Favorites/
/// Playlists. `placeholder` es lo único que varía entre vistas.
pub fn catalog_search_input<'a, Message>(
    placeholder: &'a str,
    value: &'a str,
    on_change: impl Fn(String) -> Message + 'a,
) -> TextInput<'a, Message>
where
    Message: Clone + 'a,
{
    text_input(placeholder, value)
        .font(crate::ui::assets::fonts::SF_PRO)
        .size(14)
        .padding(Padding { top: 10.0, bottom: 10.0, left: 14.0, right: 14.0 })
        .style(|theme, status| {
            let mut style = text_input::default(theme, status);
            style.border.radius = 8.0.into();
            style
        })
        .on_input(on_change)
        .width(Length::Fill)
}