use iced::border::rounded;
use iced::widget::image::Handle;
use iced::widget::text::Shaping;
use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::model::{Track, TrackState};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::icons::Icon;
use crate::ui::styles::styles::NOW_PLAYING_ACCENT;
use crate::ui::widgets::artist_links::{album_link, artist_links};
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_row::track_thumbnail_sized;
use crate::utils::formatting::format_duration;
use crate::ui::assets::fonts::JETBRAINS_MONO;

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
        return container(text(position.to_string()).font(SF_PRO).size(13).color(Color::from_rgb(0.6, 0.6, 0.65)))
            .width(size)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();
    }

    let show_toggle_icon = !is_playing || icon_hovered;

    let glyph: Element<'a, Message> = if show_toggle_icon {
        let icon = if is_playing { Icon::Pause } else { Icon::Play };
        text(icon.as_str())
            .font(JETBRAINS_MONO)
            .shaping(Shaping::Advanced)
            .size(13)
            .color(NOW_PLAYING_ACCENT)
            .into()
    } else {
        text(Icon::equalizer_frame()).font(JETBRAINS_MONO).size(11).color(NOW_PLAYING_ACCENT).into()
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

/// Fila de track con número + thumbnail al inicio: Título / Artista /
/// Álbum / Duración + indicador de caché. Click izquierdo reproduce
/// (o pausa/reanuda si ya es la fila que suena), click derecho abre el
/// menú contextual.
pub fn track_row_with_thumbnail<'a, Message: Clone + 'a, F: Fn(String) -> Message + 'a, G: Fn(String) -> Message + 'a>(
    position: usize,
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_play: Message,
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
        on_play,
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
    on_play: Message,
    on_right_click: Message,
    on_artist_click: F,
    on_album_click: G,
    is_playing_row: bool,
    on_toggle: Message,
    on_hover_enter: Message,
    on_hover_exit: Message,
) -> Element<'a, Message> {
    let cached = matches!(track.state, TrackState::Cached);

    let title_color = if is_playing_row { NOW_PLAYING_ACCENT } else { Color::WHITE };
    let title = single_line_text(track.title.as_str(), SF_PRO, 14.0, title_color, TITLE_WIDTH);

    let artist = artist_links(
        &track.artists,
        SF_PRO,
        13.0,
        Color::from_rgb(0.7, 0.7, 0.75),
        ARTIST_WIDTH,
        on_artist_click,
    );

    let album = album_link(
        track.album.as_ref(),
        SF_PRO,
        13.0,
        Color::from_rgb(0.7, 0.7, 0.75),
        ALBUM_WIDTH,
        on_album_click,
    );

    let duration = text(format_duration(track.duration_seconds))
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.7, 0.7, 0.75))
        .width(DURATION_WIDTH);

    let content = row![index, thumbnail, title, artist, album, duration, cache_indicator(cached)]
        .spacing(18)
        .align_y(Alignment::Center)
        .padding([6, 8]);

    let btn = button(content)
        .padding(0)
        .style(|_theme: &Theme, status| {
            let background = match status {
                button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                _ => None,
            };
            button::Style { background, text_color: Color::WHITE, ..Default::default() }
        })
        .on_press(if is_playing_row { on_toggle } else { on_play });

    let area = mouse_area(btn).on_right_press(on_right_click);

    if is_playing_row {
        area.on_enter(on_hover_enter).on_exit(on_hover_exit).into()
    } else {
        area.into()
    }
}

/// Fila con número de posición, título/artista apilados, indicador de caché y duración; con hover.
/// Click izquierdo reproduce (o pausa/reanuda si ya es la fila que
/// suena), click derecho abre el menú contextual.
pub fn track_row_numbered<'a, Message: Clone + 'a, F: Fn(String) -> Message + 'a>(
    position: usize,
    track: &'a Track,
    on_play: Message,
    on_right_click: Message,
    on_artist_click: F,
    is_playing_row: bool,
    is_playing: bool,
    icon_hovered: bool,
    on_toggle: Message,
    on_hover_enter: Message,
    on_hover_exit: Message,
) -> Element<'a, Message> {
    let cached = matches!(track.state, TrackState::Cached);

    let index = leading_index_cell(position, is_playing_row, is_playing, icon_hovered, Length::Fixed(NUMBERED_INDEX_WIDTH));

    let title_color = if is_playing_row { NOW_PLAYING_ACCENT } else { Color::WHITE };
    let title = single_line_text(track.title.as_str(), SF_PRO, 14.0, title_color, Length::Fill);
    let artist = artist_links(&track.artists, SF_PRO, 13.0, Color::from_rgb(0.7, 0.7, 0.75), Length::Fill, on_artist_click);
    let title_artist = column![title, artist].spacing(2).width(Length::Fill);

    let duration = text(format_duration(track.duration_seconds))
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.7, 0.7, 0.75))
        .width(DURATION_WIDTH);

    let content = row![index, title_artist, cache_indicator(cached), duration]
        .spacing(18)
        .align_y(Alignment::Center)
        .padding(Padding { top: 10.0, right: 8.0, bottom: 10.0, left: 8.0 });

    let btn = button(content)
        .padding(0)
        .style(|_theme: &Theme, status| {
            let background = match status {
                button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                _ => None,
            };
            button::Style { background, text_color: Color::WHITE, ..Default::default() }
        })
        .on_press(if is_playing_row { on_toggle } else { on_play });

    let area = mouse_area(btn).on_right_press(on_right_click);

    if is_playing_row {
        area.on_enter(on_hover_enter).on_exit(on_hover_exit).into()
    } else {
        area.into()
    }
}

/// Punto circular indicador de "en caché" según `track.state`.
pub(crate) fn cache_indicator<'a, Message: 'a>(cached: bool) -> Element<'a, Message> {
    let color = if cached {
        Color::from_rgb(0.4, 0.85, 0.5)
    } else {
        Color::from_rgb(0.45, 0.45, 0.45)
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
