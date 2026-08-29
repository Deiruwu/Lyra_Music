use std::sync::Arc;
use iced::{Alignment, Element, Length, Renderer, Task, Theme};
use iced::widget::image::Handle;
use iced::widget::{button, column, container, mouse_area, rich_text, row, slider, space, text};
use crate::audio::manager::manager::RepeatMode;
use crate::audio::track_event::TrackEvent;
use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;
use crate::ui::assets::icons::{self, Icon};
use crate::ui::styles::button as button_style;
use crate::ui::styles::slider as slider_style;
use crate::ui::widgets::artist_links::artist_links;
use crate::ui::widgets::single_line_text::single_line_text;
use crate::ui::widgets::track_row::track_thumbnail;
use crate::ui::assets::{spacing, typography};
use crate::ui::theme::theme;

/// Ancho máximo del bloque título/artista/álbum del track actual — acota
/// nombres largos (ver `current_track_content`) sin forzar que el bloque
/// ocupe ese ancho cuando el contenido real es más corto.
const CURRENT_TRACK_INFO_MAX_WIDTH: f32 = 260.0;

/// Destino de navegación al hacer click en el artista o álbum del track en reproducción.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackLink {
    Artist(String),
    Album(String),
}

#[derive(Debug, Clone)]
pub enum PlayerMessage {
    BackendEvent(TrackEvent),
    UiTogglePlayback,
    UiNext,
    UiPrev,
    UiSeek(f32),
    UiSeekReleased,
    UiToggleLike(String),
    UiToggleShuffle,
    UiCycleRepeat,
    OpenTrackLink(TrackLink),
    RightClicked,
}

#[derive(Debug, Clone)]
pub enum PlayerOutMessage {
    Idle,
    RequestTogglePlayback,
    RequestNext,
    RequestPrev,
    RequestSeek(f32),
    RequestToggleLike(String),
    RequestToggleShuffle,
    RequestCycleRepeat,
    RequestOpenTrackLink(TrackLink),
    RequestContextMenu,
}

pub struct Player {
    pub current_track: Option<Arc<PlayableTrack>>,
    /// Valor en curso mientras se arrastra el seek bar — la vista usa
    /// este valor en vez de `current_position` para no pelear con la
    /// posición real (que solo avanza cada 40ms via Tick). Se limpia
    /// y dispara el seek real recién al soltar.
    seek_preview: Option<f32>,
}

impl Default for Player {
    fn default() -> Self {
        Self { current_track: None, seek_preview: None }
    }
}

impl Player {
    pub fn update(&mut self, msg: PlayerMessage) -> (Task<PlayerMessage>, PlayerOutMessage) {
        match msg {
            PlayerMessage::BackendEvent(event) => match event {
                TrackEvent::TrackChanged(track) => {
                    self.current_track = Some(track);
                    (Task::none(), PlayerOutMessage::Idle)
                }
                TrackEvent::Stopped => {
                    self.current_track = None;
                    (Task::none(), PlayerOutMessage::Idle)
                }
                _ => (Task::none(), PlayerOutMessage::Idle),
            },
            PlayerMessage::UiTogglePlayback => (Task::none(), PlayerOutMessage::RequestTogglePlayback),
            PlayerMessage::UiNext           => (Task::none(), PlayerOutMessage::RequestNext),
            PlayerMessage::UiPrev           => (Task::none(), PlayerOutMessage::RequestPrev),
            PlayerMessage::UiSeek(pos) => {
                self.seek_preview = Some(pos);
                (Task::none(), PlayerOutMessage::Idle)
            }
            PlayerMessage::UiSeekReleased => match self.seek_preview.take() {
                Some(pos) => (Task::none(), PlayerOutMessage::RequestSeek(pos)),
                None      => (Task::none(), PlayerOutMessage::Idle),
            },
            PlayerMessage::UiToggleLike(track_id) => (Task::none(), PlayerOutMessage::RequestToggleLike(track_id)),
            PlayerMessage::UiToggleShuffle   => (Task::none(), PlayerOutMessage::RequestToggleShuffle),
            PlayerMessage::UiCycleRepeat     => (Task::none(), PlayerOutMessage::RequestCycleRepeat),
            PlayerMessage::OpenTrackLink(link) => (Task::none(), PlayerOutMessage::RequestOpenTrackLink(link)),
            PlayerMessage::RightClicked => (Task::none(), PlayerOutMessage::RequestContextMenu),
        }
    }

