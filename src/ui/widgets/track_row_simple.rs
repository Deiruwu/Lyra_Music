use iced::border::rounded;
use iced::widget::image::Handle;
use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Alignment, Element, Length, Padding, Theme};

use crate::model::{Track, TrackState};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::styles::row as row_style;
use crate::ui::styles::RowSelectionShape;
use crate::ui::theme::theme;
use crate::ui::widgets::artist_links::{album_link, artist_links};
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_row::track_thumbnail_sized;
use crate::utils::formatting::format_duration;
use crate::ui::assets::{spacing, typography};

/// Celda líder numerada de una fila: el número de posición normalmente;
/// si es la fila que está sonando, el ecualizador animado o el ícono de
/// play/pausa al pasar el mouse por encima de la fila.
fn leading_index_cell<'a, Message: 'a>(
    position: usize,
    is_playing_row: bool,
    is_playing: bool,
    icon_hovered: bool,
    size: Length,
) -> Element<'a, Message> {
    if !is_playing_row {
        return container(text(position.to_string()).font(SF_PRO).size(typography::TEXT_13).color(theme().content.muted))
            .width(size)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();
    }

    let show_toggle_icon = !is_playing || icon_hovered;

    let glyph: Element<'a, Message> = if show_toggle_icon {
        let icon_variant = if is_playing { Icon::Pause } else { Icon::Play };
        icons::icon(icon_variant, typography::TEXT_13)
            .color(theme().accent.primary)
            .into()
    } else {
        icons::glyph(Icon::equalizer_frame(), typography::TEXT_11).color(theme().accent.primary).into()
    };

    container(glyph)
        .width(size)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

const TITLE_WIDTH: Length = Length::FillPortion(3);
const ARTIST_WIDTH: Length = Length::FillPortion(2);
const ALBUM_WIDTH: Length = Length::FillPortion(2);
const DURATION_WIDTH: Length = Length::Fixed(56.0);
const CACHE_DOT_SIZE: f32 = 8.0;
const ROW_THUMBNAIL_SIZE: f32 = 40.0;
const NUMBERED_INDEX_WIDTH: f32 = 32.0;
/// Alto fijo de `track_row_numbered`.
pub const NUMBERED_ROW_HEIGHT: f32 = 58.0;
/// Alto fijo de `track_row_with_thumbnail`.
pub const THUMBNAIL_ROW_HEIGHT: f32 = 52.0;

/// Fila de track con número + thumbnail al inicio: Título / Artista /
/// Álbum / Duración + indicador de caché. Click izquierdo emite `on_click`
/// (o pausa/reanuda si ya es la fila que suena), click derecho abre el
/// menú contextual.
pub fn track_row_with_thumbnail<'a, Message: Clone + 'a, F: Fn(String) -> Message + 'a, G: Fn(String) -> Message + 'a>(
    position: usize,
    track: &'a Track,
    thumbnail: Option<Handle>,
    selection: RowSelectionShape,
    on_click: Message,
    on_right_click: Message,
    on_artist_click: F,
    on_album_click: G,
    is_playing_row: bool,
    is_playing: bool,
    icon_hovered: bool,
    on_toggle: Message,
    on_hover_enter: Message,
    on_hover_exit: Message,
) -> Element<'a, Message> {
    let index = leading_index_cell(position, is_playing_row, is_playing, icon_hovered, Length::Fixed(NUMBERED_INDEX_WIDTH));
    let thumb = track_thumbnail_sized(thumbnail, ROW_THUMBNAIL_SIZE);
    build_row(
        track,
        index,
        thumb,
        selection,
        on_click,
        on_right_click,
        on_artist_click,
        on_album_click,
        is_playing_row,
        on_toggle,
        on_hover_enter,
        on_hover_exit,
    )
}

fn build_row<'a, Message: Clone + 'a, F: Fn(String) -> Message + 'a, G: Fn(String) -> Message + 'a>(
    track: &'a Track,
    index: Element<'a, Message>,
    thumbnail: Element<'a, Message>,
    selection: RowSelectionShape,
    on_click: Message,
    on_right_click: Message,
    on_artist_click: F,
    on_album_click: G,
    is_playing_row: bool,
    on_toggle: Message,
    on_hover_enter: Message,
    on_hover_exit: Message,
) -> Element<'a, Message> {
    let title_color = if is_playing_row { theme().accent.primary } else { theme().content.primary };
    let title = title_with_download_mark(
        single_line_text(track.title.as_str(), SF_PRO, typography::TEXT_14, title_color, Length::Fill),
        track,
    )
        .width(TITLE_WIDTH);

    let artist = artist_links(
        &track.artists,
        SF_PRO,
        13.0,
        theme().content.muted,
        ARTIST_WIDTH,
        on_artist_click,
    );

    let album = album_link(
        track.album.as_ref(),
        SF_PRO,
        13.0,
        theme().content.muted,
        ALBUM_WIDTH,
        on_album_click,
    );

    let duration = text(format_duration(track.duration_seconds))
        .font(SF_PRO)
        .size(typography::TEXT_13)
        .color(theme().content.muted)
        .width(DURATION_WIDTH);

    let content = row![index, thumbnail, title, artist, album, duration]
        .spacing(spacing::SP_18)
        .align_y(Alignment::Center)
        .padding([spacing::SP_0, spacing::SP_8]);

    row_shell(
        content.into(),
        THUMBNAIL_ROW_HEIGHT,
        selection,
        if is_playing_row { on_toggle } else { on_click },
        on_right_click,
        is_playing_row.then_some((on_hover_enter, on_hover_exit)),
    )
}

