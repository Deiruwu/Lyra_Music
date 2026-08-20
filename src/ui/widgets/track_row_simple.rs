use iced::border::rounded;
use iced::widget::image::Handle;
use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::model::{Track, TrackState};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::widgets::track_row::track_thumbnail_sized;
use crate::utils::formatting::format_duration;

const TITLE_WIDTH: Length = Length::FillPortion(3);
const ARTIST_WIDTH: Length = Length::FillPortion(2);
const ALBUM_WIDTH: Length = Length::FillPortion(2);
const DURATION_WIDTH: Length = Length::Fixed(56.0);
const CACHE_DOT_SIZE: f32 = 8.0;
const ROW_THUMBNAIL_SIZE: f32 = 40.0;
const NUMBERED_INDEX_WIDTH: f32 = 32.0;

/// Fila de track con thumbnail al inicio: Título / Artista / Álbum / Duración + indicador de caché.
/// Click izquierdo reproduce, click derecho abre el menú contextual.
pub fn track_row_with_thumbnail<'a, Message: Clone + 'a>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_play: Message,
    on_right_click: Message,
) -> Element<'a, Message> {
    build_row(track, Some(track_thumbnail_sized(thumbnail, ROW_THUMBNAIL_SIZE)), on_play, on_right_click)
}

fn build_row<'a, Message: Clone + 'a>(
    track: &'a Track,
    thumbnail: Option<Element<'a, Message>>,
    on_play: Message,
    on_right_click: Message,
) -> Element<'a, Message> {
    let cached = matches!(track.state, TrackState::Cached);

    let title = text(track.title.as_str())
        .font(SF_PRO)
        .size(14)
        .color(Color::WHITE)
        .width(TITLE_WIDTH);

    let artist = text(track.format_artists())
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.7, 0.7, 0.75))
        .width(ARTIST_WIDTH);

    let album = text(track.album.as_ref().map(|a| a.name.as_str()).unwrap_or(""))
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.7, 0.7, 0.75))
        .width(ALBUM_WIDTH);

    let duration = text(format_duration(track.duration_seconds))
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.7, 0.7, 0.75))
        .width(DURATION_WIDTH);

    let mut content = row![].spacing(12).align_y(Alignment::Center).padding([6, 8]);

    if let Some(thumbnail) = thumbnail {
        content = content.push(thumbnail);
    }

    let content = content
        .push(title)
        .push(artist)
        .push(album)
        .push(duration)
        .push(cache_indicator(cached));

    let btn = button(content)
        .padding(0)
        .style(|_theme: &Theme, status| {
            let background = match status {
                button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                _ => None,
            };
            button::Style { background, text_color: Color::WHITE, ..Default::default() }
        })
        .on_press(on_play);

    mouse_area(btn).on_right_press(on_right_click).into()
}

/// Fila con número de posición, título/artista apilados, indicador de caché y duración; con hover.
/// Click izquierdo reproduce, click derecho abre el menú contextual.
pub fn track_row_numbered<'a, Message: Clone + 'a>(
    position: usize,
    track: &'a Track,
    on_play: Message,
    on_right_click: Message,
) -> Element<'a, Message> {
    let cached = matches!(track.state, TrackState::Cached);

    let index = container(text(position.to_string()).font(SF_PRO).size(13).color(Color::from_rgb(0.6, 0.6, 0.65)))
        .width(Length::Fixed(NUMBERED_INDEX_WIDTH))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    let title = text(track.title.as_str()).font(SF_PRO).size(14).color(Color::WHITE);
    let artist = text(track.format_artists()).font(SF_PRO).size(13).color(Color::from_rgb(0.7, 0.7, 0.75));
    let title_artist = column![title, artist].spacing(2).width(Length::Fill);

    let duration = text(format_duration(track.duration_seconds))
        .font(SF_PRO)
        .size(13)
        .color(Color::from_rgb(0.7, 0.7, 0.75))
        .width(DURATION_WIDTH);

    let content = row![index, title_artist, cache_indicator(cached), duration]
        .spacing(12)
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
        .on_press(on_play);

    mouse_area(btn).on_right_press(on_right_click).into()
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
