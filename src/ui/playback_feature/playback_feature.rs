use std::sync::{Arc, OnceLock};
use std::time::Duration;
use futures::SinkExt;
use iced::{stream, Alignment, Color, Element, Length, Subscription, Task, Theme};
use iced::widget::{container, column, row};
use tokio::sync::broadcast;

use crate::model::audio_tech::PlayableTrack;
use crate::audio::manager::TrackManager;
use crate::audio::track_event::{QueueEvent, TrackEvent};

use crate::ui::playback_feature::player::{Player, PlayerMessage, PlayerOutMessage};
use crate::ui::playback_feature::queue_panel::{QueueMessage, QueueOutMessage, QueuePanel};
use crate::ui::playback_feature::volume::{Volume, VolumeMessage, VolumeOutMessage};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;

#[derive(Debug, Clone)]
pub enum PlaybackFeatureMessage {
    Player(PlayerMessage),
    Volume(VolumeMessage),
    Queue(QueueMessage),
    QueueChanged,
    ThumbnailLoaded { track_id: String, bytes: Vec<u8> },
    Play(PlayableTrack),
    Tick,
}

pub struct PlaybackFeature {
    manager: Arc<TrackManager>,
    queue: QueuePanel,
    player: Player,
    volume: Volume,
}

impl PlaybackFeature {
    pub fn new(manager: Arc<TrackManager>) -> Self {
        QUEUE_TX.set(manager.queue_tx.clone()).ok();
        TX.set(manager.event_tx.clone()).ok();
        Self {
            queue: QueuePanel::default(),
            player: Player::default(),
            volume: Volume::default(),
            manager,
        }
    }

    pub fn subscription(&self) -> Subscription<PlaybackFeatureMessage> {
        let tick_sub = iced::time::every(Duration::from_millis(100))
            .map(|_| PlaybackFeatureMessage::Tick);

        Subscription::batch([
            Subscription::run(queue_events),
            Subscription::run(backend_events).map(PlaybackFeatureMessage::Player),
            tick_sub,
        ])
    }

    pub fn update(
        &mut self,
        msg: PlaybackFeatureMessage,
        thumbnails: &mut ThumbnailCache
    ) -> Task<PlaybackFeatureMessage> {
        match msg {
            PlaybackFeatureMessage::Play(track) => {
                self.manager.enqueue(track);
                Task::none()
            }

            PlaybackFeatureMessage::QueueChanged => {
                let tracks = self.manager.get_queue_snapshot();
                self.queue.queue_update(tracks.clone());

                // Usamos el caché inyectado
                let tasks: Vec<Task<_>> = tracks.into_iter().filter_map(|t| {
                    let id = t.track.id.clone();
                    let url = t.track.thumbnail_small.clone()?;
                    thumbnails.request_download(id, url, |id, bytes| {
                        PlaybackFeatureMessage::ThumbnailLoaded { track_id: id, bytes }
                    })
                }).collect();

                Task::batch(tasks)
            }

            PlaybackFeatureMessage::ThumbnailLoaded { track_id, bytes } => {
                thumbnails.insert(track_id, bytes);
                Task::none()
            }

            PlaybackFeatureMessage::Tick => {
            Task::none()
            }

            // ==========================================
            // EVALUACIÓN DE OUT-MESSAGES
            // ==========================================
            PlaybackFeatureMessage::Queue(msg) => {
                let (task, out_msg) = self.queue.update(msg);

                match out_msg {
                    QueueOutMessage::RequestPlay(index) => self.manager.skip_to_index(index).unwrap(),
                    QueueOutMessage::RequestRemove(index) => self.manager.remove_from_queue(index).unwrap(),
                    QueueOutMessage::RequestMove(from, to) => self.manager.move_in_queue(from, to).unwrap(),
                    QueueOutMessage::Idle => {}
                }

                task.map(PlaybackFeatureMessage::Queue)
            }

            PlaybackFeatureMessage::Player(msg) => {
                let mut extra_task = Task::none();

                if let PlayerMessage::BackendEvent(TrackEvent::TrackChanged(ref track)) = msg {
                    let id = track.track.id.clone();
                    if let Some(url) = track.track.thumbnail_small.clone() {
                        // Usamos el caché inyectado
                        if let Some(t) = thumbnails.request_download(id, url, |id, bytes| {
                            PlaybackFeatureMessage::ThumbnailLoaded { track_id: id, bytes }
                        }) {
                            extra_task = t;
                        }
                    }
                }

                let (task, out_msg) = self.player.update(msg);

                match out_msg {
                    PlayerOutMessage::RequestTogglePlayback => {
                        if self.manager.state.is_playing() { self.manager.pause(); }
                        else { self.manager.resume(); }
                    }
                    PlayerOutMessage::RequestNext => self.manager.skip_next(),
                    PlayerOutMessage::RequestPrev => { /* TODO: Implementar prev en core */ },
                    PlayerOutMessage::RequestSeek(pos) => self.manager.seek(Duration::from_secs_f32(pos)),
                    PlayerOutMessage::Idle => {}
                }

                Task::batch(vec![task.map(PlaybackFeatureMessage::Player), extra_task])
            }

            PlaybackFeatureMessage::Volume(msg) => {
                let (task, out_msg) = self.volume.update(msg);

                match out_msg {
                    VolumeOutMessage::RequestVolumeChange(vol) => self.manager.set_volume(vol),
                    VolumeOutMessage::Idle => {}
                }

                task.map(PlaybackFeatureMessage::Volume)
            }
        }
    }

