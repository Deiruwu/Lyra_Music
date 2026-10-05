//! [playlist-color] Selector de color: un círculo con el color actual que, al
//! pulsarlo, despliega a su izquierda tres sliders (tono, saturación y brillo).
//! El cambio se ve en vivo mientras se arrastra; `on_release` avisa cuando hay
//! que persistirlo.

use iced::border::rounded;
use iced::widget::{button, column, container, row, slider, space, text};
use iced::{Alignment, Element, Length, Theme};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{spacing, typography};
use crate::ui::playlist_color::{self, PlaylistColor};
use crate::ui::styles::container as container_style;
use crate::ui::styles::slider as slider_style;
use crate::ui::theme::theme;

const SWATCH_SIZE: f32 = 26.0;
const SLIDER_WIDTH: f32 = 180.0;
const LABEL_WIDTH: f32 = 72.0;

pub fn color_picker<'a, Message: Clone + 'a>(
    color: PlaylistColor,
    is_open: bool,
    on_toggle: Message,
    on_change: impl Fn(PlaylistColor) -> Message + Clone + 'a,
    on_release: Message,
) -> Element<'a, Message> {
    let fill = playlist_color::swatch(color);
    let swatch = button(space().width(Length::Fixed(SWATCH_SIZE)).height(Length::Fixed(SWATCH_SIZE)))
        .padding(spacing::SP_0)
        .style(move |_theme: &Theme, status| button::Style {
            background: Some(fill.into()),
            border: rounded(SWATCH_SIZE / 2.0)
                .color(if status == button::Status::Hovered { theme().content.primary } else { theme().border.subtle })
                .width(2.0),
            ..Default::default()
        })
        .on_press(on_toggle);

    if !is_open {
        return swatch.into();
    }

    let channel = |label: &'static str, range: std::ops::RangeInclusive<f32>, value: f32, step: f32, apply: fn(PlaylistColor, f32) -> PlaylistColor| {
        let on_change = on_change.clone();
        row![
            text(label).font(SF_PRO).size(typography::TEXT_12).color(theme().content.secondary).width(Length::Fixed(LABEL_WIDTH)),
            slider(range, value, move |v| on_change(apply(color, v)))
                .step(step)
                .on_release(on_release.clone())
                .width(Length::Fixed(SLIDER_WIDTH))
                .style(slider_style::track),
        ]
            .align_y(Alignment::Center)
    };

    let panel = container(
        column![
            channel("Tono", 0.0..=359.0, color.hue, 1.0, |c, v| PlaylistColor { hue: v, ..c }),
            channel("Saturación", 0.0..=1.0, color.saturation, 0.01, |c, v| PlaylistColor { saturation: v, ..c }),
            channel("Brillo", 0.0..=1.0, color.value, 0.01, |c, v| PlaylistColor { value: v, ..c }),
        ]
            .spacing(spacing::SP_8),
    )
        .padding(spacing::SP_12)
        .style(container_style::context_menu);

    row![panel, swatch].spacing(spacing::SP_12).align_y(Alignment::Start).into()
}
