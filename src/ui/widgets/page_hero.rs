//! Héroe de las vistas de detalle: portada + título + metadatos + botón de
//! reproducir sobre una banda con degradado teñido por la propia portada.
//!
//! Widget presentacional: recibe el `Handle` de la portada ya resuelto por
//! la vista y no descarga nada.

use iced::widget::image::Handle;
use iced::widget::{button, container, row, space, stack, text, Column};
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::ui::assets::icons::{self, Icon};
use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{radii, spacing, typography};
use crate::ui::styles::button as button_style;
use crate::ui::theme::theme;
use crate::ui::widgets::async_thumbnail::{async_thumbnail, ThumbnailState};

const COVER_SIZE: f32 = 200.0;
const COVER_RADIUS: f32 = radii::R_12;
const PLAY_BUTTON_SIZE: f32 = 52.0;
/// Alto de la banda desplegada; fijo para que el contenido de debajo no se
/// mueva mientras la portada carga.
pub const HERO_HEIGHT: f32 = 260.0;

/// Margen lateral de página. El héroe sangra a los bordes del panel central,
/// así que lo aplica cada vista al contenido que va debajo, no el shell.
pub const PAGE_INSET: f32 = spacing::SP_20;

/// Envuelve el contenido de una vista en el margen de página.
pub fn page_inset<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content.into())
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(PAGE_INSET)
        .into()
}

/// Constructor encadenable del héroe. Sólo `title` y la portada son
/// obligatorios; el resto de piezas se omiten si no se declaran.
pub struct PageHero<'a, Message> {
    title: &'a str,
    cover: Option<Handle>,
    kicker: Option<String>,
    subtitle: Option<Element<'a, Message>>,
    meta: Vec<String>,
    on_cover_click: Option<Message>,
    play: Option<(Message, bool)>,
    tint: Option<Color>,
    collapse: f32,
}

impl<'a, Message: Clone + 'a> PageHero<'a, Message> {
    pub fn new(title: &'a str, cover: Option<Handle>) -> Self {
        Self {
            title,
            cover,
            kicker: None,
            subtitle: None,
            meta: Vec::new(),
            on_cover_click: None,
            play: None,
            tint: None,
            collapse: 0.0,
        }
    }

    /// Etiqueta pequeña sobre el título, p. ej. "PLAYLIST" o "ÁLBUM".
    pub fn kicker(mut self, kicker: impl Into<String>) -> Self {
        self.kicker = Some(kicker.into());
        self
    }

    /// Línea bajo el título, para contenido con enlaces (créditos de artista).
    pub fn subtitle(mut self, subtitle: Element<'a, Message>) -> Self {
        self.subtitle = Some(subtitle);
        self
    }

    /// Añade una línea de metadatos. Acumulativo.
    pub fn meta(mut self, line: impl Into<String>) -> Self {
        self.meta.push(line.into());
        self
    }

    /// Envuelve la portada en un overlay de "cambiar portada".
    pub fn on_cover_click(mut self, message: Message) -> Self {
        self.on_cover_click = Some(message);
        self
    }

    /// Botón de reproducir; `is_playing` decide el glifo (▶/⏸).
    pub fn play(mut self, on_press: Message, is_playing: bool) -> Self {
        self.play = Some((on_press, is_playing));
        self
    }

    /// Color dominante de la portada; tiñe el arranque de la banda.
    pub fn tint(mut self, tint: Option<Color>) -> Self {
        self.tint = tint;
        self
    }

    /// Píxeles que la banda se repliega, normalmente el offset del scroll de
    /// la vista. En `HERO_HEIGHT` desaparece del todo.
    pub fn collapse(mut self, offset: f32) -> Self {
        self.collapse = offset.max(0.0);
        self
    }

    pub fn build(self) -> Element<'a, Message> {
        let tint = self.tint;
        let height = (HERO_HEIGHT - self.collapse).max(0.0);

        let content = row![self.view_cover(), self.view_info()]
            .spacing(spacing::SP_24)
            .align_y(Alignment::End);

        container(content)
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .align_y(Alignment::End)
            .padding(Padding {
                top: spacing::SP_0,
                bottom: spacing::SP_28,
                left: PAGE_INSET,
                right: PAGE_INSET,
            })
            .clip(true)
            .style(move |_theme: &Theme| container::Style {
                background: Some(
                    iced::gradient::Linear::new(std::f32::consts::PI)
                        .add_stop(0.0, tint.unwrap_or(theme().surface.sunken))
                        .add_stop(1.0, theme().surface.panel)
                        .into(),
                ),
                ..Default::default()
            })
            .into()
    }

    fn view_cover(&self) -> Element<'a, Message> {
        let state = match self.cover.clone() {
            Some(handle) => ThumbnailState::Loaded(handle),
            None => ThumbnailState::Loading,
        };
        let cover = async_thumbnail(state, COVER_SIZE, COVER_RADIUS);

        let Some(on_click) = self.on_cover_click.clone() else {
            return cover;
        };

        let overlay = button(
            container(icons::icon(Icon::Camera, typography::TEXT_24))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .width(Length::Fixed(COVER_SIZE))
        .height(Length::Fixed(COVER_SIZE))
        .padding(spacing::SP_0)
        .style(button_style::cover_scrim(COVER_RADIUS))
        .on_press(on_click);

        container(stack![cover, overlay])
            .width(Length::Fixed(COVER_SIZE))
            .height(Length::Fixed(COVER_SIZE))
            .into()
    }

    fn view_info(self) -> Element<'a, Message> {
        let mut info: Column<'a, Message> = Column::new().align_x(Alignment::Start).spacing(spacing::SP_2);

        if let Some(kicker) = self.kicker {
            info = info.push(
                text(kicker)
                    .font(SF_PRO)
                    .size(typography::TEXT_12)
                    .color(theme().content.muted),
            );
        }

        info = info.push(
            text(self.title)
                .font(SF_PRO)
                .size(typography::TEXT_36)
                .color(theme().content.primary),
        );

        if let Some(subtitle) = self.subtitle {
            info = info.push(subtitle);
        }

        if !self.meta.is_empty() {
            info = info.push(space().height(spacing::SP_8));
            for line in self.meta {
                info = info.push(
                    text(line)
                        .font(SF_PRO)
                        .size(typography::TEXT_13)
                        .color(theme().content.muted),
                );
            }
        }

        if let Some((on_press, is_playing)) = self.play {
            let glyph = if is_playing { Icon::Pause } else { Icon::Play };

            info = info.push(space().height(spacing::SP_16));
            info = info.push(
                button(
                    container(icons::icon(glyph, typography::TEXT_20))
                        .width(Length::Fixed(PLAY_BUTTON_SIZE))
                        .height(Length::Fixed(PLAY_BUTTON_SIZE))
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center),
                )
                .padding(spacing::SP_0)
                .style(button_style::hero_play(PLAY_BUTTON_SIZE))
                .on_press(on_press),
            );
        }

        info.into()
    }
}
