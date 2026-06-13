use iced::{Alignment, Element, Length};
use iced::widget::{column, container, image, row, text, Space};
use iced::widget::image::Handle;
use crate::model::Track;
use crate::ui::styles::styles::transparent_button;

pub fn _track<'a, Message>(track: &'a Track, thumbnail: Option<&'a Handle>) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let is_downloaded = track.file_path.as_ref().map_or(false, |p| !p.is_empty());

    let thumbnail_widget: Element<_> = match thumbnail {
        Some(handle) => image(handle.clone())
            .width(Length::Fixed(50.0))
            .height(Length::Fixed(50.0))
            .border_radius(5)
            .into(),
        None => container(Space::new())
            .width(Length::Fixed(50.0))
            .height(Length::Fixed(50.0))
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(iced::Color::from_rgb(0.2, 0.2, 0.2).into()),
                ..Default::default()
            })
            .into(),
    };

    let artists = track.format_artists();

    let (title_color, artist_color) = if is_downloaded {
        (iced::Color::WHITE, iced::Color::from_rgb(0.6, 0.6, 0.6))
    } else {
        (iced::Color::from_rgb(0.4, 0.4, 0.4), iced::Color::from_rgb(0.3, 0.3, 0.3))
    };

    let info = column![
        text(&track.title).size(14).color(title_color),
        text(artists).size(12).color(artist_color),
    ]
        .spacing(4);

    row![thumbnail_widget, info]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
}

pub fn track_row<'a, Message>(track: &'a Track, thumbnail: Option<&'a Handle>, on_press: Message) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    iced::widget::button(
        _track(track, thumbnail)
    )
        .width(Length::Fill)
        .on_press(on_press)
        .style(transparent_button)
        .into()
}

pub fn currently_playing_row<'a, Message>(track: &'a Track, thumbnail: Option<&'a Handle>) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    iced::widget::container(
        _track(track, thumbnail)
    )
        .padding(5)
        .into()
}