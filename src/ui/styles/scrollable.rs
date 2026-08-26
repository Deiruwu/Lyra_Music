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
        background: Some(Background::Color(t.overlay.hover_subtle)),
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
