use iced::border::radius;
use iced::widget::container;
use iced::{Border, Theme};

use crate::ui::assets::radii;
use crate::ui::theme::theme;

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

/// Fondo de la fila seleccionada.
///
/// Cuando varias filas contiguas están seleccionadas se "fusionan": el
/// radio de esquina solo aparece en los bordes externos del bloque
/// (arriba de la primera, abajo de la última). No se dibuja borde en
/// ningún caso — solo fondo — porque un borde de 1px por fila, aunque
/// comparta color con la vecina, sigue siendo una línea visible en el
/// punto de contacto; sin borde, el fondo compartido entre filas
/// contiguas se ve como un solo bloque continuo.
pub fn selected(shape: RowSelectionShape) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| {
        let background = Some(theme().overlay.selected.into());

        match shape {
            RowSelectionShape::None => container::Style::default(),
            RowSelectionShape::Solo => container::Style {
                background,
                border: Border {
                    radius: radius(radii::R_6),
                    ..Default::default()
                },
                ..Default::default()
            },
            RowSelectionShape::First => container::Style {
                background,
                border: Border {
                    radius: radius(radii::R_NONE).top(radii::R_6),
                    ..Default::default()
                },
                ..Default::default()
            },
            RowSelectionShape::Middle => container::Style {
                background,
                border: Border {
                    radius: radius(radii::R_NONE),
                    ..Default::default()
                },
                ..Default::default()
            },
            RowSelectionShape::Last => container::Style {
                background,
                border: Border {
                    radius: radius(radii::R_NONE).bottom(radii::R_6),
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }
}
