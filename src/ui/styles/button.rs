use iced::border::rounded;
use iced::widget::button;
use iced::{Border, Theme};

use crate::ui::assets::radii;
use crate::ui::theme::theme;

/// Botón sin fondo que solo se insinúa al pasar por encima.
pub fn transparent(_theme: &Theme, status: button::Status) -> button::Style {
    let t = theme();

    let (background, text_color) = match status {
        button::Status::Disabled => (None, t.content.muted),
        button::Status::Hovered => (Some(t.overlay.hover.into()), t.content.primary),
        _ => (None, t.content.primary),
    };

    button::Style {
        background,
        text_color,
        ..Default::default()
    }
}

/// Como `transparent`, pero sin hover ni estado deshabilitado: para filas que momentáneamente no son clicables.
pub fn inert(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        text_color: theme().content.primary,
        ..Default::default()
    }
}

pub fn sidebar_item(_theme: &Theme, status: button::Status) -> button::Style {
    let t = theme();

    let (background, text_color) = match status {
        button::Status::Disabled => (None, t.content.muted),
        button::Status::Hovered => (Some(t.overlay.hover_accent.into()), t.content.primary),
        _ => (None, t.content.primary),
    };

    button::Style {
        background,
        text_color,
        border: Border {
            radius: iced::border::radius(radii::R_NONE).right(radii::R_8),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Botón de solo icono que solo reacciona al pulsarse, tiñéndose de acento.
pub fn minimal(_theme: &Theme, status: button::Status) -> button::Style {
    let t = theme();

    let text_color = match status {
        button::Status::Disabled => t.content.muted,
        button::Status::Pressed => t.accent.primary,
        button::Status::Active | button::Status::Hovered => t.content.primary,
    };

    button::Style {
        background: None,
        text_color,
        border: Border::default(),
        ..Default::default()
    }
}

/// Fila de un menú contextual.
pub fn context_menu_item(_theme: &Theme, status: button::Status) -> button::Style {
    let t = theme();

    let background = match status {
        button::Status::Hovered => Some(t.overlay.hover.into()),
        _ => None,
    };

    button::Style {
        background,
        text_color: t.content.primary,
        border: rounded(crate::ui::assets::radii::R_5),
        ..Default::default()
    }
}

/// Tarjeta de carrusel: fondo que aparece en hover, con el radio que le pase la vista.
pub fn card_hover(radius: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme: &Theme, status: button::Status| {
        let t = theme();

        let background = match status {
            button::Status::Hovered => Some(t.overlay.hover.into()),
            _ => None,
        };

        button::Style {
            background,
            text_color: t.content.primary,
            border: rounded(radius),
            ..Default::default()
        }
    }
}

/// Flecha circular de carrusel. `enabled` decide el aspecto apagado sin
/// depender de `button::Status::Disabled`, igual que hacían las vistas.
pub fn carousel_arrow(enabled: bool, size: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme: &Theme, status: button::Status| {
        let t = theme();

        let background = if !enabled {
            t.overlay.resting
        } else {
            match status {
                button::Status::Hovered => t.overlay.control_hover,
                _ => t.overlay.control_idle,
            }
        };

        button::Style {
            background: Some(background.into()),
            text_color: if enabled {
                t.content.primary
            } else {
                t.content.on_control_disabled
            },
            border: rounded(size / 2.0),
            ..Default::default()
        }
    }
}

/// Píldora de texto con estado activo/inactivo (seguir artista, filtros).
pub fn pill(is_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme: &Theme, status: button::Status| {
        let t = theme();

        let (idle, hovered) = if is_active {
            (t.overlay.toggle_on_idle, t.overlay.toggle_on_hover)
        } else {
            (t.overlay.control_idle, t.overlay.control_hover)
        };

        button::Style {
            background: Some(match status {
                button::Status::Hovered => hovered,
                _ => idle,
            }.into()),
            text_color: t.content.primary,
            border: rounded(radii::R_14),
            ..Default::default()
        }
    }
}
