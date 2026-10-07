mod microservices;
mod model;
mod audio;
mod ui;
pub mod tray;
pub mod db;
pub mod utils;
mod settings;
mod session;
mod local_server;

use std::sync::{Arc, LazyLock, OnceLock};
use futures::SinkExt;
use std::sync::atomic::Ordering;
use iced::theme::Palette;
use iced::{border, window, Alignment, Background, Border, Element, Length, Padding, Theme};
use iced::widget::{column, container, row, space, stack};

use crate::audio::discord::DiscordPresence;
use crate::audio::download_daemon::DownloadWorker;
use crate::audio::engine::AudioEngine;
use audio::manager::manager::{PlaybackOrigin, TrackManager};
use crate::audio::mpris::MprisServer;
use crate::audio::radio_daemon::RadioWorker;
use crate::db::db::init_db;
use crate::db::playlist_manager::PlaylistManager;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::db::artist_tag_manager::ArtistTagManager;
use crate::audio::play_history_recorder::PlayHistoryRecorder;
use crate::microservices::client::MicroserviceClient;
use crate::settings::{AppSettings, ServerMode};
use crate::session::PlaybackSession;
use crate::model::Mix;
use crate::audio::track_event::TrackEvent;
use crate::ui::playback_feature::player::PlayerMessage;
use crate::tray::TrayFlags;

use crate::ui::assets::radii;
use crate::ui::library_browser_feature::library_browser_feature::{
    LibraryBrowserFeature, LibraryBrowserLocation, LibraryBrowserMessage, LibraryBrowserOutMessage,
};
use crate::ui::download_feature::{DownloadFeature, DownloadFeatureMessage, DownloadFeatureOutMessage};
use crate::ui::playback_feature::player::TrackLink;
use crate::ui::playback_feature::playback_feature::{PlaybackFeature, PlaybackFeatureMessage, PlaybackOutMessage};
use crate::ui::search_feature::search_bar::SearchMessage;
use crate::ui::search_feature::search_feature::{SearchFeature, SearchFeatureMessage, SearchFeatureOutMessage};
use crate::ui::sidebar_feature::sidebar_feature_v2::{
    SidebarFeatureV2 as SidebarFeature, SidebarMessage as SidebarFeatureMessage, SidebarOutMessage
};
use crate::ui::views::view_coordinator::{playlist_pairs, ActiveRoute, CoordinatorMessage};
use crate::ui::views::view_data::NavId;
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::views::catalog_store::CatalogStoreMessage;
use crate::ui::widgets::track_context_builder::{youtube_link, TrackContextAction, TrackContextMenuBuilder, TrackTool};
use crate::ui::widgets::context_menu::ContextMenuEvent;
use crate::ui::widgets::selection_state::SelectionStep;
use crate::ui::widgets::corner_mask::{corner_ring, CORNER_RING_WIDTH};
use crate::ui::settings_view::{SettingsMessage, SettingsView};
use crate::ui::assets::spacing;
use crate::ui::theme::theme;


static TRAY_FLAGS: OnceLock<Arc<TrayFlags>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum AppMessage {
    SearchFeature(SearchFeatureMessage),
    /// Tuerca de la barra superior: abre o cierra Ajustes.
    ToggleSettings,
    Settings(SettingsMessage),
    PlaybackFeature(PlaybackFeatureMessage),
    SidebarFeature(SidebarFeatureMessage),
    WindowOpened(window::Id),
    CloseRequested(window::Id),
    ShowWindow,
    Quit,
    LibraryBrowser(LibraryBrowserMessage),
    NavigateBack,
    NavigateForward,
    EscapePressed,
    /// Ctrl+F: filtro de la página activa o, si no tiene, la búsqueda.
    FindPressed,
    /// Espacio: pausa/reanuda.
    TogglePlayback,
    /// Flechas / RePág / AvPág: mueve la selección de la lista visible (`extend` con Shift).
    MoveSelection { step: SelectionStep, extend: bool },
    /// Enter: reproduce desde la canción seleccionada.
    PlaySelection,
    AutosaveTick,
    TrayWake,
    CursorMoved(iced::Point),
    WindowResized(iced::Size),
    DownloadFeature(DownloadFeatureMessage),
}

