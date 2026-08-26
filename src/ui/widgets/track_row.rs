use iced::{Alignment, ContentFit, Element, Font, Length, Theme};
use iced::widget::{button, column, container, image, mouse_area, row, space, stack, text};
use iced::widget::image::{Handle};
use crate::model::Track;
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};
use crate::ui::widgets::artist_links::{album_link, artist_links, artist_names_text};
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::styles::button as button_style;
use crate::ui::theme::theme;
use crate::ui::assets::fonts::JETBRAINS_MONO;
use crate::ui::assets::icons::Icon;

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

pub fn track_thumbnail_sized<'a, Message>(thumbnail: Option<Handle>, size: f32) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let state = match thumbnail {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    async_thumbnail(state, size, 6.0)
}

pub fn track_thumbnail<'a, Message>(thumbnail: Option<Handle>) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    track_thumbnail_sized(thumbnail, 60.0)
}

/// Fila de track sin navegación (usada por el dropdown de búsqueda: acá
/// clickear la fila descarga/reproduce, no debe haber otro camino de
/// navegación posible desde adentro).
pub fn track_info<'a, Message: Clone + 'a>(track: &'a Track, width: Length) -> Element<'a, Message> {
    let is_downloaded = track.file_path.as_ref().map_or(false, |p| !p.is_empty());

    let album_name = track.album.as_ref().map_or("".to_string(), |album| album.name.clone());

    let (title_color, artist_color) = if is_downloaded {
        (theme().content.primary, theme().content.tertiary_alt)
    } else {
        (theme().content.disabled_alt, theme().content.disabled_alt2)
    };

    column![
        single_line_text(&track.title, Font::default(), 14.0, title_color, width),
        artist_names_text(&track.artists, Font::default(), 11.0, artist_color, width),
        single_line_text(album_name, Font::default(), 11.0, artist_color, width),
    ]
        .align_x(Alignment::Start)
        .into()
}

pub fn basic_track_view<'a, Message: Clone + 'a>(
    track: &'a Track,
    thumbnail: Option<Handle>,
) -> Element<'a, Message> {
    row![
        track_thumbnail(thumbnail),
        track_info(track, Length::Fixed(260.0))
    ]
        .spacing(spacing::SP_10)
        .align_y(Alignment::Center)
        .into()
}

pub fn track_row<'a, Message: Clone + 'a>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_press: Message,
) -> Element<'a, Message> {
    button(basic_track_view(track, thumbnail))
        .width(Length::Fill)
        .on_press(on_press)
        .style(button_style::transparent)
        .into()
}

// ── Thumbnail con overlay ─────────────────────────────────────────────────────
//
// Nota: antes existía `QueueThumbnailState::Downloading(u8)`, que pintaba
// un SPINNER sobre la miniatura cuando la canción aún se estaba descargando
// pero ya aparecía en la cola. Se eliminó junto con su flujo (ver el
// comentario "FEAT FUTURO: canción en descarga visible en la cola" en
// queue_panel.rs) para reimplementarse como feature más adelante.

fn thumbnail_with_overlay<'a, Message>(
    thumbnail: Option<Handle>,
    on_play: Message,
    hovered: bool,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let base: Element<'a, Message> = match thumbnail {
        Some(handle) => image(handle)
            .width(Length::Fixed(55.0))
            .height(Length::Fixed(55.0))
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(space().width(Length::Fixed(55.0)).height(Length::Fixed(55.0)))
            .width(Length::Fixed(50.0))
            .height(Length::Fixed(50.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(theme().surface.control.into()),
                border: iced::border::rounded(radii::R_5),
                ..Default::default()
            })
            .into(),
    };

    if hovered {
        let play_btn = button(
            container(
                text(Icon::Play.as_ref()).font(JETBRAINS_MONO).size(typography::TEXT_18).color(theme().content.primary)
            )
                .width(Length::Fixed(55.0))
                .height(Length::Fixed(55.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
            .on_press(on_play)
            .width(Length::Fixed(55.0))
            .height(Length::Fixed(55.0))
            .padding(spacing::SP_0)
            .style(|_: &Theme, _| button::Style {
                background: Some(theme().overlay.scrim_play.into()),
                ..Default::default()
            });

        stack![base, play_btn].into()
    } else {
        base
    }
}

fn drag_handle<'a, Message>(
    on_drag_start: Message,
    on_drag_release: Message,
    is_dragging: bool,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let icon_color = if is_dragging {
        theme().content.primary
    } else {
        theme().content.faint_alt
    };

    let handle = container(
        text("")
            .font(JETBRAINS_MONO)
            .size(typography::TEXT_16)
            .color(icon_color)
    )
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(44.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    mouse_area(handle)
        .on_press(on_drag_start)
        .on_release(on_drag_release)
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

pub fn queue_track_row<'a, Message, F, G>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_play: Message,
    on_delete: Message,
    row_hovered: bool,
    delete_hovered: bool,
    on_delete_hover: Message,
    on_delete_leave: Message,
    drag: DragRowParams<Message>,
    on_artist_click: F,
    on_album_click: G,
    on_right_click: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
    F: Fn(String) -> Message + 'a,
    G: Fn(String) -> Message + 'a,
{
    let thumb = thumbnail_with_overlay(thumbnail, on_play, row_hovered);

    let is_downloaded = track.file_path.as_ref().map_or(false, |p| !p.is_empty());
    let (title_color, artist_color) = if is_downloaded {
        (theme().content.primary, theme().content.tertiary_alt)
    } else {
        (theme().content.disabled_alt, theme().content.disabled_alt2)
    };

    let info = column![
        single_line_text(&track.title, Font::default(), 14.0, title_color, Length::Fill),
        artist_links(&track.artists, Font::default(), 11.0, artist_color, Length::Fill, on_artist_click),
        album_link(track.album.as_ref(), Font::default(), 11.0, artist_color, Length::Fill, on_album_click),
    ]
        .align_x(Alignment::Start);

    let delete_button = mouse_area(
        button(
            container(
                text(if delete_hovered { "󰛌" } else { "󰆴" })
                    .font(JETBRAINS_MONO)
                    .size(typography::TEXT_16)
            )
                .width(Length::Fixed(44.0))
                .height(Length::Fixed(44.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
            .on_press(on_delete)
            .style(button_style::transparent)
            .padding(spacing::SP_0)
    )
        .on_enter(on_delete_hover)
        .on_exit(on_delete_leave);

    let handle = drag_handle(drag.on_drag_start, drag.on_drag_release, drag.is_dragging);

    let row_content = mouse_area(
        row![handle, thumb, info]
            .spacing(spacing::SP_15)
            .align_y(Alignment::Center)
            .padding([spacing::SP_8, spacing::SP_12])
            .width(Length::Fill)
    )
        .on_right_press(on_right_click);

    let content = row![row_content, delete_button]
        .align_y(Alignment::Center)
        .width(Length::Fill);

    container(content)
        .width(Length::Fill)
        .style(move |_theme: &Theme| {
            if drag.is_dragging {
                container::Style {
                    background: Some(theme().overlay.hover_row.into()),
                    border: iced::border::rounded(radii::R_6)
                        .color(theme().border.drag)
                        .width(1.0),
                    ..Default::default()
                }
            } else {
                container::Style::default()
            }
        })
        .into()
}

pub struct DragRowParams<Message> {
    pub is_dragging: bool,
    pub on_drag_start: Message,
    pub on_drag_release: Message,
}