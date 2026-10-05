use iced::border::rounded;
use iced::widget::scrollable;
use iced::{Background, Theme};

use crate::ui::assets::radii;
use crate::ui::theme::theme;

/// Barra de desplazamiento que se mantiene por debajo del contenido.
pub fn discreet(iced_theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let t = theme();

    let hovered = match status {
        scrollable::Status::Hovered { is_vertical_scrollbar_hovered, .. } => {
            is_vertical_scrollbar_hovered
        }
        scrollable::Status::Dragged { is_vertical_scrollbar_dragged, .. } => {
            is_vertical_scrollbar_dragged
        }
        scrollable::Status::Active { .. } => false,
    };

    let scroller = if hovered { t.overlay.control_hover } else { t.overlay.control_idle };

    let rail = scrollable::Rail {
        background: Some(Background::Color(t.overlay.resting)),
        border: rounded(radii::R_5),
        scroller: scrollable::Scroller {
            background: Background::Color(scroller),
            border: rounded(radii::R_5),
        },
    };

    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        ..scrollable::default(iced_theme, status)
    }
}

/// Hueco arriba y abajo de la barra de las páginas de borde a borde, para que no
/// llegue a las esquinas redondeadas del panel.
pub const INSET_SCROLLBAR_GAP: f32 = 14.0;
/// Ancho de la barra nativa de iced (la que se dibuja encima va en el mismo lugar).
pub const NATIVE_SCROLLBAR_WIDTH: f32 = 10.0;
const INSET_SCROLLER_MIN_HEIGHT: f32 = 24.0;

/// Barra nativa invisible: sigue respondiendo al arrastre, pero la que se ve es `inset_scrollbar`.
pub fn invisible(iced_theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = discreet(iced_theme, status);
    style.vertical_rail.background = None;
    style.vertical_rail.scroller.background = Background::Color(iced::Color::TRANSPARENT);
    style
}

/// Barra con el aspecto de `discreet`, pero con `INSET_SCROLLBAR_GAP` libre arriba y abajo.
/// Va encima del scrollable (con `invisible`); vacía si el contenido entra entero.
pub fn inset_scrollbar<'a, Message: 'a>(offset_y: f32, viewport_height: f32, content_height: f32) -> iced::Element<'a, Message> {
    use iced::widget::{container, space};
    use iced::Length;

    let track_height = viewport_height - 2.0 * INSET_SCROLLBAR_GAP;
    if viewport_height <= 0.0 || content_height <= viewport_height || track_height <= 0.0 {
        return space().into();
    }

    let t = theme();
    let scroller_height = (track_height * viewport_height / content_height).clamp(INSET_SCROLLER_MIN_HEIGHT.min(track_height), track_height);
    let progress = (offset_y / (content_height - viewport_height)).clamp(0.0, 1.0);
    let scroller_top = progress * (track_height - scroller_height);

    let scroller = container(space())
        .width(Length::Fixed(NATIVE_SCROLLBAR_WIDTH))
        .height(Length::Fixed(scroller_height))
        .style(move |_theme: &Theme| iced::widget::container::Style {
            background: Some(Background::Color(t.overlay.control_idle)),
            border: rounded(radii::R_5),
            ..Default::default()
        });
    let rail = container(scroller)
        .width(Length::Fixed(NATIVE_SCROLLBAR_WIDTH))
        .height(Length::Fixed(track_height))
        .padding(iced::Padding { top: scroller_top, ..iced::Padding::ZERO })
        .style(move |_theme: &Theme| iced::widget::container::Style {
            background: Some(Background::Color(t.overlay.resting)),
            border: rounded(radii::R_5),
            ..Default::default()
        });

    container(rail)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::End)
        .padding(iced::Padding { top: INSET_SCROLLBAR_GAP, ..iced::Padding::ZERO })
        .into()
}
