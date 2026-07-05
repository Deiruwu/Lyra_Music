use std::sync::{Arc, OnceLock};
use std::time::Duration;
use futures::SinkExt;
use iced::{stream, Alignment, Color, Element, Length, Subscription, Task, Theme};
use iced::widget::{container, column, row};
use tokio::sync::broadcast;
use crate::model::audio_tech::PlayableTrack;
use crate::audio::mananger::manager::TrackManager;
use crate::audio::track_event::{QueueEvent, TrackEvent};

use crate::ui::playback_feature::lyrics::lyrics_panel::{LyricsPanel, LyricsMessage, LyricsOutMessage};
use crate::ui::playback_feature::player::{Player, PlayerMessage, PlayerOutMessage};
use crate::ui::playback_feature::queue::queue_panel::{QueueMessage, QueueOutMessage, QueuePanel};
use crate::ui::playback_feature::volume::{Volume, VolumeMessage, VolumeOutMessage};
use crate::ui::utils::thumbnail_cache::{thumb_key, ThumbnailCache};

#[derive(Debug, Clone)]
pub enum PlaybackFeatureMessage {
    Player(PlayerMessage),
    Volume(VolumeMessage),
    Queue(QueueMessage),
    Lyrics(LyricsMessage),
    QueueChanged,
    DownloadingStarted(String),
    DownloadingFinished(String),
    ThumbnailColorLoaded { key: String, bytes: Vec<u8> },
    Play(PlayableTrack),
    Tick,
}

pub struct PlaybackFeature {
    manager: Arc<TrackManager>,
    queue: QueuePanel,
    player: Player,
    volume: Volume,
    lyrics: LyricsPanel,
    spinner_frame: u8,
    is_predownloading: bool,
    /// Id del track que el DownloadWorker está bajando ahora mismo, si
    /// alguno. Antes se asumía "siempre es el índice 0 de la cola", pero
    /// eso deja de ser cierto en cuanto el usuario reordena la cola con
    /// drag & drop: el índice 0 visual ya no es necesariamente el track
    /// real en descarga.
    downloading_track_id: Option<String>,
}

impl PlaybackFeature {
    pub fn new(manager: Arc<TrackManager>) -> Self {
        QUEUE_TX.set(manager.queue_tx.clone()).ok();
        TX.set(manager.event_tx.clone()).ok();
        Self {
            queue: QueuePanel::default(),
            player: Player::default(),
            volume: Volume::default(),
            lyrics: LyricsPanel::default(),
            manager,
            spinner_frame: 0,
            is_predownloading: false,
            downloading_track_id: None,
        }
    }

    pub fn subscription(&self) -> Subscription<PlaybackFeatureMessage> {
        let tick_sub = iced::time::every(Duration::from_millis(40))
            .map(|_| PlaybackFeatureMessage::Tick);

        // Suelta cualquier drag en curso sin importar dónde esté el
        // cursor al momento del release — necesario porque el panel de
        // cola es angosto y es normal que el mouse salga de su área
        // mientras arrastras. Sin esto, self.queue.drag queda "pegado"
        // y el ghost sigue las coordenadas del mouse para siempre.
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

        // Solo pedimos frames del compositor mientras alguna fila de la
        // cola está a medio animar; si no, esta subscription desaparece
        // sola y dejamos de gastar ciclos en cada refresh del monitor.
        if self.queue.is_animating() {
            subs.push(
                iced::window::frames()
                    .map(|instant| PlaybackFeatureMessage::Queue(QueueMessage::AnimationFrame(instant))),
            );
        }

        // Igual que con la cola: solo pedimos frames del compositor
        // mientras una línea de la letra está en transición de fade/slide.
        if self.lyrics.is_animating(std::time::Instant::now()) {
            subs.push(
                iced::window::frames()
                    .map(|instant| PlaybackFeatureMessage::Lyrics(LyricsMessage::AnimationFrame(instant))),
            );
        }

        Subscription::batch(subs)
    }

