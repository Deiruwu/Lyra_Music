//! Selector del color de acento de la app (el violeta de fábrica), en Ajustes. De él se
//! derivan los demás tonos (ver `theme::set_accent`). El cambio se ve en vivo; se guarda
//! con el resto de los ajustes.

use std::f32::consts::PI;

use iced::border::rounded;
use iced::gradient::Linear;
use iced::widget::{button, column, container, mouse_area, pin, row, space, stack, text, text_input};
use iced::{Alignment, Color, Element, Length, Padding, Point, Theme};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::cover_palette::{hsv, to_hsv};
use crate::ui::styles::button as button_style;
use crate::ui::styles::text_input as text_input_style;
use crate::ui::theme::{self, theme};

const FIELD_WIDTH: f32 = 236.0;
const FIELD_HEIGHT: f32 = 140.0;
const HUE_HEIGHT: f32 = 12.0;
const HANDLE_SIZE: f32 = 14.0;
const HUE_HANDLE_WIDTH: f32 = 4.0;
const SWATCH_SIZE: f32 = 22.0;
const PREVIEW_SIZE: f32 = 30.0;

/// Atajos: el violeta de fábrica y tonos apagados que combinan con las superficies grises.
const PRESETS: [(u8, u8, u8); 6] = [
    (0x92, 0x83, 0x74), // arena
    (0xc9, 0x8a, 0x9f), // rosa viejo
    (0xd8, 0xa6, 0x57), // ámbar
    (0x8f, 0xb3, 0x9a), // salvia
    (0x6f, 0xb8, 0xb0), // turquesa
    (0x7f, 0xa8, 0xd8), // cielo
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Area {
    Field,
    Hue,
}

#[derive(Debug, Clone)]
pub enum AccentMessage {
    FieldMoved(Point),
    HueMoved(Point),
    Pressed(AreaPress),
    Released,
    HexChanged(String),
    Pick(Color),
}

/// En qué parte del selector se apretó.
#[derive(Debug, Clone, Copy)]
pub struct AreaPress(Area);

#[derive(Default)]
pub struct AccentPicker {
    hue: f32,
    saturation: f32,
    value: f32,
    /// Lo que muestra el campo `#rrggbb` (puede estar a medio escribir).
    hex: String,
    dragging: Option<Area>,
    /// Última posición del mouse en cada área, para aplicar al apretar.
    field_cursor: Point,
    hue_cursor: f32,
}

impl AccentPicker {
    /// Toma el acento vigente (al abrir Ajustes).
    pub fn sync_with_theme(&mut self) {
        self.dragging = None;
        self.load(theme().accent.primary);
    }

    pub fn update(&mut self, message: AccentMessage) {
        match message {
            AccentMessage::FieldMoved(position) => {
                self.field_cursor = position;
                if self.dragging == Some(Area::Field) {
                    self.apply_field();
                }
            }
            AccentMessage::HueMoved(position) => {
                self.hue_cursor = position.x;
                if self.dragging == Some(Area::Hue) {
                    self.apply_hue();
                }
            }
            AccentMessage::Pressed(AreaPress(area)) => {
                self.dragging = Some(area);
                match area {
                    Area::Field => self.apply_field(),
                    Area::Hue => self.apply_hue(),
                }
            }
            AccentMessage::Released => self.dragging = None,
            AccentMessage::HexChanged(hex) => {
                if let Some(color) = theme::from_hex(&hex) {
                    self.set_hsv_from(color);
                    theme::set_accent(color);
                }
                self.hex = hex;
            }
            AccentMessage::Pick(color) => {
                self.load(color);
                theme::set_accent(color);
            }
        }
    }

    fn load(&mut self, color: Color) {
        self.set_hsv_from(color);
        self.hex = theme::to_hex(color);
    }

    fn set_hsv_from(&mut self, color: Color) {
        let (hue, saturation, value) = to_hsv([color.r, color.g, color.b]);
        // Un gris no tiene tono: se conserva el que había para que la barra no salte.
        if saturation > 0.0 {
            self.hue = hue;
        }
        self.saturation = saturation;
        self.value = value;
    }

    fn apply_field(&mut self) {
        self.saturation = (self.field_cursor.x / FIELD_WIDTH).clamp(0.0, 1.0);
        self.value = 1.0 - (self.field_cursor.y / FIELD_HEIGHT).clamp(0.0, 1.0);
        self.apply();
    }

    fn apply_hue(&mut self) {
        self.hue = (self.hue_cursor / FIELD_WIDTH).clamp(0.0, 1.0) * 359.0;
        self.apply();
    }

    fn apply(&mut self) {
        let color = hsv(self.hue, self.saturation, self.value);
        self.hex = theme::to_hex(color);
        theme::set_accent(color);
    }

    /// Cuadro de color, tonos, atajos y `#rrggbb`, con "Restablecer" arriba.
    pub fn view(&self) -> Element<'_, AccentMessage> {
        let accent = theme().accent.primary;
        let is_default = theme::to_hex(accent) == theme::to_hex(theme::default_accent());
        let reset = button(text("Restablecer").font(SF_PRO).size(typography::TEXT_12).color(if is_default {
            theme().content.muted
        } else {
            theme().content.primary
        }))
            .padding(Padding { top: spacing::SP_4, bottom: spacing::SP_4, left: spacing::SP_10, right: spacing::SP_10 })
            .style(button_style::pill(false));
        let reset: Element<'_, AccentMessage> =
            if is_default { reset.into() } else { reset.on_press(AccentMessage::Pick(theme::default_accent())).into() };

        let title = row![space().width(Length::Fill), reset].align_y(Alignment::Center);

        let presets = std::iter::once(theme::default_accent())
            .chain(PRESETS.iter().map(|&(r, g, b)| Color::from_rgb8(r, g, b)))
            .map(|color| swatch(color, theme::to_hex(color) == theme::to_hex(accent)));

        let hex_row = row![
            container(space())
                .width(Length::Fixed(PREVIEW_SIZE))
                .height(Length::Fixed(PREVIEW_SIZE))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(accent.into()),
                    border: rounded(radii::R_8).color(theme().border.subtle).width(1.0),
                    ..Default::default()
                }),
            text_input("#rrggbb", &self.hex)
                .on_input(AccentMessage::HexChanged)
                .font(SF_PRO)
                .size(typography::TEXT_13)
                .padding(Padding { top: spacing::SP_6, bottom: spacing::SP_6, left: spacing::SP_10, right: spacing::SP_10 })
                .style(text_input_style::field)
                .width(Length::Fill),
        ]
            .spacing(spacing::SP_10)
            .align_y(Alignment::Center);

        column![
            title,
            self.view_field(),
            self.view_hue(),
            row(presets.collect::<Vec<_>>()).spacing(spacing::SP_10),
            hex_row,
        ]
            .spacing(spacing::SP_12)
            .width(Length::Fixed(FIELD_WIDTH))
            .into()
    }

    /// Cuadro de saturación (→) y brillo (↑) del tono elegido.
    fn view_field(&self) -> Element<'_, AccentMessage> {
        let pure = hsv(self.hue, 1.0, 1.0);
        let saturation_layer = gradient_box(Linear::new(PI * 0.5).add_stop(0.0, Color::WHITE).add_stop(1.0, pure), FIELD_HEIGHT);
        let value_layer = gradient_box(
            Linear::new(PI).add_stop(0.0, Color { a: 0.0, ..Color::BLACK }).add_stop(1.0, Color::BLACK),
            FIELD_HEIGHT,
        );

        let current = hsv(self.hue, self.saturation, self.value);
        let handle = container(space())
            .width(Length::Fixed(HANDLE_SIZE))
            .height(Length::Fixed(HANDLE_SIZE))
            .style(move |_theme: &Theme| container::Style {
                background: Some(current.into()),
                border: rounded(HANDLE_SIZE / 2.0).color(Color::WHITE).width(2.0),
                ..Default::default()
            });
        let x = self.saturation * FIELD_WIDTH - HANDLE_SIZE / 2.0;
        let y = (1.0 - self.value) * FIELD_HEIGHT - HANDLE_SIZE / 2.0;

        let field = stack![saturation_layer, value_layer, pin(handle).x(x).y(y)]
            .width(Length::Fixed(FIELD_WIDTH))
            .height(Length::Fixed(FIELD_HEIGHT));

        mouse_area(field)
            .on_move(AccentMessage::FieldMoved)
            .on_press(AccentMessage::Pressed(AreaPress(Area::Field)))
            .on_release(AccentMessage::Released)
            .on_exit(AccentMessage::Released)
            .interaction(iced::mouse::Interaction::Crosshair)
            .into()
    }

    /// Barra de tonos.
    fn view_hue(&self) -> Element<'_, AccentMessage> {
        let rainbow = (0..=6).fold(Linear::new(PI * 0.5), |gradient, step| {
            gradient.add_stop(step as f32 / 6.0, hsv(step as f32 * 60.0, 1.0, 1.0))
        });
        let handle = container(space())
            .width(Length::Fixed(HUE_HANDLE_WIDTH))
            .height(Length::Fixed(HUE_HEIGHT + 4.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(Color::WHITE.into()),
                border: rounded(radii::R_5).color(Color { a: 0.4, ..Color::BLACK }).width(1.0),
                ..Default::default()
            });
        let x = self.hue / 359.0 * FIELD_WIDTH - HUE_HANDLE_WIDTH / 2.0;

        let bar = stack![
            container(gradient_box(rainbow, HUE_HEIGHT)).padding(Padding { top: 2.0, ..Padding::ZERO }),
            pin(handle).x(x).y(0.0),
        ]
            .width(Length::Fixed(FIELD_WIDTH))
            .height(Length::Fixed(HUE_HEIGHT + 4.0));

        mouse_area(bar)
            .on_move(AccentMessage::HueMoved)
            .on_press(AccentMessage::Pressed(AreaPress(Area::Hue)))
            .on_release(AccentMessage::Released)
            .on_exit(AccentMessage::Released)
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    }
}

fn gradient_box<'a>(gradient: Linear, height: f32) -> Element<'a, AccentMessage> {
    container(space())
        .width(Length::Fixed(FIELD_WIDTH))
        .height(Length::Fixed(height))
        .style(move |_theme: &Theme| container::Style {
            background: Some(gradient.into()),
            border: rounded(radii::R_8),
            ..Default::default()
        })
        .into()
}

/// Círculo de un atajo; con borde claro si es el acento actual.
fn swatch<'a>(color: Color, is_current: bool) -> Element<'a, AccentMessage> {
    button(space().width(Length::Fixed(SWATCH_SIZE)).height(Length::Fixed(SWATCH_SIZE)))
        .padding(spacing::SP_0)
        .style(move |_theme: &Theme, status| button::Style {
            background: Some(color.into()),
            border: rounded(SWATCH_SIZE / 2.0)
                .color(if is_current || status == button::Status::Hovered { theme().content.primary } else { Color::TRANSPARENT })
                .width(2.0),
            ..Default::default()
        })
        .on_press(AccentMessage::Pick(color))
        .into()
}
