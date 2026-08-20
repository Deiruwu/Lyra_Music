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

use crate::ui::playback_feature::player::{Player, PlayerMessage, PlayerOutMessage, TrackLink};
use crate::ui::playback_feature::queue::queue_panel::{QueueMessage, QueueOutMessage, QueuePanel};
use crate::ui::playback_feature::theater::theater_panel::{TheaterMessage, TheaterOutMessage, TheaterPanel};
use crate::ui::playback_feature::volume::{Volume, VolumeMessage, VolumeOutMessage};
use crate::ui::styles::styles::minimal_button;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;

#[derive(Debug, Clone)]
pub enum PlaybackFeatureMessage {
    Player(PlayerMessage),
    Volume(VolumeMessage),
    Queue(QueueMessage),
    Theater(TheaterMessage),
    QueueChanged,
    QueueThumbnailLoaded { key: String, bytes: Vec<u8> },
    SmallThumbnailLoaded { track_id: String, bytes: Vec<u8> },
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
    RequestOpenTrackLink(TrackLink),
    Idle,
}

pub struct PlaybackFeature {
    manager: Arc<TrackManager>,
    queue: QueuePanel,
    player: Player,
    volume: Volume,
    theater: TheaterPanel,
    current_track_id: Option<String>,
    current_small_thumbnail: Option<(String, Handle)>,
    current_large_thumbnail: Option<(String, Handle)>,
    queue_thumbnails: AsyncThumbnail,
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
            current_track_id: None,
            current_small_thumbnail: None,
            current_large_thumbnail: None,
            queue_thumbnails: AsyncThumbnail::new(),
        }
    }

    pub fn subscription(&self, is_theater_visible: bool) -> Subscription<PlaybackFeatureMessage> {
        // El tick de 40ms sigue vivo: es el que provoca el re-render
        // periódico que mantiene al día la barra de progreso del seek bar.
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

        // AQUÍ ESTÁ EL CAMBIO CLAVE: Agregamos la condición de la animación de ancho
        if self.queue.is_animating() || self.queue.is_animating_width() {
            subs.push(
                iced::window::frames()
                    .map(|instant| PlaybackFeatureMessage::Queue(QueueMessage::AnimationFrame(instant))),
            );
        }

        // Mientras se arrastra una fila, mantenemos un tick de 16ms para
        // poder autoscrollear aunque el mouse deje de moverse cerca del borde.
        if self.queue.is_dragging() {
            subs.push(
                iced::time::every(Duration::from_millis(16))
                    .map(|_| PlaybackFeatureMessage::Queue(QueueMessage::AutoScrollTick)),
            );
        }

        if is_theater_visible && self.theater.is_animating(Instant::now()) {
            subs.push(
                iced::window::frames().map(PlaybackFeatureMessage::AnimationFrame),
            );
        }

        Subscription::batch(subs)
    }

    pub fn update(
        &mut self,
        msg: PlaybackFeatureMessage,
    ) -> (Task<PlaybackFeatureMessage>, PlaybackOutMessage) {
        let (task, out) = self.update_inner(msg);

        // Sincronización de miniaturas de la cola (AsyncThumbnail): el
        // universo pedido = la ventana visible actual (+buffer). Corre en
        // cada update sin importar qué evento llegó, igual que hace
        // ViewCoordinator con sus vistas.
        let wanted = self.queue.visible_thumbnail_targets();
        let sync_task = self.queue_thumbnails.sync(&wanted, |key, bytes| {
            PlaybackFeatureMessage::QueueThumbnailLoaded { key, bytes }
        });

        (Task::batch([task, sync_task]), out)
    }

    fn update_inner(
        &mut self,
        msg: PlaybackFeatureMessage,
    ) -> (Task<PlaybackFeatureMessage>, PlaybackOutMessage) {
        match msg {
            PlaybackFeatureMessage::Play(track) => {
                self.manager.enqueue(track.track);
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::QueueChanged => {
                let tracks = self.manager.get_queue_snapshot();
                self.queue.queue_update(tracks);
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::QueueThumbnailLoaded { key, bytes } => {
                self.queue_thumbnails.on_loaded(key, bytes);
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::SmallThumbnailLoaded { track_id, bytes } => {
                if self.current_track_id.as_deref() == Some(track_id.as_str()) && !bytes.is_empty() {
                    self.current_small_thumbnail = Some((track_id, Handle::from_bytes(bytes)));
                }
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::LargeThumbnailLoaded { track_id, bytes } => {
                if self.current_track_id.as_deref() == Some(track_id.as_str()) && !bytes.is_empty() {
                    self.current_large_thumbnail = Some((track_id, Handle::from_bytes(bytes)));
                }
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::Tick => {
                // El spinner de descarga (canción en cola con símbolo de
                // espera) se movió a featur futuro — ver comentario en
                // queue_panel.rs. El Tick se conserva para que main.rs
                // pueda sondear tray flags y actualizar la posición.
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
                    self.current_track_id = Some(playable.track.id.clone());
                    self.current_small_thumbnail = None; // reset inmediato al cambiar de track
                    self.current_large_thumbnail = None; // reset inmediato al cambiar de track

                    // Miniatura "chica" del track actual (patrón teatro pero
                    // small): se descarga directo y se guarda como tupla
                    // `(track_id, Handle)` para evitar condiciones de carrera
                    // si el track cambia mientras la imagen llega.
                    if let Some(url) = playable.track.thumbnail_small.clone() {
                        let track_id = playable.track.id.clone();
                        extra_tasks.push(Task::perform(
                            crate::ui::utils::image::download_thumbnail(url),
                            move |result| {
                                let bytes = result.unwrap_or_default();
                                PlaybackFeatureMessage::SmallThumbnailLoaded { track_id: track_id.clone(), bytes }
                            },
                        ));
                    }

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
                    PlayerOutMessage::RequestToggleShuffle => self.manager.toggle_shuffle(),
                    PlayerOutMessage::RequestCycleRepeat   => self.manager.cycle_repeat_mode(),
                    PlayerOutMessage::RequestOpenTrackLink(link) => {
                        feature_out = PlaybackOutMessage::RequestOpenTrackLink(link);
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

    pub fn view(&self, is_theater_mode: bool, is_current_liked: bool) -> Element<'_, PlaybackFeatureMessage> {
        let current_position = self.manager.get_position().as_secs_f32();
        let vol              = self.manager.get_volume();
        let has_track        = self.player.has_track();
        let has_history      = self.manager.history_len() != 0;

        let current_thumbnail = self.current_small_thumbnail.as_ref().map(|(_, h)| h).cloned();

        let current_track = self.player
            .view_current_play(current_thumbnail, is_current_liked)
            .map(PlaybackFeatureMessage::Player);

        let play_center  = self.player.view(
            self.manager.state.is_playing(),
            has_track,
            has_history,
            self.manager.is_shuffled(),
            self.manager.repeat_mode(),
        ).map(PlaybackFeatureMessage::Player);
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

    pub fn view_queue(&self) -> Element<'_, PlaybackFeatureMessage> {
        self.queue
            .view(&self.queue_thumbnails)
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
                // ── FEAT FUTURO: descarga visible en cola ──────────────────────
                // Cuando se reintroduzca el spinner de descarga en la cola,
                // aquí se vuelven a mapear:
                //   Ok(QueueEvent::DownloadStarted(track)) => { let _ = output.send(PlaybackFeatureMessage::DownloadingStarted(track.id.clone())).await; }
                //   Ok(QueueEvent::DownloadFinished(track)) => { let _ = output.send(PlaybackFeatureMessage::DownloadingFinished(track.id.clone())).await; }
                Ok(_) => {}
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