struct App {
    _engine: AudioEngine,
    manager: Arc<TrackManager>,
    search_feature: SearchFeature,
    /// Ajustes ocupa el panel central (detrás solo del modo teatro).
    settings_open: bool,
    settings_view: SettingsView,
    playback_feature: PlaybackFeature,
    download_feature: DownloadFeature,
    sidebar_feature: SidebarFeature,
    library_browser: LibraryBrowserFeature,
    view_thumbnails: ThumbnailCache,
    tray_flags: Arc<TrayFlags>,
    main_window: Option<window::Id>,
    is_theater_mode: bool,
    nav_back_stack: Vec<NavEntry>,
    nav_forward_stack: Vec<NavEntry>,
    is_replaying_history: bool,
    last_saved_settings: AppSettings,
    /// Huella de la última sesión guardada (ver `session_fingerprint`).
    last_session_fingerprint: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum NavEntry {
    Content(ActiveRoute),
    LibraryArtist(String),
    LibraryAlbum(String),
    LibraryMix(Mix),
}

const MAX_NAV_HISTORY: usize = 3;
/// Lado del avatar de la barra superior y grosor de su anillo.
const AVATAR_SIZE: f32 = 26.0;
const AVATAR_RING_WIDTH: f32 = 1.5;

fn push_capped(stack: &mut Vec<NavEntry>, entry: NavEntry) {
    stack.push(entry);
    if stack.len() > MAX_NAV_HISTORY {
        stack.remove(0);
    }
}

impl App {
    pub fn init() -> (Self, iced::Task<AppMessage>) {
        let settings = AppSettings::load();

        let (manager, engine) = TrackManager::new()
            .expect("Fallo fatal al inicializar el hardware de audio");

        let manager    = Arc::new(manager);
        manager.set_volume(settings.volume);
        manager.set_repeat_mode(settings.repeat_mode);
        manager.set_shuffle_enabled(settings.shuffle_enabled);
        manager.set_radio_enabled(settings.radio_enabled);
        manager.set_crossfade(if settings.crossfade_enabled { settings.crossfade_seconds } else { 0.0 });

        let session = PlaybackSession::load();
        let restored_playlist = match &session.origin {
            Some(PlaybackOrigin::Playlist(id)) => Some(id.clone()),
            _ => None,
        };
        if let Some(origin) = session.origin {
            manager.set_playback_origin(origin);
        }
        let restored_track = manager.restore_session(
            session.current,
            std::time::Duration::from_millis(session.position_ms),
            session.queue,
            session.history,
        );
        if let Some(playlist_id) = restored_playlist {
            manager.relink_playlist(&playlist_id);
        }
        let tray_flags = tray::spawn_tray(Arc::clone(&manager));
        TRAY_FLAGS.set(Arc::clone(&tray_flags)).ok();

        let client = Arc::new(match settings.server.mode {
            ServerMode::Local => MicroserviceClient::new("127.0.0.1", local_server::LOCAL_PORT),
            ServerMode::Remote => MicroserviceClient::new(&settings.server.host, settings.server.port),
        });

        MprisServer::spawn(Arc::clone(&manager));
        DiscordPresence::spawn(Arc::clone(&manager));
        DownloadWorker::new(Arc::clone(&manager), Arc::clone(&client)).spawn();

        // Guardamos una copia para el sidebar/explorer antes de que RadioWorker consuma `client`.
        let sidebar_client = Arc::clone(&client);
        // Copia (no Arc, MicroserviceClient ya es Clone) para LibraryBrowserFeature.
        let library_browser_client = client.as_ref().clone();
        let library_browser_manager = Arc::clone(&manager);
        let download_feature = DownloadFeature::new(Arc::clone(&client));
        let client_for_search = client.as_ref().clone();

        let radio = RadioWorker::new(Arc::clone(&manager), client).spawn();
        radio.set_queue_target(15);

        let (window_id, open_task) = window::open(window::Settings {
            size: iced::Size::new(800.0, 600.0),
            exit_on_close_request: false,
            ..Default::default()
        });

        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://music_center.db".into());

        let init_local_db = async {
            let pool = init_db(&database_url)
                .await
                .expect("Fallo fatal al inicializar la base de datos local");
            let playlist_manager = PlaylistManager::new(pool.clone())
                .await
                .expect("Fallo fatal al inicializar PlaylistManager");
            let play_history_manager = PlayHistoryManager::new(pool.clone());
            let followed_artist_manager = FollowedArtistManager::new(pool.clone());
            let artist_tag_manager = ArtistTagManager::new(pool);
            (playlist_manager, play_history_manager, followed_artist_manager, artist_tag_manager)
        };

        let (playlist_manager, play_history_manager, followed_artist_manager, artist_tag_manager) =
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => tokio::task::block_in_place(|| handle.block_on(init_local_db)),
                Err(_) => {
                    let rt = tokio::runtime::Runtime::new()
                        .expect("No se pudo crear runtime temporal para inicializar la BD");
                    rt.block_on(init_local_db)
                }
            };
        let playlist_manager = Arc::new(playlist_manager);
        let play_history_manager = Arc::new(play_history_manager);
        let followed_artist_manager = Arc::new(followed_artist_manager);
        let artist_tag_manager = Arc::new(artist_tag_manager);

        PlayHistoryRecorder::spawn(Arc::clone(&manager), Arc::clone(&play_history_manager), sidebar_client.as_ref().clone());

        let (mut sidebar_feature, sidebar_task) = SidebarFeature::new(
            sidebar_client,
            Arc::clone(&playlist_manager),
            Arc::clone(&manager),
            Arc::clone(&play_history_manager),
            Arc::clone(&followed_artist_manager),
            artist_tag_manager,
        );
        sidebar_feature.set_expanded_immediate(settings.sidebar_expanded);
        sidebar_feature.coordinator.explorer_view.show_play_stats = settings.explorer_play_stats;
        sidebar_feature.coordinator.remix_view.enabled = settings.remix_playlists.clone();
        if let Some(accent) = settings.accent_color.as_deref().and_then(crate::ui::theme::from_hex) {
            crate::ui::theme::set_accent(accent);
        }

        let mut search_feature = SearchFeature::new(client_for_search);
        search_feature.input.filter = settings.search_filter;

        let mut app = Self {
            _engine: engine,
            search_feature,
            settings_open: false,
            settings_view: SettingsView::new(settings.crossfade_enabled, settings.crossfade_seconds, settings.server.clone(), settings.profile_name.clone()),
            playback_feature: PlaybackFeature::new(Arc::clone(&manager)),
            download_feature,
            sidebar_feature,
            library_browser: LibraryBrowserFeature::new(library_browser_client, library_browser_manager, Arc::clone(&followed_artist_manager)),
            view_thumbnails: ThumbnailCache::new(100, 50),
            tray_flags,
            main_window: Some(window_id),
            manager,
            is_theater_mode: false,
            nav_back_stack: Vec::new(),
            nav_forward_stack: Vec::new(),
            is_replaying_history: false,
            last_saved_settings: settings,
            last_session_fingerprint: 0,
        };
        app.last_session_fingerprint = session_fingerprint(&app.manager);

        // La UI se suscribe a los eventos del manager después de `init`, así
        // que la sesión restaurada se le entrega directo.
        let restore_task = match restored_track {
            Some(playable) => iced::Task::done(AppMessage::PlaybackFeature(PlaybackFeatureMessage::Player(
                PlayerMessage::BackendEvent(TrackEvent::TrackChanged(playable)),
            ))),
            None => iced::Task::done(AppMessage::PlaybackFeature(PlaybackFeatureMessage::QueueChanged)),
        };

        let init_task = iced::Task::batch(vec![
            open_task.map(AppMessage::WindowOpened),
            sidebar_task.map(AppMessage::SidebarFeature),
            restore_task,
        ]);

        (app, init_task)
    }

    pub fn update(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        let task = self.apply(message);
        self.sidebar_feature.coordinator.sync_playback_link();
        task
    }

