//! Esquinas redondeadas para paneles cuyo contenido llega al borde. iced no recorta
//! a esquinas redondeadas: sin esto, lo que se scrollea debajo (títulos fijos, banda,
//! filas) se ve con esquinas rectas.
//!
//! Es un anillo: un contenedor `CORNER_RING_WIDTH` más grande que el panel por cada
//! lado, con un borde de ese ancho del color de lo que hay detrás. El borde interior
//! de un quad tiene radio `radio - ancho`, así que coincide justo con la curva del
//! panel y tapa lo que asoma afuera de ella, con el mismo suavizado que el panel.
//! No recibe clics.

use iced::border::rounded;
use iced::widget::{container, space};
use iced::{Color, Element, Length, Theme};

/// Cuánto sobresale el anillo del panel. Tiene que ser al menos ~0.42 × el radio
/// para que su curva exterior todavía cubra la punta de la esquina del panel.
pub const CORNER_RING_WIDTH: f32 = 8.0;

/// Capa a poner encima de un panel de radio `radius` envuelto con `CORNER_RING_WIDTH` de padding.
pub fn corner_ring<'a, Message: 'a>(radius: f32, outside: Color) -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            border: rounded(radius + CORNER_RING_WIDTH).color(outside).width(CORNER_RING_WIDTH),
            ..Default::default()
        })
        .into()
}
