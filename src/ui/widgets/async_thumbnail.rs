use iced::{Element, Length};
use iced::widget::{container, image, text};
use iced::widget::image::Handle;

pub enum ThumbnailState {
    Loading,
    Loaded(Handle),
}

pub fn async_thumbnail<'a, Message>(
    state: ThumbnailState,
    size: f32,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    match state {
        ThumbnailState::Loaded(handle) => image(handle)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .border_radius(5)
            .into(),

        ThumbnailState::Loading => container(
            text("󰋩").size(size * 0.4)
        )
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(iced::Color::from_rgb(0.18, 0.18, 0.18).into()),
                border: iced::border::rounded(5),
                ..Default::default()
            })
            .into(),
    }
}