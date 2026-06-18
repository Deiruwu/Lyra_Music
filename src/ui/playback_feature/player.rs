use std::sync::Arc;
use iced::{Alignment, Element, Renderer, Task, Theme};
use iced::widget::image::Handle;
use iced::widget::{button, row, slider, space, text};
use crate::audio::track_event::TrackEvent;
use crate::JETBRAINS_MONO;
use crate::model::audio_tech::PlayableTrack;
use crate::ui::styles::styles::transparent_button;
use crate::ui::widgets::track_row::currently_playing_row;

#[derive(Debug, Clone)]
pub enum PlayerMessage {
    BackendEvent(TrackEvent),
    UiTogglePlayback,
    UiNext,
    UiPrev,
    UiSeek(f32),
}

#[derive(Debug, Clone)]
pub enum PlayerOutMessage {
    Idle,
    RequestTogglePlayback,
    RequestNext,
    RequestPrev,
    RequestSeek(f32),
}

pub struct Player {
    pub current_track: Option<Arc<PlayableTrack>>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            current_track: None,
        }
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
            PlayerMessage::UiNext => (Task::none(), PlayerOutMessage::RequestNext),
            PlayerMessage::UiPrev => (Task::none(), PlayerOutMessage::RequestPrev),
            PlayerMessage::UiSeek(pos) => (Task::none(), PlayerOutMessage::RequestSeek(pos)),
        }
    }

    pub fn view(&self, is_playing: bool, has_track: bool) -> Element<'_, PlayerMessage> {
        let play_icon = if is_playing {
            text("").font(JETBRAINS_MONO)
        } else {
            text("").font(JETBRAINS_MONO)
        };

        let prev_button = {
            let b: iced::widget::Button<'_, _, Theme, Renderer> =
                button(text("󰒮").font(JETBRAINS_MONO).size(18)).style(transparent_button);
            if has_track { b.on_press(PlayerMessage::UiPrev) } else { b }
        };

        let play_button = {
            let b = button(play_icon).style(transparent_button);
            if has_track { b.on_press(PlayerMessage::UiTogglePlayback) } else { b }
        };

        let next_button = {
            let b: iced::widget::Button<'_, _, Theme, Renderer> =
                button(text("󰒭").font(JETBRAINS_MONO).size(18)).style(transparent_button);
            if has_track { b.on_press(PlayerMessage::UiNext) } else { b }
        };

        row![prev_button, play_button, next_button]
            .spacing(15)
            .align_y(Alignment::Center)
            .into()
    }

    pub fn view_current_play<'a>(&self, thumbnail: Option<Handle>) -> Element<'_, PlayerMessage> {
        match &self.current_track {
            Some(track) => currently_playing_row(&track.track, thumbnail),
            None => space().into(),
        }
    }

    pub fn view_seek_bar(&self, current_position: f32) -> Element<'_, PlayerMessage> {
        let duration = self.current_track
            .as_ref()
            .and_then(|t| t.audio.duration_secs)
            .unwrap_or(0) as f32;

        let display_duration = duration.max(current_position);

        row![
            text(format!("{}:{:02}", (current_position / 60.0) as u32, (current_position % 60.0) as u32)).size(12),
            slider(0.0..=display_duration, current_position, PlayerMessage::UiSeek).step(1.0),
            text(format!("{}:{:02}", (display_duration / 60.0) as u32, (display_duration % 60.0) as u32)).size(12),
        ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
    }

    pub fn has_track(&self) -> bool {
        self.current_track.is_some()
    }
}