use iced::widget::{button, container, row, space, stack, text};
use iced::{border, Alignment, Color, Element, Length};
use crate::ui::styles::styles::transparent_button;
use crate::JETBRAINS_MONO;

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

    /// Transforma la configuración del Builder en un Elemento renderizable de iced.
    pub fn build(self) -> Element<'a, Message> {
        let thumb = container(space().width(20).height(20))
            .style(|_theme| container::Style {
                background: Some(Color::WHITE.into()),
                border: border::rounded(10),
                ..Default::default()
            });

        // Capa de fondo (iconos fijos)
        let background_icons = row![
            space().width(2),
            text(self.icon_active).font(JETBRAINS_MONO).size(14).style(|_theme| text::Style {
                color: Option::from(Color::from_rgb(0.6, 0.6, 0.6)),
                ..Default::default()
            }),
            space().width(Length::Fill),
            text(self.icon_inactive).font(JETBRAINS_MONO).size(14).style(|_theme| text::Style {
                color: Option::from(Color::from_rgb(0.6, 0.6, 0.6)),
                ..Default::default()
            }),
            space().width(8),
        ]
            .align_y(Alignment::Center)
            .padding([0.0, 4.0]);

        // Capa dinámica (animación del pulgar)
        let animated_thumb = row![
            space().width(Length::Fixed(self.thumb_offset)),
            thumb
        ]
            .align_y(Alignment::Center);

        // Apilamos usando Z-index
        let track_content = stack![
            background_icons,
            animated_thumb,
        ];

        let track = container(track_content)
            .width(self.width)
            .height(28)
            .padding(4)
            .style(move |_theme| container::Style {
                background: Some(Color::from_rgb(0.2, 0.2, 0.2).into()),
                border: border::rounded(20),
                ..Default::default()
            });

        button(track)
            .padding(0)
            .style(transparent_button)
            .on_press((self.on_toggle)(!self.is_active))
            .into()
    }
}