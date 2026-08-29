use iced::widget::text_input;
use iced::{Background, Border, Color, Theme};

use crate::ui::assets::radii;
use crate::ui::theme::theme;

/// Campo de texto del sistema: fondo hundido y borde que solo despierta al foco.
pub fn field(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let t = theme();

    let (background, border_color, value) = match status {
        text_input::Status::Disabled => {
            (t.overlay.resting, t.border.subtle, t.content.muted)
        }
        text_input::Status::Focused { .. } => (t.surface.sunken, t.accent.primary, t.content.primary),
        text_input::Status::Hovered => (t.surface.sunken, t.border.field, t.content.primary),
        text_input::Status::Active => (t.surface.sunken, t.border.subtle, t.content.primary),
    };

    text_input::Style {
        background: Background::Color(background),
        border: Border { radius: radii::R_8.into(), width: 1.0, color: border_color },
        icon: t.content.muted,
        placeholder: t.content.muted,
        value,
        selection: t.accent.strong,
    }
}

/// Campo de texto sin fondo ni borde propios — para vivir dentro de un
/// contenedor que ya aporta su propio fondo/sombra (la isla de búsqueda),
/// de modo que el input quede a ras sin una caja anidada visible.
pub fn island(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let t = theme();

    let value = match status {
        text_input::Status::Disabled => t.content.muted,
        _ => t.content.primary,
    };

    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border { radius: radii::R_20.into(), width: 0.0, color: Color::TRANSPARENT },
        icon: t.content.muted,
        placeholder: t.content.muted,
        value,
        selection: t.accent.strong,
    }
}