/// Botón de alto fijo con hover + fondo de selección + click derecho, y
/// enter/exit opcionales (solo la fila que suena los usa).
fn row_shell<'a, Message: Clone + 'a>(
    content: Element<'a, Message>,
    height: f32,
    selection: RowSelectionShape,
    on_press: Message,
    on_right_click: Message,
    hover: Option<(Message, Message)>,
) -> Element<'a, Message> {
    let centered = container(content).height(Length::Fill).align_y(Alignment::Center);

    let btn = button(centered)
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .padding(spacing::SP_0)
        .style(|_theme: &Theme, status| {
            let background = match status {
                button::Status::Hovered => Some(theme().overlay.hover.into()),
                _ => None,
            };
            button::Style { background, text_color: theme().content.primary, ..Default::default() }
        })
        .on_press(on_press);

    // Con la forma según las vecinas, varias filas seleccionadas seguidas se ven como un solo bloque.
    let styled = container(btn).width(Length::Fill).style(row_style::selected(selection));

    let area = mouse_area(styled).on_right_press(on_right_click);

    match hover {
        Some((on_enter, on_exit)) => area.on_enter(on_enter).on_exit(on_exit).into(),
        None => area.into(),
    }
}

/// Fila con número de posición, título/artista apilados, indicador de caché y duración; con hover.
/// Click izquierdo emite `on_click` (o pausa/reanuda si ya es la fila que
/// suena), click derecho abre el menú contextual.
pub fn track_row_numbered<'a, Message: Clone + 'a, F: Fn(String) -> Message + 'a>(
    position: usize,
    track: &'a Track,
    selection: RowSelectionShape,
    on_click: Message,
    on_right_click: Message,
    on_artist_click: F,
    is_playing_row: bool,
    is_playing: bool,
    icon_hovered: bool,
    on_toggle: Message,
    on_hover_enter: Message,
    on_hover_exit: Message,
) -> Element<'a, Message> {
    let index = leading_index_cell(position, is_playing_row, is_playing, icon_hovered, Length::Fixed(NUMBERED_INDEX_WIDTH));

    let title_color = if is_playing_row { theme().accent.primary } else { theme().content.primary };
    let title = title_with_download_mark(
        single_line_text(track.title.as_str(), SF_PRO, typography::TEXT_14, title_color, Length::Fill),
        track,
    );
    let artist = artist_links(&track.artists, SF_PRO, typography::TEXT_13, theme().content.muted, Length::Fill, on_artist_click);
    let title_artist = column![title, artist].spacing(spacing::SP_2).width(Length::Fill);

    let duration = text(format_duration(track.duration_seconds))
        .font(SF_PRO)
        .size(typography::TEXT_13)
        .color(theme().content.muted)
        .width(DURATION_WIDTH);

    let content = row![index, title_artist, duration]
        .spacing(spacing::SP_18)
        .align_y(Alignment::Center)
        .padding(Padding { top: spacing::SP_0, right: spacing::SP_8, bottom: spacing::SP_0, left: spacing::SP_8 });

    row_shell(
        content.into(),
        NUMBERED_ROW_HEIGHT,
        selection,
        if is_playing_row { on_toggle } else { on_click },
        on_right_click,
        is_playing_row.then_some((on_hover_enter, on_hover_exit)),
    )
}

/// Ancho reservado para cada marca al final de un título (descargada, letra), para
/// que todos los títulos corten igual tengan o no la marca.
pub(crate) const MARK_SLOT_WIDTH: f32 = 16.0;

/// Si la canción está descargada: tiene archivo o el servidor la marca como en caché.
pub(crate) fn is_downloaded(track: &Track) -> bool {
    track.file_path.is_some() || matches!(track.state, TrackState::Cached)
}

/// Lugar fijo para una marca al final del título.
pub(crate) fn mark_slot<'a, Message: 'a>(mark: Option<Element<'a, Message>>) -> Element<'a, Message> {
    container(mark.unwrap_or_else(|| text("").into()))
        .width(Length::Fixed(MARK_SLOT_WIDTH))
        .align_x(Alignment::Center)
        .into()
}

/// Título seguido del punto de descargada, igual en mezclas, artista y álbum.
fn title_with_download_mark<'a, Message: 'a>(title: Element<'a, Message>, track: &Track) -> iced::widget::Row<'a, Message> {
    row![title, mark_slot(Some(cache_indicator(is_downloaded(track))))]
        .spacing(spacing::SP_8)
        .align_y(Alignment::Center)
}

/// Punto circular indicador de "en caché" según `track.state`.
pub(crate) fn cache_indicator<'a, Message: 'a>(cached: bool) -> Element<'a, Message> {
    let color = if cached {
        theme().status.cached
    } else {
        theme().content.faint
    };

    container(text(""))
        .width(Length::Fixed(CACHE_DOT_SIZE))
        .height(Length::Fixed(CACHE_DOT_SIZE))
        .style(move |_theme: &Theme| container::Style {
            background: Some(color.into()),
            border: rounded(CACHE_DOT_SIZE / 2.0),
            ..Default::default()
        })
        .into()
}
