use iced::{Border, Color, Theme};
use iced::widget::{button, container};

pub fn transparent_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Disabled => (None, Color::from_rgb(0.35, 0.35, 0.38)),
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

pub fn minimal_button(_theme: &Theme, status: button::Status) -> button::Style {
    let text_color = match status {
        button::Status::Disabled => {
            Color::from_rgb(0.35, 0.35, 0.38)
        }

        button::Status::Hovered
        | button::Status::Pressed => {
            Color::WHITE
        }

        button::Status::Active => {
            Color::from_rgb(0.82, 0.82, 0.82)
        }
    };

    button::Style {
        background: None,
        text_color,
        border: Border::default(),
        ..Default::default()
    }
}

/// Posición de una fila dentro de un bloque contiguo de filas seleccionadas.
/// Determina qué esquinas llevan radio y si el borde interior (el que
/// colinda con otra fila seleccionada) se dibuja o no, para que el bloque
/// se vea como una sola "píldora" en vez de tarjetas separadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowSelectionShape {
    /// No seleccionada: sin fondo ni borde.
    None,
    /// Seleccionada, ni la fila anterior ni la siguiente lo están: radio en las 4 esquinas.
    Solo,
    /// Seleccionada, la siguiente también lo está (la anterior no): radio solo arriba.
    First,
    /// Seleccionada, la anterior también lo está (la siguiente no): radio solo abajo.
    Last,
    /// Seleccionada, anterior y siguiente también: sin radio, esquinas rectas.
    Middle,
}

impl RowSelectionShape {
    pub fn from_neighbors(is_selected: bool, prev_selected: bool, next_selected: bool) -> Self {
        if !is_selected {
            RowSelectionShape::None
        } else {
            match (prev_selected, next_selected) {
                (false, false) => RowSelectionShape::Solo,
                (false, true) => RowSelectionShape::First,
                (true, false) => RowSelectionShape::Last,
                (true, true) => RowSelectionShape::Middle,
            }
        }
    }
}

/// Fondo + borde para la fila seleccionada.
/// Mayor contraste en el canal alfa para diferenciar selección de hover.
///
/// Cuando varias filas contiguas están seleccionadas se "fusionan": el
/// radio de esquina solo aparece en los bordes externos del bloque
/// (arriba de la primera, abajo de la última). No se dibuja borde en
/// ningún caso — solo fondo — porque un borde de 1px por fila, aunque
/// comparta color con la vecina, sigue siendo una línea visible en el
/// punto de contacto; sin borde, el fondo compartido entre filas
/// contiguas se ve como un solo bloque continuo.
pub fn selected_row_container(shape: RowSelectionShape) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| {
        use iced::border::radius;

        let background = Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08).into());

        match shape {
            RowSelectionShape::None => container::Style::default(),
            RowSelectionShape::Solo => container::Style {
                background,
                border: Border {
                    radius: radius(6.0),
                    ..Default::default()
                },
                ..Default::default()
            },
            RowSelectionShape::First => container::Style {
                background,
                border: Border {
                    // Radio solo en las esquinas superiores.
                    radius: iced::border::radius(0).top(6.0),
                    ..Default::default()
                },
                ..Default::default()
            },
            RowSelectionShape::Middle => container::Style {
                background,
                border: Border {
                    radius: radius(0.0),
                    ..Default::default()
                },
                ..Default::default()
            },
            RowSelectionShape::Last => container::Style {
                background,
                border: Border {
                    // Radio solo en las esquinas inferiores.
                    radius: iced::border::radius(0).bottom(6.0),
                    ..Default::default()
                },
                ..Default::default()
            },
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