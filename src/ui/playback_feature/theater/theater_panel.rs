use std::sync::Arc;
use std::time::Instant;

use iced::widget::image::Handle;
use iced::widget::{column, container, image, responsive, row, space, stack};
use iced::{alignment::Horizontal, Alignment, Color, ContentFit, Element, Font, Length, Task, Theme};
use crate::model::audio_tech::PlayableTrack;
use crate::model::Artist;
use crate::ui::widgets::artist_links::artist_names_text_aligned;
use crate::ui::widgets::single_line_text::single_line_text_aligned;

use super::lyrics::lyrics_panel::{LyricsMessage, LyricsOutMessage, LyricsPanel};
use super::theater_backdrop::TheaterBackdrop;
use crate::ui::utils::color::lerp_color;

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{radii, spacing};
use crate::ui::theme::theme;
use crate::ui::assets::typography;

const ARTWORK_MAX_SIZE: f32 = 520.0;
/// Alto reservado debajo de la carátula para álbum, título y artistas.
const ARTWORK_CAPTION_HEIGHT: f32 = 110.0;
const GLOW_BLUR: f32 = 80.0;
const GLOW_OFFSET_Y: f32 = 16.0;
/// Oscurecimiento de la portada desenfocada arriba y, abajo, cuánto se funde con el panel.
const SCRIM_TOP_ALPHA: f32 = 0.42;
const SCRIM_BOTTOM_ALPHA: f32 = 0.88;
/// Sombra extra del lado de la letra para que se lea sobre portadas claras.
const LYRICS_SCRIM_ALPHA: f32 = 0.30;
/// Cuánto del tono ambiental queda a mitad del degradado de respaldo (sin portada).
const AMBIENT_MID_PANEL_MIX: f32 = 0.65;
const TITLE_FONT: Font = Font { weight: iced::font::Weight::Semibold, ..SF_PRO };

#[derive(Debug, Clone)]
pub enum TheaterMessage {
    Lyrics(LyricsMessage),
}
#[derive(Debug, Clone)]
pub enum TheaterOutMessage {
    RequestSeek(std::time::Duration),
    Idle,
}

#[derive(Default)]
pub struct TheaterPanel {
    lyrics: LyricsPanel,
    current_title: Option<String>,
    current_artists: Vec<Artist>,
    current_album: Option<String>,
    backdrop: TheaterBackdrop,
}


impl TheaterPanel {
    pub fn update(&mut self, msg: TheaterMessage) -> (Task<TheaterMessage>, TheaterOutMessage) {
        match msg {
            TheaterMessage::Lyrics(msg) => {
                let (task, out) = self.lyrics.update(msg);
                let out = match out {
                    LyricsOutMessage::RequestSeek(t) => TheaterOutMessage::RequestSeek(t),
                    LyricsOutMessage::Idle => TheaterOutMessage::Idle,
                };
                (task.map(TheaterMessage::Lyrics), out)
            }
        }
    }

    pub fn track_changed(&mut self, playable: &Arc<PlayableTrack>) -> Task<TheaterMessage> {
        self.current_title = Some(playable.track.title.clone());
        self.current_artists = playable.track.artists.clone();
        self.current_album = playable.track.album.as_ref().map(|album| album.name.clone());

        let (task, _out) = self.lyrics.update(LyricsMessage::TrackChanged(Arc::clone(playable)));
        task.map(TheaterMessage::Lyrics)
    }

    pub fn position_updated(&mut self, position: std::time::Duration) -> Task<TheaterMessage> {
        let (task, _out) = self.lyrics.update(LyricsMessage::PositionUpdated(position));
        task.map(TheaterMessage::Lyrics)
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.lyrics.is_animating(now) || self.backdrop.is_animating(now)
    }

    pub fn animation_frame(&mut self, instant: Instant) -> Task<TheaterMessage> {
        let (task, _out) = self.lyrics.update(LyricsMessage::AnimationFrame(instant));
        task.map(TheaterMessage::Lyrics)
    }

    /// Portada desenfocada y color predominante de la canción actual (`None` = fondo neutro).
    pub fn set_backdrop(&mut self, image: Option<Handle>, color: Option<Color>) {
        self.backdrop.set(image, color, Instant::now());
    }

    pub fn view<'a>(&'a self, large_thumbnail: Option<&'a Handle>) -> Element<'a, TheaterMessage> {
        let now = Instant::now();
        let tones = self.backdrop.tones(now);

        let content = row![
            self.view_artwork(large_thumbnail.cloned(), tones.glow),
            container(self.lyrics.view().map(TheaterMessage::Lyrics))
                .width(Length::FillPortion(1))
                .height(Length::Fill),
        ]
            .spacing(spacing::SP_32)
            .width(Length::Fill)
            .height(Length::Fill);

        let mut layers: Vec<Element<'a, TheaterMessage>> = vec![fallback_background(tones.ambient)];
        layers.extend(self.view_blurred_covers(now));
        layers.push(vertical_scrim());
        layers.push(lyrics_scrim());
        layers.push(container(content).padding(spacing::SP_32).into());

        stack(layers).width(Length::Fill).height(Length::Fill).into()
    }