    fn apply(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        // Ctrl/Shift también cuentan para la selección múltiple de artista, álbum y mezcla.
        if let AppMessage::SidebarFeature(SidebarFeatureMessage::Content(CoordinatorMessage::KeybindsChanged(modifiers))) = &message {
            self.library_browser.set_modifiers(*modifiers);
        }

        // Al apretar, el coordinator decide qué se arrastra: su lista o, si la tapa la biblioteca, la canción bajo el mouse ahí.
        if matches!(message, AppMessage::SidebarFeature(SidebarFeatureMessage::GlobalLeftPressed)) {
            let covered = self.is_theater_mode || self.search_feature.input.is_open;
            let content_visible = !covered && !self.library_browser.is_active();
            let external = (!covered && self.library_browser.is_active())
                .then(|| self.library_browser.drag_candidate(&self.sidebar_feature.coordinator.catalog_store))
                .flatten();
            self.sidebar_feature.coordinator.set_drag_context(content_visible, external);
        }

        let side_panel_was_open = self.is_side_panel_visible();
        let queue_was_shown = self.playback_feature.is_queue_shown();

        let task = self.update_inner(message);

        // La cola y los paneles laterales (agregar canciones, playlists de Remix) comparten la columna: abrir uno cierra el otro.
        if self.is_side_panel_visible() && !side_panel_was_open {
            self.playback_feature.hide_queue();
        } else if self.playback_feature.is_queue_shown() && !queue_was_shown {
            self.sidebar_feature.coordinator.close_side_panel();
        }
        self.playback_feature.set_queue_forced_open(self.is_side_panel_visible());

        // Letreros de "agregadas a la playlist", vengan de donde vengan.
        let notices: Vec<_> = self.sidebar_feature.coordinator.take_add_notices();
        let notice_tasks: Vec<_> = notices
            .iter()
            .map(|notice| {
                self.download_feature
                    .notify_playlist_add(&mut self.view_thumbnails, &notice.playlist_name, notice.added, notice.already, &notice.sample)
                    .map(AppMessage::DownloadFeature)
            })
            .collect();
        if notice_tasks.is_empty() { task } else { iced::Task::batch(std::iter::once(task).chain(notice_tasks)) }
    }

    /// Un panel lateral está ocupando la columna de la cola.
    fn is_side_panel_visible(&self) -> bool {
        !self.is_theater_mode
            && !self.settings_open
            && !self.library_browser.is_active()
            && self.sidebar_feature.coordinator.is_side_panel_open()
    }

