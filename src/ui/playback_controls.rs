use std::sync::Arc;
use std::sync::atomic::Ordering;
use iced::{Alignment, Color, Element, Length, Renderer, Subscription, Task, Theme};
use iced::widget::{button, container, column, row, text, slider, space};
use std::time::Duration;
use iced::widget::image::Handle;
use iced::stream;
use iced::futures::sink::SinkExt;
use crate::audio::manager::TrackManager;
use crate::audio::track_event::TrackEvent;
use crate::{JETBRAINS_MONO};
use crate::model::audio_tech::PlayableTrack;
use crate::ui::widgets::track_row::currently_playing_row;
use crate::ui::styles::styles::transparent_button;

use std::sync::OnceLock;
use tokio::sync::broadcast;

static TX: OnceLock<broadcast::Sender<TrackEvent>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum PlaybackMessage {
    Play(PlayableTrack),
    TogglePlayback,
    Next,
    Prev,
    Stop,
    Seek(f32),
    VolumeChanged(f32),
    ThumbnailLoaded(Vec<u8>),
    Tick,
    BackendEvent(TrackEvent),
}

pub struct PlaybackControls {
    pub audio_engine: Arc<TrackManager>,
    current_track: Option<Arc<PlayableTrack>>,
    current_thumbnail: Option<Handle>,
}

impl PlaybackControls {
    pub fn new(audio_engine: Arc<TrackManager>) -> Self {
        Self {
            audio_engine,
            current_track: None,
            current_thumbnail: None,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.audio_engine.state.status.load(Ordering::Relaxed) == 1
    }



    pub fn subscription(&self) -> Subscription<PlaybackMessage> {
        TX.set(self.audio_engine.event_tx.clone()).ok();
        Subscription::run(backend_events)
    }
    
    pub fn update(&mut self, msg: PlaybackMessage) -> Task<PlaybackMessage> {
        match msg {
            PlaybackMessage::Tick => Task::none(),

            PlaybackMessage::BackendEvent(event) => match event {
                TrackEvent::TrackChanged(track) => {
                    self.current_thumbnail = None;
                    let url = track.track.thumbnail_url.clone();
                    self.current_track = Some(track);

                    if let Some(url) = url {
                        return Task::perform(
                            crate::ui::utils::image::download_thumbnail(url),
                            |result| match result {
                                Ok(bytes) => PlaybackMessage::ThumbnailLoaded(bytes),
                                Err(_) => PlaybackMessage::ThumbnailLoaded(vec![]),
                            },
                        );
                    }
                    Task::none()
                }
                _ => Task::none(),
            },

            PlaybackMessage::ThumbnailLoaded(bytes) => {
                if !bytes.is_empty() {
                    self.current_thumbnail = Some(Handle::from_bytes(bytes));
                }
                Task::none()
            }

            PlaybackMessage::Play(track) => {
                self.audio_engine.enqueue(track);
                Task::none()
            }
            PlaybackMessage::TogglePlayback => {
                if self.is_playing() {
                    self.audio_engine.pause();
                } else {
                    self.audio_engine.resume();
                }
                Task::none()
            }
            PlaybackMessage::Next => {
                self.audio_engine.skip_next();
                Task::none()
            }
            PlaybackMessage::Prev => Task::none(),
            PlaybackMessage::Stop => {
                self.audio_engine.stop();
                Task::none()
            }
            PlaybackMessage::Seek(posicion) => {
                self.audio_engine.seek(Duration::from_secs_f32(posicion));
                Task::none()
            }
            PlaybackMessage::VolumeChanged(nuevo_volumen) => {
                self.audio_engine.set_volume(nuevo_volumen);
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, PlaybackMessage> {
        let current_position = self.audio_engine.get_position().as_secs_f32();
        let volume = self.audio_engine.get_volume();
        let is_playing = self.is_playing();
        let has_track = self.current_track.is_some();

        let current_track_widget: Element<_> = match &self.current_track {
            Some(track) => currently_playing_row(&track.track, self.current_thumbnail.as_ref()),
            None => space().into(),
        };

        let play_controller = row![
            container(current_track_widget).width(Length::FillPortion(1)),
            container(self.get_play_center(is_playing, has_track))
                .width(Length::FillPortion(2))
                .align_x(Alignment::Center),
            container(self.get_volume_slider(volume))
                .width(Length::FillPortion(1))
                .align_x(Alignment::End),
        ]
            .width(Length::Fill)
            .align_y(Alignment::Center);

        let layout_final = column![self.get_seek_bar(current_position), play_controller]
            .spacing(10)
            .align_x(Alignment::Center);

        container(layout_final)
            .width(Length::Fill)
            .padding(10)
            .align_x(Alignment::Center)
            .style(|_theme: &Theme| container::Style {
                background: Some(Color::from_rgb(0.1, 0.1, 0.1).into()),
                text_color: Some(Color::WHITE),
                ..Default::default()
            })
            .into()
    }

    fn get_seek_bar(&self, current_position: f32) -> Element<'_, PlaybackMessage> {
        let duration = self.current_track
            .as_ref()
            .and_then(|t| t.audio.duration_secs)
            .unwrap_or(0) as f32;

        let display_duration = duration.max(current_position);

        row![
            text(format!("{}:{:02}", (current_position / 60.0) as u32, (current_position % 60.0) as u32)).size(12),
            slider(0.0..=display_duration, current_position, PlaybackMessage::Seek).step(1.0),
            text(format!("{}:{:02}", (display_duration / 60.0) as u32, (display_duration % 60.0) as u32)).size(12),
        ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
    }

    fn get_play_center(&self, is_playing: bool, has_track: bool) -> Element<'_, PlaybackMessage> {
        let play_icon = if is_playing {
            text("").font(JETBRAINS_MONO)
        } else {
            text("").font(JETBRAINS_MONO)
        };

        let prev_button = {
            let b: iced::widget::Button<'_, _, Theme, Renderer> =
                button(text("󰒮").font(JETBRAINS_MONO).size(18)).style(transparent_button);
            if has_track { b.on_press(PlaybackMessage::Prev) } else { b }
        };

        let play_button = {
            let b = button(play_icon).style(transparent_button);
            if has_track { b.on_press(PlaybackMessage::TogglePlayback) } else { b }
        };

        let next_button = {
            let b: iced::widget::Button<'_, _, Theme, Renderer> =
                button(text("󰒭").font(JETBRAINS_MONO).size(18)).style(transparent_button);
            if has_track { b.on_press(PlaybackMessage::Next) } else { b }
        };

        row![prev_button, play_button, next_button]
            .spacing(15)
            .align_y(Alignment::Center)
            .into()
    }

    fn get_volume_slider(&self, volume: f32) -> Element<'_, PlaybackMessage> {
        let volume_icon = if volume < 0.2 { "" } else if volume > 0.8 { "" } else { "" };

        column![
            row![
                text(volume_icon).font(JETBRAINS_MONO),
                slider(0.0..=1.0, volume, PlaybackMessage::VolumeChanged)
                    .step(0.01)
                    .width(200)
            ]
            .spacing(20)
        ]
            .spacing(20)
            .padding(20)
            .max_width(150)
            .into()
    }
}

fn backend_events() -> impl futures::Stream<Item = PlaybackMessage> {
    let tx = TX.get().unwrap().clone();
    stream::channel(100, async move |mut output| {
        let mut receiver = tx.subscribe();
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let _ = output.send(PlaybackMessage::BackendEvent(event)).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}