    pub fn view(
        &self,
        is_playing: bool,
        has_track: bool,
        has_history: bool,
        is_shuffled: bool,
        repeat_mode: RepeatMode,
    ) -> Element<'_, PlayerMessage> {
        let play_icon = if is_playing {
            icons::icon(Icon::Pause, typography::TEXT_16)
        } else {
            icons::icon(Icon::Play, typography::TEXT_16)
        };

        let active_color = theme().accent.primary;
        let inactive_color = theme().content.muted;

        let shuffle_button = {
            let color = if is_shuffled { active_color } else { inactive_color };
            button(
                icons::icon(Icon::Shuffle, typography::TEXT_16)
                    .style(move |_: &Theme| text::Style { color: Some(color) }),
            )
                .style(button_style::minimal)
                .on_press(PlayerMessage::UiToggleShuffle)
        };

        let prev_button = {
            let b: iced::widget::Button<'_, _, Theme, Renderer> =
                button(icons::icon(Icon::SkipPrevious, typography::TEXT_18)).style(button_style::minimal);
            if has_history { b.on_press(PlayerMessage::UiPrev) } else { b }
        };

        let play_button = {
            let b = button(play_icon).style(button_style::minimal);
            if has_track { b.on_press(PlayerMessage::UiTogglePlayback) } else { b }
        };

        let next_button = {
            let b: iced::widget::Button<'_, _, Theme, Renderer> =
                button(icons::icon(Icon::SkipNext, typography::TEXT_18)).style(button_style::minimal);
            if has_track { b.on_press(PlayerMessage::UiNext) } else { b }
        };

        let repeat_button = {
            let (icon_variant, color) = match repeat_mode {
                RepeatMode::Off   => (Icon::Repeat, inactive_color),
                RepeatMode::Queue => (Icon::Repeat, active_color),
                RepeatMode::Track => (Icon::RepeatOne, active_color),
            };
            button(
                icons::icon(icon_variant, typography::TEXT_16)
                    .style(move |_: &Theme| text::Style { color: Some(color) }),
            )
                .style(button_style::minimal)
                .on_press(PlayerMessage::UiCycleRepeat)
        };