    fn update_inner(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        match message {
            AppMessage::WindowOpened(id) => window::size(id).map(AppMessage::WindowResized),

            AppMessage::WindowResized(size) => {
                self.search_feature.set_viewport(size);
                self.library_browser.set_viewport(size);
                iced::Task::batch([
                    iced::Task::done(AppMessage::SidebarFeature(SidebarFeatureMessage::GlobalWindowResized(size))),
                    iced::Task::done(AppMessage::PlaybackFeature(PlaybackFeatureMessage::TrackContextMenuEvent(
                        ContextMenuEvent::ViewportResized(size),
                    ))),
                ])
            }

            AppMessage::CloseRequested(id) => {
                self.main_window = None;
                window::close(id)
            }

            AppMessage::ShowWindow => {
                match self.main_window {
                    Some(id) => window::gain_focus(id),
                    None => {
                        let (new_id, open_task) = window::open(window::Settings {
                            size: iced::Size::new(800.0, 600.0),
                            exit_on_close_request: false,
                            ..Default::default()
                        });
                        self.main_window = Some(new_id);
                        open_task.map(AppMessage::WindowOpened)
                    }
                }
            }

            AppMessage::Quit => {
                let _ = self.current_settings().save();
                let _ = self.current_session().save();
                local_server::shutdown();
                iced::exit()
            }

            AppMessage::AutosaveTick => {
                // SIGINT/SIGTERM solo marcan el flag (no pueden despertar a la UI).
                if self.tray_flags.quit.load(Ordering::Relaxed) {
                    return iced::Task::done(AppMessage::Quit);
                }

                let current = self.current_settings();
                if current != self.last_saved_settings {
                    let _ = current.save();
                    self.last_saved_settings = current;
                }

                let fingerprint = session_fingerprint(&self.manager);
                if fingerprint != self.last_session_fingerprint {
                    self.last_session_fingerprint = fingerprint;
                    let session = self.current_session();
                    std::thread::spawn(move || {
                        if let Err(e) = session.save() {
                            eprintln!("[SESSION] No se pudo guardar la sesión: {e}");
                        }
                    });
                }
                iced::Task::none()
            }

            AppMessage::CursorMoved(position) => {
                self.search_feature.set_cursor(position);
                self.library_browser.set_cursor(position);
                self.playback_feature.set_cursor(position);
                self.sidebar_feature.set_cursor(position);
                iced::Task::none()
            }

            AppMessage::TrayWake => {
                if self.tray_flags.quit.load(Ordering::Relaxed) {
                    return iced::Task::done(AppMessage::Quit);
                }
                if self.tray_flags.show_window.load(Ordering::Relaxed) {
                    self.tray_flags.show_window.store(false, Ordering::Relaxed);
                    return iced::Task::done(AppMessage::ShowWindow);
                }
                iced::Task::none()
            }

            AppMessage::PlaybackFeature(PlaybackFeatureMessage::Tick) => {
                let position = self.manager.get_position();
                let position_task = self.playback_feature.position_updated(position);

                let playlists = playlist_pairs(self.sidebar_feature.coordinator.playlists_metadata());
                let (tick_task, _out) = self.playback_feature
                    .update(PlaybackFeatureMessage::Tick, &playlists, &self.sidebar_feature.coordinator.catalog_store);

                iced::Task::batch(vec![
                    position_task.map(AppMessage::PlaybackFeature),
                    tick_task.map(AppMessage::PlaybackFeature),
                ])
            }

            AppMessage::PlaybackFeature(msg) => {
                let playlists = playlist_pairs(self.sidebar_feature.coordinator.playlists_metadata());
                let (task, out_msg) = self.playback_feature.update(msg, &playlists, &self.sidebar_feature.coordinator.catalog_store);

                let like_task = match out_msg {
                    PlaybackOutMessage::ToggleTheaterMode => {
                        self.is_theater_mode = !self.is_theater_mode;
                        if self.is_theater_mode {
                            self.settings_open = false;
                        }

                        // Salir de modo teatro reconstruye view_content() desde
                        // cero (ver resync_active_scroll) — sin esto el
                        // scrollable de la vista de fondo vuelve a offset 0.
                        if !self.is_theater_mode && !self.library_browser.is_active() {
                            self.sidebar_feature.coordinator.resync_active_scroll().map(|m| {
                                AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m))
                            })
                        } else {
                            iced::Task::none()
                        }
                    }
                    PlaybackOutMessage::RequestToggleLike(track) => {
                        self.sidebar_feature
                            .coordinator
                            .catalog_store
                            .toggle_like_track(track)
                            .map(|catalog_msg| {
                                AppMessage::SidebarFeature(SidebarFeatureMessage::Content(
                                    CoordinatorMessage::Catalog(catalog_msg)
                                ))
                            })
                    }
                    PlaybackOutMessage::RequestOpenTrackLink(link) => {
                        iced::Task::done(AppMessage::LibraryBrowser(match link {
                            TrackLink::Artist(id) => LibraryBrowserMessage::OpenArtist(id),
                            TrackLink::Album(id) => LibraryBrowserMessage::OpenAlbum(id),
                        }))
                    }
                    PlaybackOutMessage::RequestAddToPlaylist { playlist_id, track } => self
                        .sidebar_feature
                        .coordinator
                        .add_tracks_to_playlist(&playlist_id, vec![track])
                        .map(|m| AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m))),
                    PlaybackOutMessage::RequestDeleteFromCatalog(track_id) => {
                        iced::Task::done(AppMessage::SidebarFeature(SidebarFeatureMessage::Content(
                            CoordinatorMessage::RequestDeleteTracks(vec![track_id])
                        )))
                    }
                    PlaybackOutMessage::RequestTrackTool(tool, track_id) => self.run_track_tool(tool, &track_id),
                    PlaybackOutMessage::TrackNowPlaying(track) => {
                        self.library_browser.patch_track(&track);
                        iced::Task::done(AppMessage::SidebarFeature(SidebarFeatureMessage::Content(
                            CoordinatorMessage::Catalog(CatalogStoreMessage::TrackDownloadedAndCached(track))
                        )))
                    }
                    PlaybackOutMessage::TrackDownloaded(track) => {
                        self.library_browser.patch_track(&track);
                        iced::Task::done(AppMessage::SidebarFeature(SidebarFeatureMessage::Content(
                            CoordinatorMessage::Catalog(CatalogStoreMessage::TrackDownloadedAndCached(track))
                        )))
                    }
                    PlaybackOutMessage::Idle => iced::Task::none(),
                };

                iced::Task::batch(vec![
                    task.map(AppMessage::PlaybackFeature),
                    like_task,
                ])
            }

            AppMessage::SidebarFeature(msg) => {
                if matches!(msg, SidebarFeatureMessage::SelectPlaylist(_)) && self.sidebar_feature.is_playlist_drag_click() {
                    return iced::Task::none();
                }

                // Propaga tracks actualizados a la vista de artista/álbum abierta.
                if let SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(
                    CatalogStoreMessage::TrackDownloadedAndCached(track)
                    | CatalogStoreMessage::TrackToolFinished(_, _, Ok(track)),
                )) = &msg {
                    self.library_browser.patch_track(track);
                }

                let leaving = self.current_nav_entry();
                let was_theater = self.is_theater_mode;

                let is_nav_select = matches!(msg, SidebarFeatureMessage::SelectNav(_) | SidebarFeatureMessage::SelectPlaylist(_));
                let closing_browser = self.library_browser.is_active() && is_nav_select;
                if closing_browser {
                    self.library_browser.close();
                }
                if is_nav_select {
                    self.is_theater_mode = false;
                    self.settings_open = false;
                }

                let (task, out) = self.sidebar_feature.update(msg);

                let open_artist_task = match out {
                    SidebarOutMessage::RequestOpenArtist(id) => {
                        iced::Task::done(AppMessage::LibraryBrowser(LibraryBrowserMessage::OpenArtist(id)))
                    }
                    SidebarOutMessage::RequestOpenAlbum(id) => {
                        iced::Task::done(AppMessage::LibraryBrowser(LibraryBrowserMessage::OpenAlbum(id)))
                    }
                    SidebarOutMessage::RequestOpenMix(mix) => {
                        iced::Task::done(AppMessage::LibraryBrowser(LibraryBrowserMessage::OpenMix(mix)))
                    }
                    SidebarOutMessage::Idle => iced::Task::none(),
                };

                // El scroll de la ruta ya actualizada (arriba) necesita
                // reafirmarse contra el widget nativo tras salir del library
                // browser — ver resync_active_scroll.
                let just_exited_overlay = is_nav_select && (was_theater || closing_browser);
                let resync_task = if just_exited_overlay && !self.is_theater_mode && !self.library_browser.is_active() {
                    self.sidebar_feature.coordinator.resync_active_scroll().map(|m| {
                        AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m))
                    })
                } else {
                    iced::Task::none()
                };

                if self.current_nav_entry() != leaving {
                    self.push_history(leaving);
                }

                iced::Task::batch(vec![task.map(AppMessage::SidebarFeature), open_artist_task, resync_task])
            }

            AppMessage::SearchFeature(msg) => {
                let (search_task, out_msg) = self.search_feature.update(msg, &mut self.view_thumbnails);
                let mut feature_task = iced::Task::none();
                let mut app_task = iced::Task::none();

                match out_msg {
                    SearchFeatureOutMessage::TrackReadyToPlay(playable) => {
                        let track_metadata = playable.track.clone();
                        self.library_browser.patch_track(&track_metadata);

                        let playlists = playlist_pairs(self.sidebar_feature.coordinator.playlists_metadata());
                        let (t, _out) = self.playback_feature.update(
                            PlaybackFeatureMessage::Play(playable),
                            &playlists,
                            &self.sidebar_feature.coordinator.catalog_store,
                        );
                        feature_task = t;

                        app_task = iced::Task::done(AppMessage::SidebarFeature(
                            SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(
                                CatalogStoreMessage::TrackDownloadedAndCached(track_metadata)
                            ))
                        ));
                    }
                    SearchFeatureOutMessage::OpenAlbum(album_id) => {
                        app_task = iced::Task::done(AppMessage::LibraryBrowser(LibraryBrowserMessage::OpenAlbum(album_id)));
                    }
                    SearchFeatureOutMessage::OpenArtist(artist_id) => {
                        app_task = iced::Task::done(AppMessage::LibraryBrowser(LibraryBrowserMessage::OpenArtist(artist_id)));
                    }
                    SearchFeatureOutMessage::RequestContextMenu(track) => {
                        let catalog_store = &self.sidebar_feature.coordinator.catalog_store;
                        let playlists = playlist_pairs(catalog_store.playlists_metadata());
                        let member_of = catalog_store.playlists_containing_track(&track.id);
                        let items = TrackContextMenuBuilder::new(catalog_store.is_liked(&track.id))
                            .with_playlists(&playlists, None, &member_of)
                            .build();
                        self.search_feature.open_context_menu(track.id, items);
                    }
                    SearchFeatureOutMessage::TrackContextAction(action, track) => {
                        app_task = self.apply_search_track_action(action, track);
                    }
                    SearchFeatureOutMessage::Idle => {}
                }

                iced::Task::batch(vec![
                    search_task.map(AppMessage::SearchFeature),
                    feature_task.map(AppMessage::PlaybackFeature),
                    app_task,
                ])
            }

            AppMessage::DownloadFeature(msg) => {
                let (task, out) = self.download_feature.update(msg, &mut self.view_thumbnails);

                let catalog_task = match out {
                    DownloadFeatureOutMessage::TrackReady(track) => {
                        self.library_browser.patch_track(&track);
                        self.sidebar_feature.coordinator.forget_lyrics(&track.id);
                        iced::Task::done(AppMessage::SidebarFeature(SidebarFeatureMessage::Content(
                            CoordinatorMessage::Catalog(CatalogStoreMessage::TrackDownloadedAndCached(track))
                        )))
                    }
                    DownloadFeatureOutMessage::LyricsUpdated(track_id) => {
                        self.sidebar_feature.coordinator.lyrics_found(&track_id);
                        self.playback_feature.lyrics_updated(&track_id).map(AppMessage::PlaybackFeature)
                    }
                    DownloadFeatureOutMessage::Idle => iced::Task::none(),
                };

                iced::Task::batch(vec![task.map(AppMessage::DownloadFeature), catalog_task])
            }

            AppMessage::LibraryBrowser(msg) => self.update_library_browser(msg),

            AppMessage::NavigateBack => {
                let Some(entry) = self.nav_back_stack.pop() else {
                    return iced::Task::none();
                };
                let leaving = self.current_nav_entry();
                let task = self.replay_nav_entry(entry);
                push_capped(&mut self.nav_forward_stack, leaving);
                task
            }

            AppMessage::NavigateForward => {
                let Some(entry) = self.nav_forward_stack.pop() else {
                    return iced::Task::none();
                };
                let leaving = self.current_nav_entry();
                let task = self.replay_nav_entry(entry);
                push_capped(&mut self.nav_back_stack, leaving);
                task
            }

            AppMessage::TogglePlayback => {
                if self.manager.state.is_playing() { self.manager.pause(); } else { self.manager.resume(); }
                iced::Task::none()
            }

            AppMessage::MoveSelection { step, extend } => {
                if self.is_theater_mode || self.search_feature.input.is_open {
                    return iced::Task::none();
                }
                if self.library_browser.is_active() {
                    let catalog_store = &self.sidebar_feature.coordinator.catalog_store;
                    return self.library_browser.move_selection(step, extend, catalog_store).map(AppMessage::LibraryBrowser);
                }
                self.sidebar_feature
                    .coordinator
                    .move_selection(step, extend)
                    .map(|m| AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m)))
            }

            AppMessage::PlaySelection => {
                if self.is_theater_mode || self.search_feature.input.is_open {
                    return iced::Task::none();
                }
                if self.library_browser.is_active() {
                    self.library_browser.play_selection(&self.sidebar_feature.coordinator.catalog_store);
                } else {
                    self.sidebar_feature.coordinator.play_selection();
                }
                iced::Task::none()
            }

            AppMessage::ToggleSettings => {
                self.settings_open = !self.settings_open;
                if self.settings_open {
                    self.settings_view.opened();
                    self.is_theater_mode = false;
                    return iced::Task::none();
                }
                self.resync_after_closing_settings()
            }

            AppMessage::Settings(msg) => {
                let (task, crossfade) = self.settings_view.update(msg);
                if let Some(seconds) = crossfade {
                    self.manager.set_crossfade(seconds);
                }
                task.map(AppMessage::Settings)
            }

            AppMessage::FindPressed => {
                if self.is_theater_mode {
                    return iced::Task::none();
                }
                if !self.library_browser.is_active()
                    && !self.search_feature.input.is_open
                    && let Some(task) = self.sidebar_feature.coordinator.open_filter()
                {
                    return task.map(|m| AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m)));
                }
                iced::Task::done(AppMessage::SearchFeature(SearchFeatureMessage::Ui(SearchMessage::Open)))
            }

            AppMessage::EscapePressed => {
                if self.settings_view.cancel_photo_crop()
                    || self.sidebar_feature.coordinator.cancel_delete_dialog()
                    || self.sidebar_feature.coordinator.cancel_cover_crop()
                    || self.sidebar_feature.coordinator.cancel_artists_edit()
                {
                    return iced::Task::none();
                }

                if self.settings_open && !self.search_feature.input.is_open {
                    self.settings_open = false;
                    return self.resync_after_closing_settings();
                }

                if self.search_feature.dismiss_context_menu() {
                    return iced::Task::none();
                }
                if self.search_feature.input.is_open {
                    return iced::Task::done(AppMessage::SearchFeature(SearchFeatureMessage::Ui(SearchMessage::Close)));
                }

                if !self.is_theater_mode
                    && !self.library_browser.is_active()
                    && (self.sidebar_feature.coordinator.cancel_rename()
                        || (self.sidebar_feature.coordinator.is_side_panel_open() && self.sidebar_feature.coordinator.close_side_panel())
                        || self.sidebar_feature.coordinator.clear_selection())
                {
                    return iced::Task::none();
                }

                if self.is_theater_mode {
                    self.is_theater_mode = false;
                } else if self.library_browser.is_active() {
                    self.library_browser.close();
                } else {
                    return iced::Task::none();
                }

                // `view_content()` solo vuelve a mostrarse (y por lo tanto
                // solo hace falta reafirmar su scroll, ver
                // resync_active_scroll) si ninguna de las otras dos ramas
                // de `App::view()` sigue activa — p. ej. si había teatro Y
                // library browser, salir de teatro revela el browser, no
                // view_content() todavía.
                if self.is_theater_mode || self.library_browser.is_active() {
                    iced::Task::none()
                } else {
                    self.sidebar_feature.coordinator.resync_active_scroll().map(|m| {
                        AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m))
                    })
                }
            }
        }
    }

    fn current_settings(&self) -> AppSettings {
        AppSettings {
            sidebar_expanded: self.sidebar_feature.is_expanded,
            shuffle_enabled: self.manager.is_shuffled(),
            repeat_mode: self.manager.repeat_mode(),
            volume: self.manager.get_volume(),
            radio_enabled: self.manager.is_radio_enabled(),
            search_filter: self.search_feature.input.filter,
            explorer_play_stats: self.sidebar_feature.coordinator.explorer_view.show_play_stats,
            remix_playlists: self.sidebar_feature.coordinator.remix_view.enabled.clone(),
            crossfade_enabled: self.settings_view.crossfade_enabled(),
            crossfade_seconds: self.settings_view.crossfade_seconds(),
            server: self.settings_view.server().clone(),
            profile_name: self.settings_view.profile_name(),
            accent_color: {
                let accent = theme().accent.primary;
                (accent != crate::ui::theme::default_accent()).then(|| crate::ui::theme::to_hex(accent))
            },
        }
    }

    /// Canción actual (con posición), cola, historial y origen tal como están ahora.
    fn current_session(&self) -> PlaybackSession {
        let current = self.manager.get_current_track().map(|p| p.track.clone());
        PlaybackSession {
            position_ms: if current.is_some() { self.manager.get_position().as_millis() as u64 } else { 0 },
            current,
            queue: self.manager.get_queue_snapshot().into_iter().map(|slot| (*slot.track).clone()).collect(),
            history: self.manager.get_history_snapshot(),
            origin: self.manager.get_playback_origin(),
        }
    }

    fn current_nav_entry(&self) -> NavEntry {
        if let Some(loc) = self.library_browser.current_location() {
            match loc {
                LibraryBrowserLocation::Artist(id) => NavEntry::LibraryArtist(id),
                LibraryBrowserLocation::Album(id) => NavEntry::LibraryAlbum(id),
                LibraryBrowserLocation::Mix(mix) => NavEntry::LibraryMix(mix),
            }
        } else {
            NavEntry::Content(self.sidebar_feature.coordinator.active_route.clone())
        }
    }

    fn push_history(&mut self, leaving: NavEntry) {
        if self.is_replaying_history {
            return;
        }
        push_capped(&mut self.nav_back_stack, leaving);
        self.nav_forward_stack.clear();
    }

    fn replay_nav_entry(&mut self, entry: NavEntry) -> iced::Task<AppMessage> {
        self.is_replaying_history = true;
        let task = match entry {
            NavEntry::Content(ActiveRoute::Nav(nav_id)) => {
                self.update(AppMessage::SidebarFeature(SidebarFeatureMessage::SelectNav(nav_id)))
            }
            NavEntry::Content(ActiveRoute::Playlist(id)) => {
                self.update(AppMessage::SidebarFeature(SidebarFeatureMessage::SelectPlaylist(id)))
            }
            NavEntry::LibraryArtist(id) => self.update_library_browser(LibraryBrowserMessage::OpenArtist(id)),
            NavEntry::LibraryAlbum(id) => self.update_library_browser(LibraryBrowserMessage::OpenAlbum(id)),
            NavEntry::LibraryMix(mix) => self.update_library_browser(LibraryBrowserMessage::OpenMix(mix)),
        };
        self.is_replaying_history = false;
        task
    }

    /// Puentea `LibraryBrowserOutMessage` hacia el `CatalogStore` del sidebar,
    /// igual que ya hace `PlaybackOutMessage::RequestToggleLike`.
    fn update_library_browser(&mut self, msg: LibraryBrowserMessage) -> iced::Task<AppMessage> {
        let leaving = self.current_nav_entry();

        if matches!(msg, LibraryBrowserMessage::OpenArtist(_) | LibraryBrowserMessage::OpenAlbum(_)) {
            self.is_theater_mode = false;
        }
        if matches!(msg, LibraryBrowserMessage::OpenArtist(_) | LibraryBrowserMessage::OpenAlbum(_) | LibraryBrowserMessage::OpenMix(_)) {
            self.settings_open = false;
        }

        let playlists = playlist_pairs(self.sidebar_feature.coordinator.playlists_metadata());
        let (task, out) = self.library_browser.update(msg, &playlists, &self.sidebar_feature.coordinator.catalog_store);

        let bridge_task = match out {
            LibraryBrowserOutMessage::RequestToggleLike(tracks) => {
                let catalog_store = &mut self.sidebar_feature.coordinator.catalog_store;
                iced::Task::batch(tracks.into_iter().map(|track| {
                    catalog_store.toggle_like_track(track).map(|catalog_msg| {
                        AppMessage::SidebarFeature(SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(catalog_msg)))
                    })
                }).collect::<Vec<_>>())
            }
            LibraryBrowserOutMessage::RequestAddToPlaylist { playlist_id, tracks } => self
                .sidebar_feature
                .coordinator
                .add_tracks_to_playlist(&playlist_id, tracks)
                .map(|m| AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m))),
            LibraryBrowserOutMessage::RequestToggleFollowArtist(artist_id, name, photo_url) => self
                .sidebar_feature
                .coordinator
                .catalog_store
                .toggle_follow_artist(&artist_id, &name, photo_url.as_deref())
                .map(|catalog_msg| {
                    AppMessage::SidebarFeature(SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(catalog_msg)))
                }),
            LibraryBrowserOutMessage::RequestTrackTool(tool, track_id) => self.run_track_tool(tool, &track_id),
            LibraryBrowserOutMessage::Idle => iced::Task::none(),
        };

        if self.current_nav_entry() != leaving {
            self.push_history(leaving);
        }

        iced::Task::batch([task.map(AppMessage::LibraryBrowser), bridge_task])
    }

    /// Corre una herramienta del track_manager vía el `CatalogStore`.
    fn run_track_tool(&self, tool: TrackTool, track_id: &str) -> iced::Task<AppMessage> {
        self.sidebar_feature
            .coordinator
            .catalog_store
            .run_track_tool(tool, track_id)
            .map(|catalog_msg| {
                AppMessage::SidebarFeature(SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(catalog_msg)))
            })
    }

    /// Aplica una opción del menú de una canción de la búsqueda (puede no estar en el catálogo).
    fn apply_search_track_action(&mut self, action: TrackContextAction, track: crate::model::Track) -> iced::Task<AppMessage> {
        let to_app = |catalog_msg| AppMessage::SidebarFeature(SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(catalog_msg)));
        let catalog_store = &mut self.sidebar_feature.coordinator.catalog_store;
        match action {
            TrackContextAction::PlayNow => self.manager.play_context(vec![track], 0),
            TrackContextAction::FrontEnqueue => self.manager.enqueue_front(track),
            TrackContextAction::Enqueue => self.manager.enqueue(track),
            TrackContextAction::StartRadio => self.manager.start_radio(track),
            TrackContextAction::ToggleLike => return catalog_store.toggle_like_track(track).map(to_app),
            TrackContextAction::AddToPlaylist(playlist_id) => {
                return self
                    .sidebar_feature
                    .coordinator
                    .add_tracks_to_playlist(&playlist_id, vec![track])
                    .map(|m| AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m)));
            }
            TrackContextAction::CopyId => return iced::clipboard::write(track.id),
            TrackContextAction::CopyYoutubeLink => return iced::clipboard::write(youtube_link(&track.id)),
            TrackContextAction::Tool(_) | TrackContextAction::DeleteFromCatalog | TrackContextAction::RemoveFromPlaylist => {}
        }
        iced::Task::none()
    }

    /// Avatar de la barra superior (abre Ajustes), con un anillo de acento mientras está abierto.
    fn view_settings_toggle(&self) -> Element<'_, AppMessage> {
        let ring = if self.settings_open { theme().accent.primary } else { iced::Color::TRANSPARENT };
        let avatar = container(self.settings_view.avatar(AVATAR_SIZE))
            .padding(AVATAR_RING_WIDTH)
            .style(move |_| container::Style {
                border: border::rounded(AVATAR_SIZE).color(ring).width(AVATAR_RING_WIDTH),
                ..Default::default()
            });
        iced::widget::button(avatar)
            .style(crate::ui::styles::button::minimal)
            .padding(spacing::SP_4)
            .on_press(AppMessage::ToggleSettings)
            .into()
    }

    /// Al cerrar Ajustes vuelve a verse la vista de fondo: se reafirma su scroll.
    fn resync_after_closing_settings(&self) -> iced::Task<AppMessage> {
        if self.is_theater_mode || self.library_browser.is_active() {
            return iced::Task::none();
        }
        self.sidebar_feature
            .coordinator
            .resync_active_scroll()
            .map(|m| AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m)))
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, AppMessage> {
        let center_content: Element<'_, AppMessage> = if self.is_theater_mode {
            self.playback_feature.view_theater().map(AppMessage::PlaybackFeature)
        } else if self.settings_open {
            self.settings_view.view().map(AppMessage::Settings)
        } else if self.library_browser.is_active() {
            self.library_browser.view(&self.sidebar_feature.coordinator.catalog_store).map(AppMessage::LibraryBrowser)
        } else {
            self.sidebar_feature.view_content().map(AppMessage::SidebarFeature)
        };

        // El teatro, la biblioteca (artista, álbum, mezcla) y las páginas de colección van de borde a
        // borde (pintan su propio fondo) y ponen sus propios márgenes.
        let edge_to_edge = self.is_theater_mode
            || (!self.settings_open
                && (self.library_browser.is_active()
                    || matches!(
                        self.sidebar_feature.coordinator.active_route,
                        ActiveRoute::Playlist(_) | ActiveRoute::Nav(NavId::Explorer | NavId::Favorites | NavId::Remix)
                    )));

        let center_view = container(center_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(if edge_to_edge { spacing::SP_0 } else { spacing::SP_20 })
            .style(|_theme| container::Style {
                background: Some(Background::Color(theme().surface.panel)),
                border: Border {
                    radius: border::Radius::from(radii::R_18),
                    ..Default::default()
                },
                ..Default::default()
            });

        // El panel va envuelto en `CORNER_RING_WIDTH` de aire: ahí se apoya el anillo que, de borde a
        // borde, vuelve a redondear sus esquinas cuando el contenido scrolleado las tapa.
        let center_view: Element<'_, AppMessage> = {
            let wrapped = container(center_view).padding(CORNER_RING_WIDTH);
            if edge_to_edge {
                stack![wrapped, corner_ring(radii::R_18, theme().surface.base)].into()
            } else {
                wrapped.into()
            }
        };
        // Lo que rodea al panel se achica lo mismo que ese aire, para que nada se mueva.
        let beside_panel = 15.0 - CORNER_RING_WIDTH;

        let side_panel = if self.is_side_panel_visible() { self.sidebar_feature.view_side_panel() } else { None };
        let queue_view = match side_panel {
            // Mismo ancho (animado) que la cola: se ve como si la cola se transformara en el panel.
            Some(panel) => container(panel.map(AppMessage::SidebarFeature))
                .width(Length::Fixed(self.playback_feature.queue_width()))
                .height(Length::Fill)
                .clip(true),
            None => container(self.playback_feature.view_queue().map(AppMessage::PlaybackFeature)),
        };

        let ring_rows = Padding { top: CORNER_RING_WIDTH, bottom: CORNER_RING_WIDTH, ..Padding::ZERO };
        let content_layer = row![
            container(self.sidebar_feature.view_sidebar().map(AppMessage::SidebarFeature)).padding(ring_rows),
            space().width(beside_panel),
            center_view,
            space().width(beside_panel),
            container(queue_view).padding(ring_rows),
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: spacing::SP_10 - CORNER_RING_WIDTH,
                right: spacing::SP_15,
                bottom: spacing::SP_10 - CORNER_RING_WIDTH,
                left: spacing::SP_0,
            });

        let catalog_store = &self.sidebar_feature.coordinator.catalog_store;

        // Píldoras de descarga: viven en `layout_stack` (no en
        // `absolute_root_layers`) porque alinearlas `align_y(End)` dentro de
        // esta región las deja pegadas justo arriba de `playback_view` sin
        // necesitar conocer su altura real (la barra no tiene altura fija).
        let pill_overlay = container(
            self.download_feature
                .view(&self.view_thumbnails)
                .map(AppMessage::DownloadFeature),
        )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::End)
            .padding(Padding { bottom: spacing::SP_20, ..Default::default() });

        // 1. EXTRAEMOS los overlays de aquí. Este stack ahora es netamente estructural
        // para el cuerpo de la aplicación y ya no mezcla popups.
        let layout_stack = stack![content_layer, pill_overlay];

        let is_current_liked = self.playback_feature.current_track_id()
            .and_then(|id| catalog_store.track_by_id(id))
            .map(|t| t.liked)
            .unwrap_or(false);

        let playback_view  = self.playback_feature.view(self.is_theater_mode, is_current_liked).map(AppMessage::PlaybackFeature);
        let search_toggle  = self.search_feature.view_toggle().map(AppMessage::SearchFeature);
        let search_overlay = self.search_feature.view_overlay(&self.view_thumbnails);

        let top_bar = row![
            self.sidebar_feature.view_toggle().map(AppMessage::SidebarFeature),
            space().width(Length::Fill),
            container(search_toggle)
                .align_x(Alignment::End)
                .padding(Padding { top: spacing::SP_10, ..Default::default() }),
            container(self.view_settings_toggle())
                .padding(Padding { top: spacing::SP_10, right: spacing::SP_15, left: spacing::SP_8, ..Default::default() }),
        ]
            .align_y(iced::alignment::Vertical::Center)
            .width(Length::Fill);

        let app_root = container(
            column![top_bar, layout_stack, playback_view].height(Length::Fill)
        )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(theme().surface.base)),
                ..Default::default()
            });

        // 2. CONSTRUIMOS EL STACK RAÍZ ABSOLUTO.
        // El origen (0,0) de este stack coincide milimétricamente con el (0,0) de la ventana nativa
        // y del evento global iced::mouse::Event::CursorMoved.
        let mut absolute_root_layers: Vec<Element<'_, AppMessage>> = vec![app_root.into()];

        if let Some(search_overlay) = search_overlay {
            absolute_root_layers.push(search_overlay.map(AppMessage::SearchFeature));
        }
        if let Some(menu) = self.search_feature.view_context_menu() {
            absolute_root_layers.push(menu.map(AppMessage::SearchFeature));
        }

        // 3. INYECTAMOS LOS OVERLAYS DEL SIDEBAR (Menús y Diálogos) EN LA CÚSPIDE.
        absolute_root_layers.extend(
            self.sidebar_feature
                .view_overlays()
                .into_iter()
                .map(|layer| layer.map(AppMessage::SidebarFeature)),
        );

        if let Some(menu) = self.library_browser.view_context_menu() {
            absolute_root_layers.push(menu.map(AppMessage::LibraryBrowser));
        }

        if let Some(menu) = self.playback_feature.view_track_context_menu() {
            absolute_root_layers.push(menu.map(AppMessage::PlaybackFeature));
        }

        if let Some(editor) = self.settings_view.view_photo_crop() {
            absolute_root_layers.push(editor.map(AppMessage::Settings));
        }

        stack(absolute_root_layers).into()
    }
    pub fn subscription(&self) -> iced::Subscription<AppMessage> {
        let playback_sub = self.playback_feature.subscription(self.is_theater_mode).map(AppMessage::PlaybackFeature);
        let download_sub = self.download_feature.subscription().map(AppMessage::DownloadFeature);
        let sidebar_sub  = self.sidebar_feature.subscription().map(AppMessage::SidebarFeature);

        let close_sub = window::events()
            .filter_map(|(id, event)| match event {
                window::Event::CloseRequested => Some(AppMessage::CloseRequested(id)),
                _ => None,
            });

        let nav_sub = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Back)) => {
                Some(AppMessage::NavigateBack)
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Forward)) => {
                Some(AppMessage::NavigateForward)
            }
            _ => None,
        });

        // Única suscripción global de cursor de la app. Antes había tres
        // (acá, playback_feature y sidebar), o sea tres mensajes -> tres
        // rebuilds completos de UI por cada pixel de movimiento del mouse.
        let cursor_sub = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(AppMessage::CursorMoved(position))
            }
            _ => None,
        });

        // Espacio / flechas / RePág / AvPág / Enter, salvo que un widget (p. ej. un text_input) ya consumió la tecla.
        let shortcuts_sub = iced::event::listen_with(|event, status, _window| {
            use iced::keyboard::key::Named;

            if status == iced::event::Status::Captured {
                return None;
            }
            let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
                return None;
            };
            if (modifiers.control() || modifiers.command())
                && matches!(&key, iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("f"))
            {
                return Some(AppMessage::FindPressed);
            }
            let step = match key {
                iced::keyboard::Key::Named(Named::Space) => return Some(AppMessage::TogglePlayback),
                iced::keyboard::Key::Named(Named::Enter) => return Some(AppMessage::PlaySelection),
                iced::keyboard::Key::Named(Named::ArrowUp) => SelectionStep::Rows(-1),
                iced::keyboard::Key::Named(Named::ArrowDown) => SelectionStep::Rows(1),
                iced::keyboard::Key::Named(Named::PageUp) => SelectionStep::Pages(-1),
                iced::keyboard::Key::Named(Named::PageDown) => SelectionStep::Pages(1),
                _ => return None,
            };
            Some(AppMessage::MoveSelection { step, extend: modifiers.shift() })
        });

        // Tamaño de la ventana para que los menús contextuales no se salgan.
        let resize_sub = window::resize_events().map(|(_id, size)| AppMessage::WindowResized(size));

        let escape_sub = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) => Some(AppMessage::EscapePressed),
            _ => None,
        });

        let autosave_sub = iced::time::every(std::time::Duration::from_secs(2))
            .map(|_| AppMessage::AutosaveTick);

        // El tray corre en su propio hilo y avisa por canal; antes esto se
        // sondeaba desde el Tick de 40ms, que por eso no podía apagarse.
        let tray_sub = iced::Subscription::run(tray_wake_events);

        iced::Subscription::batch(vec![
            playback_sub,
            download_sub,
            sidebar_sub,
            close_sub,
            nav_sub,
            cursor_sub,
            escape_sub,
            shortcuts_sub,
            resize_sub,
            autosave_sub,
            tray_sub,
        ])
    }

    pub fn theme(&self, _window: window::Id) -> Theme {
        ATELIER_THEME.clone()
    }
}

