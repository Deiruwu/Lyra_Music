use iced::{Element, Length, ContentFit};
use iced::widget::{container, image, text};
use iced::widget::image::Handle;

pub enum ThumbnailState {
    Loading,
    Loaded(Handle),
}

pub fn async_thumbnail<'a, Message>(
    state: ThumbnailState,
    size: f32,
    radius: f32,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    match state {
        ThumbnailState::Loaded(handle) => image(handle)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .content_fit(ContentFit::Cover)
            .border_radius(radius)
            .into(),

        ThumbnailState::Loading => container(
            text("󰋩").size(size * 0.4)
        )
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Color::from_rgb(0.18, 0.18, 0.18).into()),
                border: iced::border::rounded(radius),
                ..Default::default()
            })
            .into(),
    }
}