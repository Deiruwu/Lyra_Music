use std::sync::Arc;
use std::time::Instant;

use iced::widget::image::Handle;
use iced::widget::{column, container, image, responsive, row, space};
use iced::{alignment::Horizontal, Alignment, Color, Element, Length, Task, Theme};
use crate::model::audio_tech::PlayableTrack;
use crate::model::Artist;
use crate::ui::widgets::artist_links::artist_names_text_aligned;
use crate::ui::widgets::single_line_text::single_line_text_aligned;

// ACTUALIZADO A LA NUEVA RUTA:
use super::lyrics::lyrics_panel::{LyricsMessage, LyricsOutMessage, LyricsPanel};

use crate::ui::assets::fonts::SF_PRO;
use crate::ui::assets::{radii, spacing};
use crate::ui::theme::theme;
use crate::ui::assets::typography;
const ARTWORK_MAX_SIZE: f32 = 544.0;

#[derive(Debug, Clone)]
pub enum TheaterMessage {
    Lyrics(LyricsMessage),
}
#[derive(Debug, Clone)]
pub enum TheaterOutMessage {
    RequestSeek(std::time::Duration),
    Idle,
}

pub struct TheaterPanel {
    lyrics: LyricsPanel,
    current_title: Option<String>,
    current_artists: Vec<Artist>,
}

impl Default for TheaterPanel {
    fn default() -> Self {
        Self {
            lyrics: LyricsPanel::default(),
            current_title: None,
            current_artists: Vec::new(),
        }
    }
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

        let (task, _out) = self.lyrics.update(LyricsMessage::TrackChanged(Arc::clone(playable)));
        task.map(TheaterMessage::Lyrics)
    }

    pub fn position_updated(&mut self, position: std::time::Duration) -> Task<TheaterMessage> {
        let (task, _out) = self.lyrics.update(LyricsMessage::PositionUpdated(position));
        task.map(TheaterMessage::Lyrics)
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.lyrics.is_animating(now)
    }

    pub fn animation_frame(&mut self, instant: Instant) -> Task<TheaterMessage> {
        let (task, _out) = self.lyrics.update(LyricsMessage::AnimationFrame(instant));
        task.map(TheaterMessage::Lyrics)
    }

    pub fn view<'a>(&'a self, large_thumbnail: Option<&'a Handle>) -> Element<'a, TheaterMessage> {
        let thumbnail_handle = large_thumbnail.cloned();
        let title = self.current_title.clone().unwrap_or_default();
        let artists = self.current_artists.clone();

        let artwork_panel = responsive(move |size| {
            let available_height = (size.height - 80.0).max(50.0);

            let side = size.width.min(available_height).min(ARTWORK_MAX_SIZE);

            let artwork_box = |content: Element<'a, TheaterMessage>, style_bg: Option<Color>| -> container::Container<'a, TheaterMessage> {
                container(content)
                    .width(Length::Fixed(side))
                    .height(Length::Fixed(side))
                    .clip(true)
                    .style(move |_theme: &Theme| container::Style {
                        background: style_bg.map(Into::into),
                        border: iced::border::rounded(radii::R_16),
                        shadow: if style_bg.is_none() {
                            theme().elevation.shadow
                        } else {
                            Default::default()
                        },
                        ..Default::default()
                    })
            };

            let artwork: Element<'_, TheaterMessage> = match &thumbnail_handle {
                Some(handle) => artwork_box(
                    image(handle.clone())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .content_fit(iced::ContentFit::Cover)
                        .into(),
                    None,
                )
                    .into(),
                None => artwork_box(
                    space().width(Length::Fill).height(Length::Fill).into(),
                    Some(Color::TRANSPARENT),
                )
                    .into(),
            };

            let header = column![
                single_line_text_aligned(title.as_str(), SF_PRO, typography::TEXT_20, theme().content.primary, Length::Fixed(side), Horizontal::Center),
                artist_names_text_aligned(
                    &artists,
                    SF_PRO,
                    14.0,
                    theme().content.secondary,
                    Length::Fixed(side),
                    Horizontal::Center,
                ),
            ]
                .spacing(spacing::SP_4)
                .align_x(Alignment::Center);

            let artwork_column = column![artwork, header]
                .spacing(spacing::SP_18)
                .align_x(Alignment::Center);


            let (align_x, pad_right) = if size.width < ARTWORK_MAX_SIZE {
                (iced::alignment::Horizontal::Right, 24.0)
            } else {
                (iced::alignment::Horizontal::Center, 0.0)
            };

            container(artwork_column)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(align_x)
                .align_y(Alignment::Center)
                .padding(iced::Padding {
                    right: pad_right,
                    ..Default::default()
                })
                .into()
        });

        let lyrics_panel = container(self.lyrics.view().map(TheaterMessage::Lyrics))
            .width(Length::FillPortion(1))
            .height(Length::Fill);

        let divider = container(space().width(Length::Fixed(1.0)).height(Length::Fill))
            .style(|_theme: &Theme| container::Style {
                background: Some(theme().overlay.hover.into()),
                ..Default::default()
            });

        row![artwork_panel, divider, lyrics_panel]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}