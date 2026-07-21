use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use futures::SinkExt;
use iced::widget::image::Handle;
use iced::{stream, Alignment, Color, Element, Length, Subscription, Task, Theme};
use iced::widget::{container, column, row};
use tokio::sync::broadcast;

use crate::model::audio_tech::PlayableTrack;
use crate::audio::manager::manager::TrackManager;
use crate::audio::track_event::{QueueEvent, TrackEvent};

use crate::ui::playback_feature::player::{Player, PlayerMessage, PlayerOutMessage};
use crate::ui::playback_feature::queue::queue_panel::{QueueMessage, QueueOutMessage, QueuePanel};
use crate::ui::playback_feature::theater::theater_panel::{TheaterMessage, TheaterOutMessage, TheaterPanel};
use crate::ui::playback_feature::volume::{Volume, VolumeMessage, VolumeOutMessage};
use crate::ui::styles::styles::minimal_button;
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};

/// Epoch fijo para este feature. La cola de reproducción y la canción
/// actual no tienen un concepto de "vista invalidada" como el scroll del
/// Explorer — un thumbnail que llega tarde para una canción que sigue en
/// cola sigue siendo válido. Se usa 0 constante solo porque la firma de
/// `request_color` ahora lo exige.
const EPOCH: u64 = 0;

#[derive(Debug, Clone)]
pub enum PlaybackFeatureMessage {
    Player(PlayerMessage),
    Volume(VolumeMessage),
    Queue(QueueMessage),
    Theater(TheaterMessage),
    QueueChanged,
    DownloadingStarted(String),
    DownloadingFinished(String),
    ThumbnailColorLoaded { key: String, bytes: Vec<u8>, epoch: u64 },
    LargeThumbnailLoaded { track_id: String, bytes: Vec<u8> },
    Play(PlayableTrack),
    ToggleTheaterMode,
    Tick,
    AnimationFrame(Instant),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackOutMessage {
    ToggleTheaterMode,
    RequestToggleLike(String),
    Idle,
}

pub struct PlaybackFeature {
    manager: Arc<TrackManager>,
    queue: QueuePanel,
    player: Player,
    volume: Volume,
    theater: TheaterPanel,
    spinner_frame: u8,
    is_predownloading: bool,
    downloading_track_id: Option<String>,
    current_track_id: Option<String>,
    current_large_thumbnail: Option<(String, Handle)>,
}

impl PlaybackFeature {
    pub fn new(manager: Arc<TrackManager>) -> Self {
        QUEUE_TX.set(manager.queue_tx.clone()).ok();
        TX.set(manager.event_tx.clone()).ok();
        Self {
            queue: QueuePanel::default(),
            player: Player::default(),
            volume: Volume::default(),
            theater: TheaterPanel::default(),
            manager,
            spinner_frame: 0,
            is_predownloading: false,
            downloading_track_id: None,
            current_track_id: None,
            current_large_thumbnail: None,
        }
    }