    /// Portada desenfocada de la canción anterior y la actual entrando encima.
    fn view_blurred_covers(&self, now: Instant) -> Vec<Element<'_, TheaterMessage>> {
        let progress = self.backdrop.progress(now);
        let mut layers = Vec::new();
        if progress < 1.0
            && let Some(previous) = self.backdrop.previous()
        {
            // Sin portada nueva, la anterior se desvanece; con portada nueva, la tapa.
            let opacity = if self.backdrop.current().is_some() { 1.0 } else { 1.0 - progress };
            layers.push(blurred_cover(previous.clone(), opacity));
        }
        if let Some(current) = self.backdrop.current() {
            layers.push(blurred_cover(current.clone(), progress));
        }
        layers
    }

    /// Carátula con resplandor de su color y, debajo, álbum, título y artistas.
    fn view_artwork<'a>(&'a self, thumbnail: Option<Handle>, glow: Color) -> Element<'a, TheaterMessage> {
        let title = self.current_title.clone().unwrap_or_default();
        let artists = self.current_artists.clone();
        let album = self.current_album.clone().unwrap_or_default().to_uppercase();

        responsive(move |size| {
            let side = size.width.min(size.height - ARTWORK_CAPTION_HEIGHT).clamp(50.0, ARTWORK_MAX_SIZE);

            let artwork: Element<'_, TheaterMessage> = match &thumbnail {
                Some(handle) => image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Cover)
                    .into(),
                None => space().width(Length::Fill).height(Length::Fill).into(),
            };
            let has_artwork = thumbnail.is_some();
            let artwork = container(artwork)
                .width(Length::Fixed(side))
                .height(Length::Fixed(side))
                .clip(true)
                .style(move |_theme: &Theme| container::Style {
                    background: (!has_artwork).then(|| theme().overlay.hover.into()),
                    border: iced::border::rounded(radii::R_16),
                    shadow: iced::Shadow { color: glow, offset: iced::Vector::new(0.0, GLOW_OFFSET_Y), blur_radius: GLOW_BLUR },
                    ..Default::default()
                });

            let caption = column![
                single_line_text_aligned(album.as_str(), SF_PRO, typography::TEXT_12, theme().content.muted, Length::Fixed(side), Horizontal::Center),
                single_line_text_aligned(title.as_str(), TITLE_FONT, typography::TEXT_22, theme().content.primary, Length::Fixed(side), Horizontal::Center),
                artist_names_text_aligned(&artists, SF_PRO, typography::TEXT_14, theme().content.secondary, Length::Fixed(side), Horizontal::Center),
            ]
                .spacing(spacing::SP_6)
                .align_x(Alignment::Center);

            container(column![artwork, caption].spacing(spacing::SP_24).align_x(Alignment::Center))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .into()
        })
            .width(Length::FillPortion(1))
            .into()
    }
}

/// Fondo cuando no hay portada: el tono ambiental desde la esquina de la carátula hacia el panel.
fn fallback_background<'a>(ambient: Color) -> Element<'a, TheaterMessage> {
    let panel = theme().surface.panel;
    gradient_layer(
        iced::gradient::Linear::new(std::f32::consts::PI * 0.75)
            .add_stop(0.0, ambient)
            .add_stop(0.5, lerp_color(ambient, panel, AMBIENT_MID_PANEL_MIX))
            .add_stop(1.0, panel),
    )
}

fn blurred_cover<'a>(handle: Handle, opacity: f32) -> Element<'a, TheaterMessage> {
    image(handle)
        .width(Length::Fill)
        .height(Length::Fill)
        .content_fit(ContentFit::Cover)
        .border_radius(radii::R_18)
        .opacity(opacity)
        .into()
}

/// Oscurece la portada arriba y la funde con el panel abajo (hacia la barra de reproducción).
fn vertical_scrim<'a>() -> Element<'a, TheaterMessage> {
    let panel = theme().surface.panel;
    gradient_layer(
        iced::gradient::Linear::new(std::f32::consts::PI)
            .add_stop(0.0, Color { a: SCRIM_TOP_ALPHA, ..Color::BLACK })
            .add_stop(1.0, Color { a: SCRIM_BOTTOM_ALPHA, ..panel }),
    )
}

/// Sombra suave del lado de la letra.
fn lyrics_scrim<'a>() -> Element<'a, TheaterMessage> {
    gradient_layer(
        iced::gradient::Linear::new(std::f32::consts::PI * 0.5)
            .add_stop(0.0, Color::TRANSPARENT)
            .add_stop(0.45, Color::TRANSPARENT)
            .add_stop(1.0, Color { a: LYRICS_SCRIM_ALPHA, ..Color::BLACK }),
    )
}

fn gradient_layer<'a>(gradient: iced::gradient::Linear) -> Element<'a, TheaterMessage> {
    container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(gradient.into()),
            border: iced::border::rounded(radii::R_18),
            ..Default::default()
        })
        .into()
}
