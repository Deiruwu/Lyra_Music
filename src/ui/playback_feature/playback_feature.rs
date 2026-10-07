use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use futures::SinkExt;
use iced::widget::image::Handle;
use iced::{stream, Alignment, Element, Length, Subscription, Task, Theme};
use iced::widget::{container, column, row};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::model::audio_tech::PlayableTrack;
use crate::model::Track;
use crate::audio::manager::manager::TrackManager;
use crate::audio::track_event::{QueueEvent, TrackEvent};

use crate::ui::playback_feature::player::{Player, PlayerMessage, PlayerOutMessage, TrackLink};
use crate::ui::playback_feature::queue::queue_panel::{QueueMessage, QueueOutMessage, QueuePanel};
use crate::ui::playback_feature::theater::theater_panel::{TheaterMessage, TheaterOutMessage, TheaterPanel};
use crate::ui::playback_feature::volume::{Volume, VolumeMessage, VolumeOutMessage};
use crate::ui::styles::button as button_style;
use crate::ui::utils::async_thumbnail::AsyncThumbnail;
use crate::ui::views::catalog_store::CatalogStore;
use crate::ui::widgets::context_menu::{ContextMenu, ContextMenuEvent, ContextMenuItem};
use crate::ui::widgets::track_context_builder::{youtube_link, TrackContextAction, TrackContextMenuBuilder, TrackTool};
use crate::ui::assets::{spacing, typography};
use crate::ui::theme::theme;

/// Identidad de "sobre qué track" está abierto el menú contextual de
/// reproducción — el track actual (barra inferior) o una fila puntual de
/// la cola (por id de slot, estable aunque la cola se reordene).
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackContextTarget {
    CurrentTrack,
    QueueSlot(Uuid),
    HistorySlot(usize),
}

