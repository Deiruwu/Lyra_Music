use iced::{Border, Color, Theme};
use iced::widget::{button, container};

pub fn transparent_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Disabled => (None, Color::from_rgb(0.35, 0.35, 0.38)),
        // Subtle hover feedback para la fila completa tipo Tidal
        button::Status::Hovered => (
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.03).into()),
            Color::WHITE
        ),
        _ => (None, Color::WHITE),
    };

    button::Style {
        background,
        text_color,
        ..Default::default()
    }
}

/// Fondo + borde para la fila seleccionada.
/// Mayor contraste en el canal alfa para diferenciar selección de hover.
pub fn selected_row_container(is_selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| {
        if is_selected {
            container::Style {
                background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08).into()),
                border: Border {
                    radius: 6.0.into(),
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.05),
                    width: 1.0,
                },
                ..Default::default()
            }
        } else {
            container::Style::default()
        }
    }
}

/// Menú contextual estilo panel flotante oscuro.
/// Fondo antracita semi-sólido para evitar que el texto de la fila inferior distraiga.
pub fn context_menu_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb(0.10, 0.10, 0.12).into()),
        border: Border {
            radius: 8.0.into(),
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            width: 1.0,
        },
        ..Default::default()
    }
}

pub fn context_menu_item(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.10).into()),
        _ => None,
    };
    button::Style {
        background,
        text_color: Color::WHITE,
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}