    pub fn subscription(&self, is_theater_visible: bool) -> Subscription<PlaybackFeatureMessage> {
        let tick_sub = iced::time::every(Duration::from_millis(40))
            .map(|_| PlaybackFeatureMessage::Tick);

        let global_release = iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(PlaybackFeatureMessage::Queue(QueueMessage::DragReleased))
            }
            _ => None,
        });

        let mut subs = vec![
            Subscription::run(queue_events),
            Subscription::run(backend_events).map(PlaybackFeatureMessage::Player),
            tick_sub,
            global_release,
        ];

        if self.queue.is_animating() {
            subs.push(
                iced::window::frames()
                    .map(|instant| PlaybackFeatureMessage::Queue(QueueMessage::AnimationFrame(instant))),
            );
        }

        if is_theater_visible && self.theater.is_animating(Instant::now()) {
            subs.push(
                iced::window::frames().map(PlaybackFeatureMessage::AnimationFrame),
            );
        }

        Subscription::batch(subs)
    }

    fn is_downloading(&self) -> bool {
        self.manager.state.is_downloading() || self.is_predownloading
    }

    pub fn update(
        &mut self,
        msg: PlaybackFeatureMessage,
        thumbnails: &mut ThumbnailCache,
    ) -> (Task<PlaybackFeatureMessage>, PlaybackOutMessage) {
        match msg {
            PlaybackFeatureMessage::Play(track) => {
                self.manager.enqueue(track.track);
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::QueueChanged => {
                let tracks = self.manager.get_queue_snapshot();
                self.queue.queue_update(tracks.clone());

                let tasks: Vec<Task<_>> = tracks.into_iter().filter_map(|t| {
                    let url = t.thumbnail_small.clone()?;
                    let key = thumb_key(&t);
                    thumbnails.request_color(key, url, EPOCH, |key, bytes, epoch| {
                        PlaybackFeatureMessage::ThumbnailColorLoaded { key, bytes, epoch }
                    })
                }).collect();

                (Task::batch(tasks), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::DownloadingStarted(track_id) => {
                self.is_predownloading = true;
                self.downloading_track_id = Some(track_id);
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::DownloadingFinished(track_id) => {
                self.is_predownloading = false;
                if self.downloading_track_id.as_deref() == Some(track_id.as_str()) {
                    self.downloading_track_id = None;
                }
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::ThumbnailColorLoaded { key, bytes, .. } => {
                thumbnails.insert_color(key.clone(), bytes);
                // Libera el slot de concurrencia y arranca el siguiente
                // pendiente en la cola (si hay alguno).
                let next = thumbnails.on_color_finished(&key, |key, bytes, epoch| {
                    PlaybackFeatureMessage::ThumbnailColorLoaded { key, bytes, epoch }
                });
                (next, PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::LargeThumbnailLoaded { track_id, bytes } => {
                if self.current_track_id.as_deref() == Some(track_id.as_str()) && !bytes.is_empty() {
                    self.current_large_thumbnail = Some((track_id, Handle::from_bytes(bytes)));
                }
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::Tick => {
                if self.is_downloading() {
                    self.spinner_frame = (self.spinner_frame + 1) % 6;
                    let _ = self.queue.update(QueueMessage::Tick);
                }
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::AnimationFrame(instant) => {
                let task = self.theater.animation_frame(instant).map(PlaybackFeatureMessage::Theater);
                (task, PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::Queue(msg) => {
                let (task, out_msg) = self.queue.update(msg);

                match out_msg {
                    QueueOutMessage::RequestPlay(index)    => self.manager.skip_to_index(index).unwrap(),
                    QueueOutMessage::RequestRemove(index)  => self.manager.remove_from_queue(index).unwrap(),
                    QueueOutMessage::RequestMove(from, to) => self.manager.move_in_queue(from, to).unwrap(),
                    QueueOutMessage::Idle                  => {}
                }

                (task.map(PlaybackFeatureMessage::Queue), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::Theater(msg) => {
                let (task, out) = self.theater.update(msg);
                if let TheaterOutMessage::RequestSeek(timestamp) = out {
                    self.manager.seek(timestamp);
                }
                (task.map(PlaybackFeatureMessage::Theater), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::Player(msg) => {
                let mut extra_tasks = vec![];

                if let PlayerMessage::BackendEvent(TrackEvent::TrackChanged(ref playable)) = msg {
                    let key = thumb_key(&playable.track);
                    if let Some(url) = playable.track.thumbnail_small.clone() {
                        if let Some(t) = thumbnails.request_color(key, url, EPOCH, |key, bytes, epoch| {
                            PlaybackFeatureMessage::ThumbnailColorLoaded { key, bytes, epoch }
                        }) {
                            extra_tasks.push(t);
                        }
                    }

                    self.current_track_id = Some(playable.track.id.clone());
                    self.current_large_thumbnail = None; // reset inmediato al cambiar de track

                    let theater_task = self.theater
                        .track_changed(playable)
                        .map(PlaybackFeatureMessage::Theater);
                    extra_tasks.push(theater_task);

                    if let Some(url) = playable.track.thumbnail_large.clone() {
                        let track_id = playable.track.id.clone();
                        extra_tasks.push(Task::perform(
                            crate::ui::utils::image::download_thumbnail(url),
                            move |result| {
                                let bytes = result.unwrap_or_default();
                                PlaybackFeatureMessage::LargeThumbnailLoaded { track_id: track_id.clone(), bytes }
                            },
                        ));
                    }
                }

                let (task, out_msg) = self.player.update(msg);

                let mut feature_out = PlaybackOutMessage::Idle;

                match out_msg {
                    PlayerOutMessage::RequestTogglePlayback => {
                        if self.manager.state.is_playing() { self.manager.pause(); }
                        else { self.manager.resume(); }
                    }
                    PlayerOutMessage::RequestNext      => self.manager.skip_next(),
                    PlayerOutMessage::RequestPrev      => {
                        if let Err(e) = self.manager.skip_prev() {
                            eprintln!("Error: {}", e);
                        }
                    }
                    PlayerOutMessage::RequestSeek(pos) => {
                        let position = Duration::from_secs_f32(pos);
                        self.manager.seek(position);
                    }
                    PlayerOutMessage::RequestToggleLike(track_id) => {
                        feature_out = PlaybackOutMessage::RequestToggleLike(track_id);
                    }
                    PlayerOutMessage::Idle             => {}
                }

                extra_tasks.push(task.map(PlaybackFeatureMessage::Player));
                (Task::batch(extra_tasks), feature_out)
            }

            PlaybackFeatureMessage::ToggleTheaterMode => {
                (Task::none(), PlaybackOutMessage::ToggleTheaterMode)
            }

            PlaybackFeatureMessage::Volume(msg) => {
                let (task, out_msg) = self.volume.update(msg);
                match out_msg {
                    VolumeOutMessage::RequestVolumeChange(vol) => {
                        self.manager.set_volume(vol);
                    }
                }

                (task.map(PlaybackFeatureMessage::Volume), PlaybackOutMessage::Idle)
            }
        }
    }

    /// Id del track actualmente en reproducción, si hay alguno. `main.rs`
    /// lo usa para consultar `CatalogStore::track_by_id` y así saber si
    /// está likeado antes de llamar a `view()`.
    pub fn current_track_id(&self) -> Option<&str> {
        self.current_track_id.as_deref()
    }

    pub fn position_updated(&mut self, position: Duration) -> Task<PlaybackFeatureMessage> {
        self.theater.position_updated(position).map(PlaybackFeatureMessage::Theater)
    }

    pub fn view(&self, thumbnails: &ThumbnailCache, is_theater_mode: bool, is_current_liked: bool) -> Element<'_, PlaybackFeatureMessage> {
        let current_position = self.manager.get_position().as_secs_f32();
        let vol              = self.manager.get_volume();
        let has_track        = self.player.has_track();
        let has_history      = self.manager.history_len() != 0;
        let is_downloading   = self.is_downloading();

        let current_thumbnail = self.player.current_track.as_ref()
            .and_then(|p| thumbnails.peek_color(&thumb_key(&p.track)));

        let current_track = self.player
            .view_current_play(current_thumbnail, is_downloading, self.spinner_frame, is_current_liked)
            .map(PlaybackFeatureMessage::Player);

        let play_center  = self.player.view(self.manager.state.is_playing(), has_track, has_history).map(PlaybackFeatureMessage::Player);
        let seek_bar     = self.player.view_seek_bar(current_position).map(PlaybackFeatureMessage::Player);
        let vol_view     = self.volume.view(vol).map(PlaybackFeatureMessage::Volume);
        let queue_toggle   = self.queue.view_toggle_button().map(PlaybackFeatureMessage::Queue);
        let theater_toggle = self.view_theater_toggle(is_theater_mode);

        let right_view = row![
            container(queue_toggle).width(Length::Shrink),
            container(vol_view).width(Length::Fill),
            container(theater_toggle).width(Length::Shrink),
        ]
            .align_y(Alignment::Center)
            .width(Length::Fill);

        let right_view = container(right_view).max_width(250).width(Length::Fill);

        let play_controller = row![
            container(current_track).width(Length::FillPortion(2)),
            container(play_center)
                .width(Length::FillPortion(3))
                .align_x(Alignment::Center),
            container(right_view)
                .width(Length::FillPortion(2))
                .align_x(Alignment::End),

        ]
            .width(Length::Fill)
            .align_y(Alignment::Center);

        let layout_final = column![seek_bar, play_controller]
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
        self.queue
            .view(thumbnails, self.downloading_track_id.as_deref())
            .map(PlaybackFeatureMessage::Queue)
    }

    pub fn view_theater(&self) -> Element<'_, PlaybackFeatureMessage> {
        let handle = self.current_large_thumbnail.as_ref().map(|(_, h)| h);
        self.theater.view(handle).map(PlaybackFeatureMessage::Theater)
    }

    pub fn view_theater_toggle(&self, is_theater_mode: bool) -> Element<'_, PlaybackFeatureMessage> {
        use iced::widget::{button, text};

        let icon = if is_theater_mode { "" } else { "" };

        button(text(icon).font(crate::JETBRAINS_MONO).size(18))
            .style(minimal_button)
            .on_press(PlaybackFeatureMessage::ToggleTheaterMode)
            .into()
    }
}

static QUEUE_TX: OnceLock<broadcast::Sender<QueueEvent>> = OnceLock::new();
static TX:       OnceLock<broadcast::Sender<TrackEvent>> = OnceLock::new();

fn queue_events() -> impl futures::Stream<Item = PlaybackFeatureMessage> {
    let tx = QUEUE_TX.get().unwrap().clone();
    stream::channel(100, async move |mut output| {
        let mut rx = tx.subscribe();
        loop {
            match rx.recv().await {
                Ok(QueueEvent::QueueChanged) => { let _ = output.send(PlaybackFeatureMessage::QueueChanged).await; }
                Ok(QueueEvent::DownloadStarted(track)) => { let _ = output.send(PlaybackFeatureMessage::DownloadingStarted(track.id.clone())).await; }
                Ok(QueueEvent::DownloadFinished(track)) => { let _ = output.send(PlaybackFeatureMessage::DownloadingFinished(track.id.clone())).await; }
                Ok(QueueEvent::DownloadRequired(_)) => {}
                Err(_) => continue,
            }
        }
    })
}

fn backend_events() -> impl futures::Stream<Item = PlayerMessage> {
    let tx = TX.get().unwrap().clone();
    stream::channel(100, async move |mut output| {
        let mut rx = tx.subscribe();
        loop {
            if let Ok(event) = rx.recv().await {
                let _ = output.send(PlayerMessage::BackendEvent(event)).await;
            }
        }
    })
}