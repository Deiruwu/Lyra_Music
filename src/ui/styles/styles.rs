use iced::{Color, Theme};
use iced::widget::button;

pub fn transparent_button(_theme: &Theme, status: button::Status) -> button::Style {
    let text_color = match status {
        button::Status::Disabled => Color::from_rgb(0.4, 0.4, 0.4),
        _ => Color::WHITE,
    };
    button::Style {
        background: None,
        text_color,
        ..Default::default()
    }
}