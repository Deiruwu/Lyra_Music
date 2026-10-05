//! Piezas de las páginas de colección (playlist, Explorar, Me gusta): header de
//! borde a borde que scrollea con la tabla, barra de acciones debajo y la banda
//! de color que marca la división. Las constantes son las que la tabla necesita
//! para ubicar sus filas (`TrackViewState::rows_offset`).

use iced::widget::image::Handle;
use iced::widget::{button, column, container, row, scrollable, space};
use iced::{Alignment, Color, Element, Length, Padding};

use crate::model::Track;
use crate::ui::cover_palette;

use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::{spacing, typography};
use crate::ui::styles::button as button_style;
use crate::ui::theme::theme;
use crate::ui::widgets::catalog_status_message::{catalog_status_message, StatusTone};
use crate::ui::widgets::playlist_header::{playlist_header, HeaderCover, PlaylistHeaderData, EDGE_HEADER_HEIGHT};
use crate::ui::widgets::track_list_builder::COLUMN_HEADER_HEIGHT;

/// Alto de la barra de acciones.
pub const ACTION_BAR_HEIGHT: f32 = 76.0;
/// Margen lateral de la barra y la tabla (el header va de borde a borde).
pub const CONTENT_PADDING_X: f32 = spacing::SP_20;
/// Lo que va arriba de las filas dentro del scroll: header, barra y títulos de columna.
pub const ROWS_OFFSET: f32 = EDGE_HEADER_HEIGHT + ACTION_BAR_HEIGHT + COLUMN_HEADER_HEIGHT;
const FILTER_INPUT_WIDTH: f32 = 260.0;

/// Header de borde a borde con el mosaico de carátulas, teñido con `base`.
pub fn mosaic_header<'a, Message: Clone + 'a>(
    kicker: &'a str,
    name: &'a str,
    tracks: &[&Track],
    mosaic: Vec<Option<Handle>>,
    base: Color,
) -> Element<'a, Message> {
    playlist_header(
        PlaylistHeaderData {
            name,
            kicker: Some(kicker),
            description: None,
            track_count: tracks.len(),
            total_duration_seconds: tracks.iter().map(|t| t.duration_seconds as i64).sum(),
            tint: Some(cover_palette::header_tint(base)),
            tint_end: Some(cover_palette::header_tint_end(base)),
            lyrics_count: None,
            edge_to_edge: true,
        },
        HeaderCover::Mosaic(mosaic),
        None,
        None,
        None,
        false,
    )
}

/// Si la canción que suena está entre `tracks`.
pub fn contains_now_playing(tracks: &[&Track], now_playing_id: Option<&str>) -> bool {
    now_playing_id.is_some_and(|id| tracks.iter().any(|t| t.id == id))
}

/// Barra de acciones de alto fijo: `left` pegado a la izquierda, `right` a la derecha.
pub fn action_bar<'a, Message: 'a>(left: Vec<Element<'a, Message>>, right: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        row(left).spacing(spacing::SP_16).align_y(Alignment::Center),
        space().width(Length::Fill),
        row(right).spacing(spacing::SP_16).align_y(Alignment::Center),
    ]
        .align_y(Alignment::Center)
        .height(Length::Fixed(ACTION_BAR_HEIGHT))
        .padding(Padding { left: CONTENT_PADDING_X, right: CONTENT_PADDING_X, ..Padding::ZERO })
        .into()
}

/// Ícono clicable que se pinta con el acento cuando está activo.
pub fn icon_toggle<'a, Message: Clone + 'a>(icon: Icon, is_active: bool, on_press: Message) -> Element<'a, Message> {
    let color = if is_active { theme().accent.primary } else { theme().content.secondary };
    button(icons::icon(icon, typography::TEXT_20).color(color))
        .padding(spacing::SP_8)
        .style(button_style::minimal)
        .on_press(on_press)
        .into()
}

/// Campo del filtro que abre la lupa, de ancho fijo.
pub fn filter_input<'a, Message: Clone + 'a>(
    placeholder: &'a str,
    value: &'a str,
    on_change: impl Fn(String) -> Message + 'a,
    id: &'static str,
) -> Element<'a, Message> {
    container(
        crate::ui::widgets::catalog_search_input::catalog_search_input(placeholder, value, on_change)
            .id(iced::widget::Id::new(id)),
    )
        .width(Length::Fixed(FILTER_INPUT_WIDTH))
        .into()
}

/// Lugar de la tabla cuando no hay filas: header y barra igual, y un mensaje sobre la banda.
pub fn empty_page<'a, Message: 'a>(
    header: Element<'a, Message>,
    toolbar: Element<'a, Message>,
    message: &'a str,
    band: Color,
) -> Element<'a, Message> {
    let below = column![
        toolbar,
        container(catalog_status_message(message, StatusTone::Muted))
            .padding(Padding { left: CONTENT_PADDING_X, right: CONTENT_PADDING_X, ..Padding::ZERO }),
    ];
    let below = container(below).width(Length::Fill).style(move |_theme: &iced::Theme| container::Style {
        background: Some(
            iced::gradient::Linear::new(std::f32::consts::PI)
                .add_stop(0.0, band)
                .add_stop(1.0, theme().surface.panel)
                .into(),
        ),
        ..Default::default()
    });
    scrollable(column![header, below]).height(Length::Fill).into()
}