/// Huella barata de la sesión (actual, slots de la cola, historial): el
/// autosave solo reescribe `session.json` cuando cambia.
fn session_fingerprint(manager: &TrackManager) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    manager.get_current_track().map(|p| p.track.id.clone()).hash(&mut hasher);
    for slot in manager.get_queue_snapshot() {
        slot.id.hash(&mut hasher);
    }
    manager.history_len().hash(&mut hasher);
    manager.get_history_snapshot().last().map(|t| t.id.clone()).hash(&mut hasher);
    hasher.finish()
}

/// Tema de iced derivado de los tokens: alimenta todo widget sin `.style()`.
/// Emite un `TrayWake` cada vez que el tray marca un flag.
fn tray_wake_events() -> impl futures::Stream<Item = AppMessage> {
    iced::stream::channel(16, async move |mut output| {
        let Some(flags) = TRAY_FLAGS.get() else { return };
        let mut rx = flags.wake.subscribe();
        loop {
            match rx.recv().await {
                Ok(()) => { let _ = output.send(AppMessage::TrayWake).await; }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    })
}

static ATELIER_THEME: LazyLock<Theme> = LazyLock::new(|| {
    let t = ui::theme::theme();

    Theme::custom(
        "Atelier",
        Palette {
            background: t.surface.base,
            text: t.content.primary,
            primary: t.accent.primary,
            success: t.status.cached,
            warning: t.status.error,
            danger: t.status.error,
        },
    )
});

fn main() -> iced::Result {
    unsafe {
        extern "C" fn signal_handler(_: libc::c_int) {
            if let Some(f) = TRAY_FLAGS.get() {
                f.quit.store(true, Ordering::Relaxed);
            }
        }
        libc::signal(libc::SIGTERM, signal_handler as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT,  signal_handler as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }

    let _ = dotenvy::dotenv();

    if AppSettings::load().server.mode == ServerMode::Local {
        local_server::start();
    }

    iced::daemon(App::init, App::update, App::view)
        .subscription(App::subscription)
        .font(include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf"))
        .font(include_bytes!("../assets/fonts/SF-Pro-Display-Regular.otf"))
        .theme(App::theme)
        .run()
}