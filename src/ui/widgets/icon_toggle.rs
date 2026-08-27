use iced::widget::{button, container, row, space, stack, text};
use iced::{border, Alignment, Element, Length};
use crate::ui::assets::fonts::JETBRAINS_MONO;
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::styles::button as button_style;
use crate::ui::theme::theme;

pub struct IconToggle<'a, Message> {
    is_active: bool,
    thumb_offset: f32,
    icon_inactive: &'a str,
    icon_active: &'a str,
    width: f32,
    on_toggle: Box<dyn Fn(bool) -> Message + 'a>,
}

impl<'a, Message> IconToggle<'a, Message>
where
    Message: Clone + 'a
{
    pub fn new(
        is_active: bool,
        thumb_offset: f32,
        on_toggle: impl Fn(bool) -> Message + 'a,
    ) -> Self {
        Self {
            is_active,
            thumb_offset,
            icon_inactive: "",
            icon_active: "",
            width: 60.0,
            on_toggle: Box::new(on_toggle),
        }
    }

    pub fn icons(mut self, inactive: &'a str, active: &'a str) -> Self {
        self.icon_inactive = inactive;
        self.icon_active = active;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn build(self) -> Element<'a, Message> {
        let thumb = container(space().width(20).height(20))
            .style(|_theme| container::Style {
                background: Some(theme().content.primary.into()),
                border: border::rounded(radii::R_10),
                ..Default::default()
            });

        let background_icons = row![
            space().width(spacing::SP_2),
            text(self.icon_active).font(JETBRAINS_MONO).size(typography::TEXT_14).style(|_theme| text::Style {
                color: Option::from(theme().content.muted),
                ..Default::default()
            }),
            space().width(Length::Fill),
            text(self.icon_inactive).font(JETBRAINS_MONO).size(typography::TEXT_14).style(|_theme| text::Style {
                color: Option::from(theme().content.muted),
                ..Default::default()
            }),
            space().width(spacing::SP_8),
        ]
            .align_y(Alignment::Center)
            .padding([spacing::SP_0, spacing::SP_4]);

        let animated_thumb = row![
            space().width(Length::Fixed(self.thumb_offset)),
            thumb
        ]
            .align_y(Alignment::Center);

        let track_content = stack![
            background_icons,
            animated_thumb,
        ];

        let track = container(track_content)
            .width(self.width)
            .height(28)
            .padding(spacing::SP_4)
            .style(move |_theme| container::Style {
                background: Some(theme().surface.control.into()),
                border: border::rounded(radii::R_20),
                ..Default::default()
            });

        button(track)
            .padding(spacing::SP_0)
            .style(button_style::transparent)
            .on_press((self.on_toggle)(!self.is_active))
            .into()
    }
}