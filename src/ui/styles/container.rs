use iced::widget::container;
use iced::{Border, Theme};

use crate::ui::assets::radii;
use crate::ui::theme::theme;

/// Panel flotante del menú contextual.
pub fn context_menu(_theme: &Theme) -> container::Style {
    let t = theme();

    container::Style {
        background: Some(t.surface.elevated.into()),
        border: Border {
            radius: radii::R_8.into(),
            color: t.border.subtle,
            width: 1.0,
        },
        ..Default::default()
    }
}

/// Fondo del panel de la cola, tanto vacío como con contenido.
pub fn queue_panel(_theme: &Theme) -> container::Style {
    let t = theme();

    container::Style {
        background: Some(t.surface.panel.into()),
        border: iced::border::rounded(radii::R_12),
        ..Default::default()
    }
}