    pub fn view(&self, thumbnails: &ThumbnailCache) -> Element<'_, PlaybackFeatureMessage> {
        let current_position = self.manager.get_position().as_secs_f32();
        let vol = self.manager.get_volume();

        let current_thumbnail = self.player.current_track.as_ref()
            .and_then(|t| thumbnails.peek(&t.track.id));

        let current_track = self.player.view_current_play(current_thumbnail).map(PlaybackFeatureMessage::Player);
        let play_center = self.player.view(self.manager.state.is_playing(), self.player.has_track()).map(PlaybackFeatureMessage::Player);
        let seek_bar = self.player.view_seek_bar(current_position).map(PlaybackFeatureMessage::Player);
        let vol_view = self.volume.view(vol).map(PlaybackFeatureMessage::Volume);
        let queue_toggle = self.queue.view_toggle_button().map(PlaybackFeatureMessage::Queue);

        let rigth_view = row![queue_toggle, vol_view]
            .align_y(Alignment::Center);

        let play_controller = row![
            container(current_track).width(Length::FillPortion(1)),
            container(play_center)
                .width(Length::FillPortion(4))
                .align_x(Alignment::Center),
            container(rigth_view)
                .width(Length::FillPortion(1))
                .align_x(Alignment::End),
        ]
            .width(Length::Fill)
            .align_y(Alignment::Center);

        let layout_final = column![
            seek_bar,
            play_controller,
        ]
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

    pub fn view_queue(&self, thumbnails: &ThumbnailCache) -> Element<'_, PlaybackFeatureMessage> {
        self.queue.view(thumbnails).map(PlaybackFeatureMessage::Queue)
    }
}

static QUEUE_TX: OnceLock<broadcast::Sender<QueueEvent>> = OnceLock::new();
static TX: OnceLock<broadcast::Sender<TrackEvent>> = OnceLock::new();

fn queue_events() -> impl futures::Stream<Item = PlaybackFeatureMessage> {
    let tx = QUEUE_TX.get().unwrap().clone();
    stream::channel(100, async move |mut output| {
        let mut receiver = tx.subscribe();
        loop {
            match receiver.recv().await {
                Ok(QueueEvent::QueueChanged) => {
                    let _ = output.send(PlaybackFeatureMessage::QueueChanged).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

fn backend_events() -> impl futures::Stream<Item = PlayerMessage> {
    let tx = TX.get().unwrap().clone();
    stream::channel(100, async move |mut output| {
        let mut receiver = tx.subscribe();
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let _ = output.send(PlayerMessage::BackendEvent(event)).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}