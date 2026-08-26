use iced::widget::slider;
use iced::{Background, Border, Theme};

use crate::ui::assets::radii;
use crate::ui::theme::theme;

/// Deslizador de progreso y de volumen: lo recorrido en acento, lo restante en superficie.
pub fn track(_theme: &Theme, status: slider::Status) -> slider::Style {
    let t = theme();

    let handle = match status {
        slider::Status::Hovered => t.accent.hover,
        slider::Status::Dragged => t.accent.strong,
        slider::Status::Active => t.accent.primary,
    };

    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(t.accent.primary),
                Background::Color(t.surface.control),
            ),
            width: 4.0,
            border: Border {
                radius: radii::R_5.into(),
                width: 0.0,
                color: t.border.subtle,
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 6.0 },
            background: Background::Color(handle),
            border_width: 0.0,
            border_color: t.border.subtle,
        },
    }
}
