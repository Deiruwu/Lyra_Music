//! # catalog_status_message — mensaje centrado de estado (cargando,
//! error, vacío) usado como `body_content` de las vistas de catálogo
//! mientras no hay tabla que mostrar.
//!
//! Las tres vistas repetían el mismo bloque
//! `container(text(...)).width(Fill).padding(40).align_x(Center).style(...)`
//! entre 3 y 4 veces cada una (loading, error de conexión, catálogo
//! vacío, búsqueda sin resultados) — solo el texto y el color cambian.
//! `StatusTone` captura esas tres variantes de color ya usadas en el
//! código; el resto del layout se resuelve una sola vez aquí.
//!
//! ## Uso
//!
//! ```ignore
//! use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
//!
//! let body_content: Element<'_, ExplorerViewMessage> = if store.is_loading() {
//!     catalog_status_message("Cargando catálogo desde microservicios...", StatusTone::Neutral)
//! } else if let Some(err) = store.last_error() {
//!     catalog_status_message(&format!("Error de conexión: {}", err), StatusTone::Error)
//! } else if self.visible_count() == 0 {
//!     catalog_status_message("No se encontraron pistas que coincidan con tu búsqueda.", StatusTone::Muted)
//! } else {
//!     // ... track_list(...)
//! };
//! ```

use iced::widget::{container, text};
use iced::{Alignment, Color, Element, Length};

/// Los tres tonos de color que ya usaban Explorer/Favorites/Playlists
/// para sus mensajes de estado. `Neutral` no fija `text_color`
/// (mantiene el color por defecto del tema, igual que el mensaje de
/// "cargando" original); `Error` y `Muted` son los mismos RGB que ya
/// estaban hardcodeados en cada vista.
pub enum StatusTone {
    Neutral,
    Error,
    Muted,
}

impl StatusTone {
    fn color(&self) -> Option<Color> {
        match self {
            StatusTone::Neutral => None,
            StatusTone::Error => Some(Color::from_rgb(0.9, 0.4, 0.4)),
            StatusTone::Muted => Some(Color::from_rgb(0.6, 0.6, 0.65)),
        }
    }
}

/// Construye el mensaje centrado con el layout/estilo compartido.
/// `message` se recibe como `String` (no `&str`) porque el caso de
/// error de cada vista ya llega formateado con `format!(...)` — evita
/// que el llamador tenga que decidir entre pasar owned/borrowed según
/// el caso.
pub fn catalog_status_message<'a, Message: 'a>(
    message: impl Into<String>,
    tone: StatusTone,
) -> Element<'a, Message> {
    let color = tone.color();

    container(
        text(message.into())
            .font(crate::ui::assets::fonts::SF_PRO)
            .size(14),
    )
        .width(Length::Fill)
        .padding(40)
        .align_x(Alignment::Center)
        .style(move |_| container::Style {
            text_color: color,
            ..Default::default()
        })
        .into()
}