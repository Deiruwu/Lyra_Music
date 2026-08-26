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
use crate::ui::assets::{radii, spacing, typography};

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
        .size(typography::TEXT_14)
        .padding(Padding { top: spacing::SP_10, bottom: spacing::SP_10, left: spacing::SP_14, right: spacing::SP_14 })
        .style(|theme, status| {
            let mut style = text_input::default(theme, status);
            style.border.radius = radii::R_8.into();
            style
        })
        .on_input(on_change)
        .width(Length::Fill)
}