    /// True si hay cualquier descarga activa: emergencia (status==4)
    /// o pre-descarga proactiva del worker.
    fn is_downloading(&self) -> bool {
        self.manager.state.is_downloading() || self.is_predownloading
    }

    pub fn update(
        &mut self,
        msg: PlaybackFeatureMessage,
        thumbnails: &mut ThumbnailCache,
    ) -> Task<PlaybackFeatureMessage> {
        match msg {
            PlaybackFeatureMessage::Play(track) => {
                self.manager.enqueue(track.track);
                Task::none()
            }

            PlaybackFeatureMessage::QueueChanged => {
                let tracks = self.manager.get_queue_snapshot();
                self.queue.queue_update(tracks.clone());

                let tasks: Vec<Task<_>> = tracks.into_iter().filter_map(|t| {
                    let url = t.thumbnail_small.clone()?;
                    let key = thumb_key(&t);
                    thumbnails.request_color(key, url, |key, bytes| {
                        PlaybackFeatureMessage::ThumbnailColorLoaded { key, bytes }
                    })
                }).collect();

                Task::batch(tasks)
            }

            PlaybackFeatureMessage::DownloadingStarted(track_id) => {
                self.is_predownloading = true;
                self.downloading_track_id = Some(track_id);
                Task::none()
            }

            PlaybackFeatureMessage::DownloadingFinished(track_id) => {
                self.is_predownloading = false;
                // Solo limpiamos si coincide con la que teníamos guardada;
                // si por alguna condición de carrera llega un Finished de
                // un id viejo después de que ya empezó otra descarga, no
                // queremos borrar el id correcto por error.
                if self.downloading_track_id.as_deref() == Some(track_id.as_str()) {
                    self.downloading_track_id = None;
                }
                Task::none()
            }

            PlaybackFeatureMessage::ThumbnailColorLoaded { key, bytes } => {
                thumbnails.insert_color(key, bytes);
                Task::none()
            }

            PlaybackFeatureMessage::Tick => {
                if self.is_downloading() {
                    self.spinner_frame = (self.spinner_frame + 1) % 6;
                    let _ = self.queue.update(QueueMessage::Tick);
                }

                let position = self.manager.get_position();
                let (_, _) = self.lyrics.update(LyricsMessage::PositionUpdated(position));

                Task::none()
            }

            PlaybackFeatureMessage::Queue(msg) => {
                let (task, out_msg) = self.queue.update(msg);

                match out_msg {
                    QueueOutMessage::RequestPlay(index)    => self.manager.skip_to_index(index).unwrap(),
                    QueueOutMessage::RequestRemove(index)  => self.manager.remove_from_queue(index).unwrap(),
                    QueueOutMessage::RequestMove(from, to) => self.manager.move_in_queue(from, to).unwrap(),
                    QueueOutMessage::Idle                  => {}
                }

                task.map(PlaybackFeatureMessage::Queue)
            }

            PlaybackFeatureMessage::Player(msg) => {
                let mut extra_task = Task::none();
                let mut lyrics_task = Task::none();

                if let PlayerMessage::BackendEvent(TrackEvent::TrackChanged(ref playable)) = msg {
                    let key = thumb_key(&playable.track);
                    if let Some(url) = playable.track.thumbnail_small.clone() {
                        if let Some(t) = thumbnails.request_color(key, url, |key, bytes| {
                            PlaybackFeatureMessage::ThumbnailColorLoaded { key, bytes }
                        }) {
                            extra_task = t;
                        }
                    }

                    // Nuevo track sonando: avisamos al panel de letras
                    // para que busque y cargue su .lrc correspondiente.
                    let (t, _out) = self.lyrics.update(LyricsMessage::TrackChanged(Arc::clone(playable)));
                    lyrics_task = t.map(PlaybackFeatureMessage::Lyrics);
                }

                let (task, out_msg) = self.player.update(msg);

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
                    PlayerOutMessage::RequestSeek(pos) => self.manager.seek(Duration::from_secs_f32(pos)),
                    PlayerOutMessage::Idle             => {}
                }

                Task::batch(vec![
                    task.map(PlaybackFeatureMessage::Player),
                    extra_task,
                    lyrics_task,
                ])
            }

            PlaybackFeatureMessage::Volume(msg) => {
                let (task, out_msg) = self.volume.update(msg);

                match out_msg {
                    VolumeOutMessage::RequestVolumeChange(vol) => self.manager.set_volume(vol),
                }

                task.map(PlaybackFeatureMessage::Volume)
            }

            PlaybackFeatureMessage::Lyrics(msg) => {
                let (task, out_msg) = self.lyrics.update(msg);

                if let LyricsOutMessage::RequestSeek(timestamp) = out_msg {
                    self.manager.seek(timestamp);
                }

                task.map(PlaybackFeatureMessage::Lyrics)
            }
        }
    }

    pub fn view(&self, thumbnails: &ThumbnailCache) -> Element<'_, PlaybackFeatureMessage> {
        let current_position = self.manager.get_position().as_secs_f32();
        let vol              = self.manager.get_volume();
        let has_track        = self.player.has_track();
        let has_history      = self.manager.history_len() != 0;
        let is_downloading   = self.is_downloading();

        let current_thumbnail = self.player.current_track.as_ref()
            .and_then(|p| thumbnails.peek_color(&thumb_key(&p.track)));

        let current_track = self.player
            .view_current_play(current_thumbnail, is_downloading, self.spinner_frame)
            .map(PlaybackFeatureMessage::Player);

        let play_center  = self.player.view(self.manager.state.is_playing(), has_track, has_history).map(PlaybackFeatureMessage::Player);
        let seek_bar     = self.player.view_seek_bar(current_position).map(PlaybackFeatureMessage::Player);
        let vol_view     = self.volume.view(vol).map(PlaybackFeatureMessage::Volume);
        let queue_toggle = self.queue.view_toggle_button().map(PlaybackFeatureMessage::Queue);

        let right_view = row![queue_toggle, vol_view].align_y(Alignment::Center);

        let play_controller = row![
            container(current_track).width(Length::FillPortion(1)),
            container(play_center)
                .width(Length::FillPortion(4))
                .align_x(Alignment::Center),
            container(right_view)
                .width(Length::FillPortion(1))
                .align_x(Alignment::End),
        ]
            .width(Length::Fill)
            .align_y(Alignment::Center);

        // La letra ahora vive en su propio panel grande (ver view_lyrics),
        // así que aquí solo queda la seek bar y los controles.
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

    /// Panel grande de letras, pensado para ocupar el espacio central
    /// vacío del layout principal (antes un placeholder sin contenido).
    pub fn view_lyrics(&self) -> Element<'_, PlaybackFeatureMessage> {
        self.lyrics.view().map(PlaybackFeatureMessage::Lyrics)
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
                Ok(QueueEvent::QueueChanged) => {
                    let _ = output.send(PlaybackFeatureMessage::QueueChanged).await;
                }
                Ok(QueueEvent::DownloadStarted(track)) => {
                    let _ = output.send(PlaybackFeatureMessage::DownloadingStarted(track.id.clone())).await;
                }
                Ok(QueueEvent::DownloadFinished(track)) => {
                    let _ = output.send(PlaybackFeatureMessage::DownloadingFinished(track.id.clone())).await;
                }
                // DownloadRequired lo maneja el worker, la UI no necesita reaccionar.
                Ok(QueueEvent::DownloadRequired(_))         => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed)    => break,
            }
        }
    })
}

fn backend_events() -> impl futures::Stream<Item = PlayerMessage> {
    let tx = TX.get().unwrap().clone();
    stream::channel(100, async move |mut output| {
        let mut rx = tx.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => { let _ = output.send(PlayerMessage::BackendEvent(event)).await; }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed)    => break,
            }
        }
    })
}