#[derive(Debug, Clone)]
pub enum PlaybackFeatureMessage {
    Player(PlayerMessage),
    Volume(VolumeMessage),
    Queue(QueueMessage),
    Theater(TheaterMessage),
    QueueChanged,
    /// Un track de la cola (pre-descarga proactiva o de emergencia, ver
    /// `DownloadWorker`) terminó de descargarse — trae el `Track` fresco,
    /// no el stub con el que arrancó la descarga.
    TrackDownloaded(Track),
    QueueThumbnailLoaded { key: String, bytes: Vec<u8> },
    SmallThumbnailLoaded { track_id: String, bytes: Vec<u8> },
    LargeThumbnailLoaded { track_id: String, bytes: Vec<u8> },
    /// Portada desenfocada y color predominante del track actual para el modo teatro.
    TheaterBackdropReady { track_id: String, backdrop: Option<Handle>, color: Option<iced::Color> },
    Play(PlayableTrack),
    ToggleTheaterMode,
    Tick,
    AnimationFrame(Instant),
    TrackContextMenuEvent(ContextMenuEvent<PlaybackContextTarget>),
    TrackContextAction(TrackContextAction, PlaybackContextTarget),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackOutMessage {
    ToggleTheaterMode,
    RequestToggleLike(Track),
    RequestOpenTrackLink(TrackLink),
    RequestAddToPlaylist { playlist_id: String, track: Track },
    RequestDeleteFromCatalog(String),
    RequestTrackTool(TrackTool, String),
    TrackNowPlaying(Track),
    /// Igual que `TrackNowPlaying`, pero para un track que la cola
    /// descargó en segundo plano (pre-descarga/emergencia) sin que
    /// necesariamente haya empezado a sonar todavía.
    TrackDownloaded(Track),
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
    track_context_menu: ContextMenu<PlaybackContextTarget>,
    track_context_menu_items: Vec<ContextMenuItem<TrackContextAction>>,
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
            queue_thumbnails: AsyncThumbnail::new(128),
            track_context_menu: ContextMenu::new(),
            track_context_menu_items: Vec::new(),
        }
    }

    pub fn subscription(&self, is_theater_visible: bool) -> Subscription<PlaybackFeatureMessage> {
        // El tick de 40ms es el que provoca el re-render periódico que
        // mantiene al día la barra de progreso del seek bar. Solo hace falta
        // mientras la posición se mueve sola: en pausa la barra está quieta,
        // y un seek manual emite su propia actualización de posición.
        let tick_sub = if self.manager.state.is_playing() {
            iced::time::every(Duration::from_millis(40))
                .map(|_| PlaybackFeatureMessage::Tick)
        } else {
            Subscription::none()
        };

        // La cola arranca su drag desde cualquier punto de la fila (sin
        // handle dedicado, ver QueueMessage::GlobalPressed): necesita el
        // press global además del release, mismo patrón que
        // sidebar_feature_v2 usa para PlaylistMessage::GlobalMousePress/Release.
        let global_mouse = iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                Some(PlaybackFeatureMessage::Queue(QueueMessage::GlobalPressed))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(PlaybackFeatureMessage::Queue(QueueMessage::DragReleased))
            }
            _ => None,
        });

        let mut subs = vec![
            Subscription::run(queue_events),
            Subscription::run(backend_events).map(PlaybackFeatureMessage::Player),
            tick_sub,
            global_mouse,
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
        playlists: &[(String, String)],
        catalog_store: &CatalogStore,
    ) -> (Task<PlaybackFeatureMessage>, PlaybackOutMessage) {
        let (task, out) = self.update_inner(msg, playlists, catalog_store);

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
        playlists: &[(String, String)],
        catalog_store: &CatalogStore,
    ) -> (Task<PlaybackFeatureMessage>, PlaybackOutMessage) {
        match msg {
            PlaybackFeatureMessage::Play(track) => {
                self.manager.enqueue(track.track);
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::QueueChanged => {
                let task = self.queue.sync_playback(
                    self.manager.get_current_track().map(|p| p.track.clone()),
                    self.manager.get_history_snapshot(),
                    self.manager.get_queue_snapshot(),
                );
                (task.map(PlaybackFeatureMessage::Queue), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::TrackDownloaded(track) => {
                (Task::none(), PlaybackOutMessage::TrackDownloaded(track))
            }

            PlaybackFeatureMessage::QueueThumbnailLoaded { key, bytes } => {
                self.queue_thumbnails.on_loaded(key, bytes);
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::SmallThumbnailLoaded { track_id, bytes } => {
                if self.current_track_id.as_deref() != Some(track_id.as_str()) {
                    return (Task::none(), PlaybackOutMessage::Idle);
                }
                if !bytes.is_empty() {
                    self.current_small_thumbnail = Some((track_id.clone(), Handle::from_bytes(bytes.clone())));
                }
                // La miniatura chica alcanza para el fondo desenfocado y el color del modo teatro.
                let task = Task::perform(
                    async move {
                        let backdrop = crate::ui::playback_feature::theater::theater_backdrop::blurred_backdrop(&bytes);
                        let color = crate::ui::cover_palette::dominant_color(&bytes);
                        (backdrop, color)
                    },
                    move |(backdrop, color)| PlaybackFeatureMessage::TheaterBackdropReady { track_id: track_id.clone(), backdrop, color },
                );
                (task, PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::TheaterBackdropReady { track_id, backdrop, color } => {
                if self.current_track_id.as_deref() == Some(track_id.as_str()) {
                    self.theater.set_backdrop(backdrop, color);
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

                let mut feature_out = PlaybackOutMessage::Idle;
                match out_msg {
                    QueueOutMessage::RequestPlay(index)    => self.manager.skip_to_index(index).unwrap(),
                    QueueOutMessage::RequestRemove(index)  => self.manager.remove_from_queue(index).unwrap(),
                    QueueOutMessage::RequestMove(from, to) => self.manager.move_in_queue(from, to).unwrap(),
                    QueueOutMessage::RequestOpenTrackLink(link) => {
                        feature_out = PlaybackOutMessage::RequestOpenTrackLink(link);
                    }
                    QueueOutMessage::RequestContextMenu(slot_id) => {
                        if let Some(track) = self.queue.find_slot_track(slot_id) {
                            let member_of = catalog_store.playlists_containing_track(&track.id);
                            let items = TrackContextMenuBuilder::new(catalog_store.is_liked(&track.id))
                                .with_playlists(playlists, None, &member_of)
                                .with_tools(track.file_path.is_some())
                                .with_delete()
                                .build();
                            self.track_context_menu.handle(ContextMenuEvent::RightClicked(PlaybackContextTarget::QueueSlot(slot_id)));
                            self.track_context_menu_items = items;
                        }
                    }
                    QueueOutMessage::RequestHistoryContextMenu(steps_back) => {
                        if let Some(track) = self.queue.history_track(steps_back) {
                            let member_of = catalog_store.playlists_containing_track(&track.id);
                            let items = TrackContextMenuBuilder::new(catalog_store.is_liked(&track.id))
                                .with_playlists(playlists, None, &member_of)
                                .with_tools(track.file_path.is_some())
                                .with_delete()
                                .build();
                            self.track_context_menu.handle(ContextMenuEvent::RightClicked(PlaybackContextTarget::HistorySlot(steps_back)));
                            self.track_context_menu_items = items;
                        }
                    }
                    QueueOutMessage::RequestCurrentContextMenu => {
                        if let Some(track) = self.queue.current_track() {
                            let member_of = catalog_store.playlists_containing_track(&track.id);
                            let items = TrackContextMenuBuilder::new(catalog_store.is_liked(&track.id))
                                .with_playlists(playlists, None, &member_of)
                                .with_tools(track.file_path.is_some())
                                .with_delete()
                                .build();
                            self.track_context_menu.handle(ContextMenuEvent::RightClicked(PlaybackContextTarget::CurrentTrack));
                            self.track_context_menu_items = items;
                        }
                    }
                    QueueOutMessage::RequestJumpBack(steps) => {
                        if let Err(e) = self.manager.skip_to_history_index(steps) {
                            eprintln!("Error: {}", e);
                        }
                    }
                    QueueOutMessage::RequestRemoveHistory(steps_back) => {
                        if let Err(e) = self.manager.remove_from_history(steps_back) {
                            eprintln!("Error: {}", e);
                        }
                    }
                    QueueOutMessage::RequestMoveToHistory(index) => {
                        if let Err(e) = self.manager.move_queue_to_history(index) {
                            eprintln!("Error: {}", e);
                        }
                    }
                    QueueOutMessage::RequestMoveToQueue(steps_back) => {
                        if let Err(e) = self.manager.move_history_to_queue(steps_back, true) {
                            eprintln!("Error: {}", e);
                        }
                    }
                    QueueOutMessage::RequestPlayQueuedOverCurrent(index) => {
                        if let Err(e) = self.manager.play_queued_over_current(index) {
                            eprintln!("Error: {}", e);
                        }
                    }
                    QueueOutMessage::RequestPlayHistoryOverCurrent(steps_back) => {
                        if let Err(e) = self.manager.play_history_over_current(steps_back) {
                            eprintln!("Error: {}", e);
                        }
                    }
                    QueueOutMessage::Idle                  => {}
                }

                (task.map(PlaybackFeatureMessage::Queue), feature_out)
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
                let mut feature_out = PlaybackOutMessage::Idle;

                if let PlayerMessage::BackendEvent(TrackEvent::Stopped) = msg {
                    let task = self.queue.sync_playback(
                        None,
                        self.manager.get_history_snapshot(),
                        self.manager.get_queue_snapshot(),
                    );
                    extra_tasks.push(task.map(PlaybackFeatureMessage::Queue));
                }

                if let PlayerMessage::BackendEvent(TrackEvent::TrackChanged(ref playable)) = msg {
                    feature_out = PlaybackOutMessage::TrackNowPlaying(playable.track.clone());
                    self.current_track_id = Some(playable.track.id.clone());
                    self.current_small_thumbnail = None; // reset inmediato al cambiar de track
                    self.current_large_thumbnail = None; // reset inmediato al cambiar de track

                    let queue_task = self.queue.sync_playback(
                        Some(playable.track.clone()),
                        self.manager.get_history_snapshot(),
                        self.manager.get_queue_snapshot(),
                    );
                    extra_tasks.push(queue_task.map(PlaybackFeatureMessage::Queue));

                    // Miniatura "chica" del track actual (patrón teatro pero
                    // small): se descarga directo y se guarda como tupla
                    // `(track_id, Handle)` para evitar condiciones de carrera
                    // si el track cambia mientras la imagen llega.
                    if playable.track.thumbnail_small.is_none() {
                        self.theater.set_backdrop(None, None);
                    }
                    if let Some(url) = playable.track.thumbnail_small.clone() {
                        let track_id = playable.track.id.clone();
                        extra_tasks.push(Task::perform(
                            crate::ui::utils::image::download_thumbnail(url, 128),
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
                            crate::ui::utils::image::download_thumbnail(url, 640),
                            move |result| {
                                let bytes = result.unwrap_or_default();
                                PlaybackFeatureMessage::LargeThumbnailLoaded { track_id: track_id.clone(), bytes }
                            },
                        ));
                    }
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
                    PlayerOutMessage::RequestSeek(pos) => {
                        let position = Duration::from_secs_f32(pos);
                        self.manager.seek(position);
                    }
                    PlayerOutMessage::RequestToggleLike(track_id) => {
                        if let Some(playable) = self.player.current_track.as_ref().filter(|p| p.track.id == track_id) {
                            feature_out = PlaybackOutMessage::RequestToggleLike(playable.track.clone());
                        }
                    }
                    PlayerOutMessage::RequestToggleShuffle => self.manager.toggle_shuffle(),
                    PlayerOutMessage::RequestCycleRepeat   => self.manager.cycle_repeat_mode(),
                    PlayerOutMessage::RequestToggleRadio   => self.manager.set_radio_enabled(!self.manager.is_radio_enabled()),
                    PlayerOutMessage::RequestOpenTrackLink(link) => {
                        feature_out = PlaybackOutMessage::RequestOpenTrackLink(link);
                    }
                    PlayerOutMessage::RequestContextMenu => {
                        if let Some(playable) = &self.player.current_track {
                            let member_of = catalog_store.playlists_containing_track(&playable.track.id);
                            let items = TrackContextMenuBuilder::new(catalog_store.is_liked(&playable.track.id))
                                .with_playlists(playlists, None, &member_of)
                                .with_tools(playable.track.file_path.is_some())
                                .with_delete()
                                .build();
                            self.track_context_menu.handle(ContextMenuEvent::RightClicked(PlaybackContextTarget::CurrentTrack));
                            self.track_context_menu_items = items;
                        }
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
                let (task, out_msg) = self.volume.update(msg, self.manager.get_volume());
                match out_msg {
                    VolumeOutMessage::RequestVolumeChange(vol) => {
                        self.manager.set_volume(vol);
                    }
                }

                (task.map(PlaybackFeatureMessage::Volume), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::TrackContextMenuEvent(event) => {
                if matches!(event, ContextMenuEvent::Dismissed) {
                    self.track_context_menu_items.clear();
                }
                self.track_context_menu.handle(event);
                (Task::none(), PlaybackOutMessage::Idle)
            }

            PlaybackFeatureMessage::TrackContextAction(action, target) => {
                self.track_context_menu.handle(ContextMenuEvent::Dismissed);
                self.track_context_menu_items.clear();
                self.handle_track_context_action(action, target)
            }
        }
    }

    /// Resuelve el `Track` sobre el que está abierto el menú contextual —
    /// el track actual (barra inferior) o una fila puntual de la cola.
    fn resolve_context_track(&self, target: &PlaybackContextTarget) -> Option<Track> {
        match target {
            PlaybackContextTarget::CurrentTrack => self.player.current_track.as_ref().map(|p| p.track.clone()),
            PlaybackContextTarget::QueueSlot(slot_id) => self.queue.find_slot_track(*slot_id),
            PlaybackContextTarget::HistorySlot(steps_back) => self.queue.history_track(*steps_back),
        }
    }

    /// Traduce una acción elegida en el menú contextual de track a la
    /// llamada real correspondiente. Calcado de
    /// `ViewCoordinator::handle_track_context_action`.
    fn handle_track_context_action(
        &mut self,
        action: TrackContextAction,
        target: PlaybackContextTarget,
    ) -> (Task<PlaybackFeatureMessage>, PlaybackOutMessage) {
        let Some(track) = self.resolve_context_track(&target) else {
            return (Task::none(), PlaybackOutMessage::Idle);
        };

        match action {
            TrackContextAction::PlayNow => {
                self.manager.play_context(vec![track], 0);
                (Task::none(), PlaybackOutMessage::Idle)
            }
            TrackContextAction::Enqueue => {
                self.manager.enqueue(track);
                (Task::none(), PlaybackOutMessage::Idle)
            }
            TrackContextAction::FrontEnqueue => {
                self.manager.enqueue_front(track);
                (Task::none(), PlaybackOutMessage::Idle)
            }
            TrackContextAction::StartRadio => {
                self.manager.start_radio(track);
                (Task::none(), PlaybackOutMessage::Idle)
            }
            TrackContextAction::CopyId => {
                (iced::clipboard::write(track.id), PlaybackOutMessage::Idle)
            }
            TrackContextAction::CopyYoutubeLink => {
                (iced::clipboard::write(youtube_link(&track.id)), PlaybackOutMessage::Idle)
            }
            TrackContextAction::Tool(tool) => {
                (Task::none(), PlaybackOutMessage::RequestTrackTool(tool, track.id))
            }
            TrackContextAction::ToggleLike => {
                (Task::none(), PlaybackOutMessage::RequestToggleLike(track))
            }
            TrackContextAction::AddToPlaylist(playlist_id) => {
                (Task::none(), PlaybackOutMessage::RequestAddToPlaylist { playlist_id, track })
            }
            TrackContextAction::DeleteFromCatalog => {
                (Task::none(), PlaybackOutMessage::RequestDeleteFromCatalog(track.id))
            }
            TrackContextAction::RemoveFromPlaylist => {
                (Task::none(), PlaybackOutMessage::Idle)
            }
        }
    }

    /// Overlay del menú contextual de track, si hay uno abierto — se
    /// compone en la capa raíz absoluta de `main.rs` (mismo motivo que el
    /// menú de playlist del sidebar: el anclaje viene de coordenadas de
    /// mouse globales, no de un `mouse_area` local).
    pub fn view_track_context_menu(&self) -> Option<Element<'_, PlaybackFeatureMessage>> {
        let open_target = self.track_context_menu.open_id();
        let (anchor, target) = self.track_context_menu.render_target(|_| open_target)?;

        Some(self.track_context_menu.view(
            anchor,
            self.track_context_menu_items.clone(),
            target,
            PlaybackFeatureMessage::TrackContextAction,
            PlaybackFeatureMessage::TrackContextMenuEvent(ContextMenuEvent::Dismissed),
            |sub| PlaybackFeatureMessage::TrackContextMenuEvent(ContextMenuEvent::SubmenuHovered(sub)),
        ))
    }

    /// Id del track actualmente en reproducción, si hay alguno. `main.rs`
    /// lo usa para consultar `CatalogStore::track_by_id` y así saber si
    /// está likeado antes de llamar a `view()`.
    pub fn current_track_id(&self) -> Option<&str> {
        self.current_track_id.as_deref()
    }

    /// Ver `ViewCoordinator::set_cursor`: ruta barata para el movimiento de
    /// cursor, sin pasar por `update()`.
    pub fn set_cursor(&mut self, position: iced::Point) {
        self.track_context_menu.handle(ContextMenuEvent::MouseMoved(position));
    }

    /// Vuelve a leer el `.lrc` si `track_id` es el que está sonando.
    pub fn lyrics_updated(&mut self, track_id: &str) -> Task<PlaybackFeatureMessage> {
        match &self.player.current_track {
            Some(playable) if playable.track.id == track_id => {
                self.theater.track_changed(playable).map(PlaybackFeatureMessage::Theater)
            }
            _ => Task::none(),
        }
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
            self.manager.is_radio_enabled(),
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

        // El seek bar va SOLO debajo de los controles de play (no de todo
        // el bloque de la derecha) — angosto, del ancho de esta columna.
        let play_column = column![play_center, seek_bar]
            .spacing(spacing::SP_6)
            .align_x(Alignment::Center);

        let play_controller = row![
            container(current_track).width(Length::FillPortion(2)),
            container(play_column)
                .width(Length::FillPortion(3))
                .align_x(Alignment::Center),
            container(right_view)
                .width(Length::FillPortion(2))
                .align_x(Alignment::End),
        ]
            .width(Length::Fill)
            .align_y(Alignment::Center);

        container(play_controller)
            .width(Length::Fill)
            .padding(spacing::SP_10)
            .align_x(Alignment::Center)
            .style(|_theme: &Theme| container::Style {
                background: Some(theme().surface.base.into()),
                text_color: Some(theme().content.primary),
                ..Default::default()
            })
            .into()
    }

    pub fn is_queue_shown(&self) -> bool {
        self.queue.show
    }

    pub fn hide_queue(&mut self) {
        self.queue.hide();
    }

    /// Expande la columna de la cola para el panel de agregar canciones (o la libera).
    pub fn set_queue_forced_open(&mut self, forced: bool) {
        self.queue.set_forced_open(forced);
    }

    pub fn queue_width(&self) -> f32 {
        self.queue.width()
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

        button(text(icon).font(crate::ui::assets::fonts::JETBRAINS_MONO).size(typography::TEXT_18))
            .style(button_style::minimal)
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
                Ok(QueueEvent::DownloadFinished(track)) => {
                    let _ = output.send(PlaybackFeatureMessage::TrackDownloaded((*track).clone())).await;
                }
                // ── FEAT FUTURO: spinner de descarga visible en cola ───────────
                // Cuando se reintroduzca, mapear acá:
                //   Ok(QueueEvent::DownloadStarted(track)) => { let _ = output.send(PlaybackFeatureMessage::DownloadingStarted(track.id.clone())).await; }
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