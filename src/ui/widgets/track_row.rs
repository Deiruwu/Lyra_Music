use iced::{Alignment, Element, Length, Theme};
use iced::widget::{button, column, container, image, row, space, text, stack, mouse_area};
use iced::widget::image::Handle;
use crate::model::Track;
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};
use crate::ui::styles::styles::transparent_button;
use crate::JETBRAINS_MONO;

const SPINNER: [&str; 6] = ["", "", "", "", "", ""];

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

pub fn track_thumbnail<'a, Message>(thumbnail: Option<Handle>) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let state = match thumbnail {
        Some(handle) => ThumbnailState::Loaded(handle),
        None => ThumbnailState::Loading,
    };
    async_thumbnail(state, 50.0)
}

pub fn track_info<'a, Message>(track: &'a Track) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let is_downloaded = track.file_path.as_ref().map_or(false, |p| !p.is_empty());
    let title   = truncate(&track.title, 28);
    let artists = truncate(&track.format_artists(), 28);

    let (title_color, artist_color) = if is_downloaded {
        (iced::Color::WHITE, iced::Color::from_rgb(0.6, 0.6, 0.6))
    } else {
        (iced::Color::from_rgb(0.4, 0.4, 0.4), iced::Color::from_rgb(0.3, 0.3, 0.3))
    };

    column![
        text(title)
            .size(14)
            .color(title_color)
            .width(Length::Fixed(180.0)),
        text(artists)
            .size(12)
            .color(artist_color)
            .width(Length::Fixed(180.0)),
    ]
        .spacing(4)
        .into()
}

pub fn basic_track_view<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    row![track_thumbnail(thumbnail), track_info(track)]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
}

pub fn track_row<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_press: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    button(basic_track_view(track, thumbnail))
        .width(Length::Fill)
        .on_press(on_press)
        .style(transparent_button)
        .into()
}

pub fn currently_playing_row<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    container(basic_track_view(track, thumbnail))
        .padding(5)
        .into()
}

// ── Thumbnail con overlay ─────────────────────────────────────────────────────

/// Estado visual del thumbnail en la queue.
pub enum QueueThumbnailState {
    /// Reproduciendo o en pausa — muestra icono play al hover.
    Normal,
    /// El DownloadWorker está bajando este track — muestra spinner animado.
    Downloading(u8),
}

fn thumbnail_with_overlay<'a, Message>(
    thumbnail: Option<Handle>,
    on_play: Message,
    hovered: bool,
    state: QueueThumbnailState,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let base: Element<'a, Message> = match thumbnail {
        Some(handle) => image(handle)
            .width(Length::Fixed(50.0))
            .height(Length::Fixed(50.0))
            .into(),
        None => container(space().width(Length::Fixed(50.0)).height(Length::Fixed(50.0)))
            .width(Length::Fixed(50.0))
            .height(Length::Fixed(50.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Color::from_rgb(0.2, 0.2, 0.2).into()),
                border: iced::border::rounded(5),
                ..Default::default()
            })
            .into(),
    };

    match state {
        // Spinner de descarga — siempre visible, no clickeable.
        QueueThumbnailState::Downloading(frame) => {
            let spinner_char = SPINNER[frame as usize % 6];
            let overlay = container(
                text(spinner_char).font(JETBRAINS_MONO).size(20).color(iced::Color::WHITE)
            )
                .width(Length::Fixed(50.0))
                .height(Length::Fixed(50.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_: &Theme| container::Style {
                    background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                    ..Default::default()
                });

            stack![base, overlay].into()
        }

        // Normal — play icon al hover.
        QueueThumbnailState::Normal => {
            if hovered {
                let play_btn = button(
                    container(
                        text("").font(JETBRAINS_MONO).size(18).color(iced::Color::WHITE)
                    )
                        .width(Length::Fixed(50.0))
                        .height(Length::Fixed(50.0))
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center)
                )
                    .on_press(on_play)
                    .width(Length::Fixed(50.0))
                    .height(Length::Fixed(50.0))
                    .padding(0)
                    .style(|_: &Theme, _| button::Style {
                        background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                        ..Default::default()
                    });

                stack![base, play_btn].into()
            } else {
                base
            }
        }
    }
}

pub fn queue_track_row<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_play: Message,
    on_delete: Message,
    row_hovered: bool,
    on_hover: Message,
    on_leave: Message,
    delete_hovered: bool,
    on_delete_hover: Message,
    on_delete_leave: Message,
    queue_state: QueueThumbnailState,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let thumb = thumbnail_with_overlay(thumbnail, on_play, row_hovered, queue_state);
    let info  = track_info(track);

    let delete_button = mouse_area(
        button(
            container(
                text(if delete_hovered { "󰛌" } else { "󰆴" })
                    .font(JETBRAINS_MONO)
                    .size(16)
            )
                .width(Length::Fixed(44.0))
                .height(Length::Fixed(44.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        )
            .on_press(on_delete)
            .style(transparent_button)
            .padding(0)
    )
        .on_enter(on_delete_hover)
        .on_exit(on_delete_leave);

    let row_content = mouse_area(
        row![thumb, info, space().width(Length::Fill)]
            .spacing(15)
            .align_y(Alignment::Center)
            .padding([8, 12])
    )
        .on_enter(on_hover)
        .on_exit(on_leave);

    row![row_content, delete_button]
        .align_y(Alignment::Center)
        .into()
}