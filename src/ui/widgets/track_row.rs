use iced::{Alignment, Element, Length, Theme};
use iced::widget::{button, column, container, image, row, space, text, stack, mouse_area};
use iced::widget::image::Handle;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};
use crate::ui::styles::styles::transparent_button;
use crate::JETBRAINS_MONO;

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
    let artists = track.format_artists();
    let (title_color, artist_color) = if is_downloaded {
        (iced::Color::WHITE, iced::Color::from_rgb(0.6, 0.6, 0.6))
    } else {
        (iced::Color::from_rgb(0.4, 0.4, 0.4), iced::Color::from_rgb(0.3, 0.3, 0.3))
    };

    column![
        text(&track.title)
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
    thumbnail: Option<Handle>
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    row![
        track_thumbnail(thumbnail),
        track_info(track)
    ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
}

pub fn track_row<'a, Message>(
    track: &'a Track,
    thumbnail: Option<Handle>,
    on_press: Message
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
    thumbnail: Option<Handle>
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    container(basic_track_view(track, thumbnail))
        .padding(5)
        .into()
}

fn thumbnail_with_play_hover<'a, Message>(
    thumbnail: Option<Handle>,
    on_play: Message,
    hovered: bool,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    match thumbnail {
        Some(handle) => {
            let img = image(handle)
                .width(Length::Fixed(50.0))
                .height(Length::Fixed(50.0));

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
                stack![img, play_btn].into()
            } else {
                img.into()
            }
        }
        None => container(space().width(Length::Fixed(50.0)).height(Length::Fixed(50.0)))
            .width(Length::Fixed(50.0))
            .height(Length::Fixed(50.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Color::from_rgb(0.2, 0.2, 0.2).into()),
                border: iced::border::rounded(5),
                ..Default::default()
            })
            .into(),
    }
}

pub fn queue_track_row<'a, Message>(
    track: &'a PlayableTrack,
    thumbnail: Option<Handle>,
    on_play: Message,
    on_delete: Message,
    row_hovered: bool,
    on_hover: Message,
    on_leave: Message,
    delete_hovered: bool,
    on_delete_hover: Message,
    on_delete_leave: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let thumb = thumbnail_with_play_hover(thumbnail, on_play, row_hovered);
    let info = track_info(&track.track);

    let delete_button = mouse_area(
        button(
            container(
                text(if delete_hovered { "󰛌" } else { "󰆴" }).font(JETBRAINS_MONO).size(16)
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