        row![shuffle_button, prev_button, play_button, next_button, repeat_button]
            .spacing(spacing::SP_15)
            .align_y(Alignment::Center)
            .into()
    }

    /// Miniatura "small" del track en reproducción, obtenida por descarga
    /// directa como tupla `(track_id, Handle)` (mismo patrón que el teatro).
    ///
    /// El placeholder "Descargando…" (cuando el motor espera la descarga
    /// antes de sonar y no hay current track) se movió a feat futuro — ver
    /// el comentario "FEAT FUTURO: canción en descarga visible en la cola"
    /// en queue_panel.rs.
    pub fn view_current_play(
        &self,
        thumbnail: Option<Handle>,
        is_liked: bool,
    ) -> Element<'_, PlayerMessage> {
        match &self.current_track {
            Some(track) => {
                let like = Self::like_button(track.track.id.clone(), is_liked);
                Self::current_track_content(&track.track, thumbnail, like)
            }

            None => space().into(),
        }
    }

    /// Thumbnail + título + artista/álbum del track en reproducción. El
    /// nombre del artista y del álbum son links de `rich_text`: el hover
    /// los subraya de forma nativa (sin fondo, para no competir
    /// visualmente con el resto de botones de la barra) y el click abre
    /// `ArtistView`/`AlbumView`. Si el track no tiene artista o álbum
    /// resuelto, ese tramo queda como texto plano sin link.
    fn current_track_content<'a>(
        track: &'a Track,
        thumbnail: Option<Handle>,
        trailing: Element<'a, PlayerMessage>,
    ) -> Element<'a, PlayerMessage> {
        let title = single_line_text(&track.title, iced::Font::default(), typography::TEXT_14, theme().content.primary, Length::Shrink);

        let subtitle_color = theme().content.muted;

        let album_span = {
            let album_name = track.album.as_ref().map(|a| a.name.as_str()).unwrap_or("");
            let mut span = iced::widget::span(album_name).size(typography::TEXT_11).color(subtitle_color);
            if let Some(album) = &track.album {
                span = span.link(TrackLink::Album(album.id.clone()));
            }
            span
        };

        // Cada línea (título/artista/álbum) se mide por su contenido real
        // (`Length::Shrink`) para que el botón de like quede pegado al
        // texto en vez de al borde de una caja ancha — el tope lo pone
        // `info.max_width(...)` más abajo, no cada línea por separado. El
        // artista puede ser una lista (colabs): cada nombre es un link
        // individual a su propio artista, no uno solo apuntando al primero.
        let artist_line = artist_links(
            &track.artists,
            iced::Font::default(),
            11.0,
            subtitle_color,
            Length::Shrink,
            |id| PlayerMessage::OpenTrackLink(TrackLink::Artist(id)),
        );

        let album_line = container(
            rich_text![album_span]
                .on_link_click(PlayerMessage::OpenTrackLink)
                .wrapping(iced::widget::text::Wrapping::None),
        )
            .width(Length::Shrink)
            .clip(true);

        let info = column![title, artist_line, album_line]
            .width(Length::Shrink)
            .max_width(CURRENT_TRACK_INFO_MAX_WIDTH)
            .clip(true)
            .align_x(Alignment::Start);

        let content = row![track_thumbnail(thumbnail), info, trailing]
            .spacing(spacing::SP_12)
            .align_y(Alignment::Center);

        mouse_area(content)
            .on_right_press(PlayerMessage::RightClicked)
            .into()
    }

    fn like_button(track_id: String, is_liked: bool) -> Element<'static, PlayerMessage> {
        let (icon_variant, color) = if is_liked {
            (Icon::HeartFull, theme().status.liked)
        } else {
            (Icon::Heart, theme().content.muted)
        };

        button(
            icons::icon(icon_variant, typography::TEXT_18)
                .style(move |_: &Theme| text::Style { color: Some(color) }),
        )
            .style(button_style::minimal)
            .on_press(PlayerMessage::UiToggleLike(track_id))
            .into()
    }

    pub fn view_seek_bar(&self, current_position: f32) -> Element<'_, PlayerMessage> {
        let duration = self.current_track
            .as_ref()
            .and_then(|t| t.audio.duration_secs)
            .unwrap_or(0) as f32;

        let display_duration = duration.max(current_position);
        let display_position = self.seek_preview.unwrap_or(current_position);

        row![
            text(format!("{}:{:02}", (display_position / 60.0) as u32, (display_position % 60.0) as u32)).size(typography::TEXT_10_5),
            slider(0.0..=display_duration, display_position, PlayerMessage::UiSeek)
                .step(1.0)
                .on_release(PlayerMessage::UiSeekReleased)
                .style(slider_style::track),
            text(format!("{}:{:02}", (display_duration / 60.0) as u32, (display_duration % 60.0) as u32)).size(typography::TEXT_10_5),
        ]
            .spacing(spacing::SP_6)
            .align_y(Alignment::Center)
            .into()
    }

    pub fn has_track(&self) -> bool {
        self.current_track.is_some()
    }
}