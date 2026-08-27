mod microservices;
mod model;
mod audio;
mod ui;
pub mod tray;
pub mod db;
pub mod utils;
mod settings;

use std::sync::{Arc, LazyLock, OnceLock};
use std::sync::atomic::Ordering;
use iced::theme::Palette;
use iced::{border, window, Background, Border, Element, Length, Padding, Theme};
use iced::widget::{column, container, row, space, stack};

use crate::audio::discord::DiscordPresence;
use crate::audio::download_daemon::DownloadWorker;
use crate::audio::engine::AudioEngine;
use audio::manager::manager::TrackManager;
use crate::audio::mpris::MprisServer;
use crate::audio::radio_daemon::RadioWorker;
use crate::db::db::init_db;
use crate::db::playlist_manager::PlaylistManager;
use crate::db::play_history_manager::PlayHistoryManager;
use crate::db::followed_artist_manager::FollowedArtistManager;
use crate::audio::play_history_recorder::PlayHistoryRecorder;
use crate::microservices::client::MicroserviceClient;
use crate::settings::AppSettings;
use crate::tray::TrayFlags;

use crate::ui::assets::radii;
use crate::ui::library_browser_feature::library_browser_feature::{
    LibraryBrowserFeature, LibraryBrowserMessage, LibraryBrowserOutMessage,
};
use crate::ui::playback_feature::player::TrackLink;
use crate::ui::playback_feature::playback_feature::{PlaybackFeature, PlaybackFeatureMessage, PlaybackOutMessage};
use crate::ui::search_feature::search_feature::{SearchFeature, SearchFeatureMessage, SearchFeatureOutMessage};
use crate::ui::sidebar_feature::sidebar_feature_v2::{
    SidebarFeatureV2 as SidebarFeature, SidebarMessage as SidebarFeatureMessage, SidebarOutMessage
};
use crate::ui::views::view_coordinator::{playlist_pairs, CoordinatorMessage};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;
use crate::ui::views::catalog_store::CatalogStoreMessage;
use crate::ui::widgets::context_menu::ContextMenuEvent;
use crate::ui::assets::spacing;
use crate::ui::theme::theme;


static TRAY_FLAGS: OnceLock<Arc<TrayFlags>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum AppMessage {
    SearchFeature(SearchFeatureMessage),
    PlaybackFeature(PlaybackFeatureMessage),
    SidebarFeature(SidebarFeatureMessage),
    WindowOpened(window::Id),
    CloseRequested(window::Id),
    ShowWindow,
    Quit,
    LibraryBrowser(LibraryBrowserMessage),
    NavigateBack,
    EscapePressed,
    AutosaveTick,
}

struct App {
    _engine: AudioEngine,
    manager: Arc<TrackManager>,
    search_feature: SearchFeature,
    playback_feature: PlaybackFeature,
    sidebar_feature: SidebarFeature,
    library_browser: LibraryBrowserFeature,
    view_thumbnails: ThumbnailCache,
    tray_flags: Arc<TrayFlags>,
    main_window: Option<window::Id>,
    is_theater_mode: bool,
    last_saved_settings: AppSettings,
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
        let tray_flags = tray::spawn_tray(Arc::clone(&manager));
        TRAY_FLAGS.set(Arc::clone(&tray_flags)).ok();

        let client = Arc::new(MicroserviceClient::new(
            &std::env::var("TRACK_MANAGER_HOST").unwrap_or("127.0.0.1".into()),
            std::env::var("TRACK_MANAGER_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(7878),
        ));

        MprisServer::spawn(Arc::clone(&manager));
        DiscordPresence::spawn(Arc::clone(&manager));
        DownloadWorker::new(Arc::clone(&manager), Arc::clone(&client)).spawn();

        // Guardamos una copia para el sidebar/explorer antes de que RadioWorker consuma `client`.
        let sidebar_client = Arc::clone(&client);
        // Copia (no Arc, MicroserviceClient ya es Clone) para LibraryBrowserFeature.
        let library_browser_client = client.as_ref().clone();
        let library_browser_manager = Arc::clone(&manager);

        let radio = RadioWorker::new(Arc::clone(&manager), client).spawn();
        radio.set_enabled(false);
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
            let followed_artist_manager = FollowedArtistManager::new(pool);
            (playlist_manager, play_history_manager, followed_artist_manager)
        };

        let (playlist_manager, play_history_manager, followed_artist_manager) =
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

        PlayHistoryRecorder::spawn(Arc::clone(&manager), Arc::clone(&play_history_manager), sidebar_client.as_ref().clone());

        let (mut sidebar_feature, sidebar_task) = SidebarFeature::new(
            sidebar_client,
            Arc::clone(&playlist_manager),
            Arc::clone(&manager),
            Arc::clone(&play_history_manager),
            Arc::clone(&followed_artist_manager),
        );
        sidebar_feature.set_expanded_immediate(settings.sidebar_expanded);

        let app = Self {
            _engine: engine,
            search_feature: SearchFeature::new(),
            playback_feature: PlaybackFeature::new(Arc::clone(&manager)),
            sidebar_feature,
            library_browser: LibraryBrowserFeature::new(library_browser_client, library_browser_manager, Arc::clone(&followed_artist_manager)),
            view_thumbnails: ThumbnailCache::new(100, 50),
            tray_flags,
            main_window: Some(window_id),
            manager,
            is_theater_mode: false,
            last_saved_settings: settings,
        };

        let init_task = iced::Task::batch(vec![
            open_task.map(AppMessage::WindowOpened),
            sidebar_task.map(AppMessage::SidebarFeature),
        ]);

        (app, init_task)
    }

    pub fn update(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        match message {
            AppMessage::WindowOpened(_) => iced::Task::none(),

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
                iced::exit()
            }

            AppMessage::AutosaveTick => {
                let current = self.current_settings();
                if current != self.last_saved_settings {
                    let _ = current.save();
                    self.last_saved_settings = current;
                }
                iced::Task::none()
            }

            AppMessage::PlaybackFeature(PlaybackFeatureMessage::Tick) => {
                if self.tray_flags.quit.load(Ordering::Relaxed) {
                    return iced::Task::done(AppMessage::Quit);
                }
                if self.tray_flags.show_window.load(Ordering::Relaxed) {
                    self.tray_flags.show_window.store(false, Ordering::Relaxed);
                    return iced::Task::done(AppMessage::ShowWindow);
                }

                let position = self.manager.get_position();
                let position_task = self.playback_feature.position_updated(position);

                let playlists = playlist_pairs(self.sidebar_feature.coordinator.playlists_metadata());
                let (tick_task, _out) = self.playback_feature
                    .update(PlaybackFeatureMessage::Tick, &playlists);

                iced::Task::batch(vec![
                    position_task.map(AppMessage::PlaybackFeature),
                    tick_task.map(AppMessage::PlaybackFeature),
                ])
            }

            AppMessage::PlaybackFeature(msg) => {
                let playlists = playlist_pairs(self.sidebar_feature.coordinator.playlists_metadata());
                let (task, out_msg) = self.playback_feature.update(msg, &playlists);

                let like_task = match out_msg {
                    PlaybackOutMessage::ToggleTheaterMode => {
                        self.is_theater_mode = !self.is_theater_mode;
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
                    PlaybackOutMessage::RequestToggleLike(track_id) => {
                        self.sidebar_feature
                            .coordinator
                            .catalog_store
                            .toggle_like(&track_id)
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
                    PlaybackOutMessage::RequestAddToPlaylist { playlist_id, track_id } => {
                        self.sidebar_feature
                            .coordinator
                            .catalog_store
                            .add_track_to_playlist(&playlist_id, &track_id)
                            .map(|catalog_msg| {
                                AppMessage::SidebarFeature(SidebarFeatureMessage::Content(
                                    CoordinatorMessage::Catalog(catalog_msg)
                                ))
                            })
                    }
                    PlaybackOutMessage::RequestDeleteFromCatalog(track_id) => {
                        self.sidebar_feature.coordinator.catalog_store.delete_track(&track_id);
                        iced::Task::none()
                    }
                    PlaybackOutMessage::TrackNowPlaying(track) => {
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
                let closing_browser = self.library_browser.is_active()
                    && matches!(msg, SidebarFeatureMessage::SelectNav(_) | SidebarFeatureMessage::SelectPlaylist(_));
                if closing_browser {
                    self.library_browser.close();
                    self.is_theater_mode = false;
                }

                let (task, out) = self.sidebar_feature.update(msg);

                let open_artist_task = match out {
                    SidebarOutMessage::RequestOpenArtist(id) => {
                        iced::Task::done(AppMessage::LibraryBrowser(LibraryBrowserMessage::OpenArtist(id)))
                    }
                    SidebarOutMessage::RequestOpenAlbum(id) => {
                        iced::Task::done(AppMessage::LibraryBrowser(LibraryBrowserMessage::OpenAlbum(id)))
                    }
                    SidebarOutMessage::Idle => iced::Task::none(),
                };

                // El scroll de la ruta ya actualizada (arriba) necesita
                // reafirmarse contra el widget nativo tras salir del library
                // browser — ver resync_active_scroll.
                let resync_task = if closing_browser {
                    self.sidebar_feature.coordinator.resync_active_scroll().map(|m| {
                        AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m))
                    })
                } else {
                    iced::Task::none()
                };

                iced::Task::batch(vec![task.map(AppMessage::SidebarFeature), open_artist_task, resync_task])
            }

            AppMessage::SearchFeature(msg) => {
                let (search_task, out_msg) = self.search_feature.update(msg, &mut self.view_thumbnails);
                let mut feature_task = iced::Task::none();
                let mut catalog_task = iced::Task::none();

                match out_msg {
                    SearchFeatureOutMessage::TrackReadyToPlay(playable) => {
                        let track_metadata = playable.track.clone();

                        let playlists = playlist_pairs(self.sidebar_feature.coordinator.playlists_metadata());
                        let (t, _out) = self.playback_feature.update(
                            PlaybackFeatureMessage::Play(playable),
                            &playlists,
                        );
                        feature_task = t;

                        catalog_task = iced::Task::done(AppMessage::SidebarFeature(
                            SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(
                                CatalogStoreMessage::TrackDownloadedAndCached(track_metadata)
                            ))
                        ));
                    }
                    SearchFeatureOutMessage::Idle => {}
                }

                iced::Task::batch(vec![
                    search_task.map(AppMessage::SearchFeature),
                    feature_task.map(AppMessage::PlaybackFeature),
                    catalog_task,
                ])
            }

            AppMessage::LibraryBrowser(msg) => self.update_library_browser(msg),

            AppMessage::NavigateBack => {
                if self.library_browser.is_active() {
                    self.update_library_browser(LibraryBrowserMessage::Back)
                } else {
                    iced::Task::none()
                }
            }

            AppMessage::EscapePressed => {
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
        }
    }

    /// Puentea `LibraryBrowserOutMessage` hacia el `CatalogStore` del sidebar,
    /// igual que ya hace `PlaybackOutMessage::RequestToggleLike`.
    fn update_library_browser(&mut self, msg: LibraryBrowserMessage) -> iced::Task<AppMessage> {
        if matches!(msg, LibraryBrowserMessage::OpenArtist(_) | LibraryBrowserMessage::OpenAlbum(_)) {
            self.is_theater_mode = false;
        }

        let was_active = self.library_browser.is_active();
        let playlists = playlist_pairs(self.sidebar_feature.coordinator.playlists_metadata());
        let (task, out) = self.library_browser.update(msg, &playlists);

        let bridge_task = match out {
            LibraryBrowserOutMessage::RequestToggleLike(track_id) => self
                .sidebar_feature
                .coordinator
                .catalog_store
                .toggle_like(&track_id)
                .map(|catalog_msg| {
                    AppMessage::SidebarFeature(SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(catalog_msg)))
                }),
            LibraryBrowserOutMessage::RequestAddToPlaylist { playlist_id, track_id } => self
                .sidebar_feature
                .coordinator
                .catalog_store
                .add_track_to_playlist(&playlist_id, &track_id)
                .map(|catalog_msg| {
                    AppMessage::SidebarFeature(SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(catalog_msg)))
                }),
            LibraryBrowserOutMessage::RequestToggleFollowArtist(artist_id, name, photo_url) => self
                .sidebar_feature
                .coordinator
                .catalog_store
                .toggle_follow_artist(&artist_id, &name, photo_url.as_deref())
                .map(|catalog_msg| {
                    AppMessage::SidebarFeature(SidebarFeatureMessage::Content(CoordinatorMessage::Catalog(catalog_msg)))
                }),
            LibraryBrowserOutMessage::Idle => iced::Task::none(),
        };

        // Se agotó el historial de `Back` (o llegó `Close`) y volvemos a
        // mostrar view_content() — reafirmar su scroll nativo (ver
        // resync_active_scroll).
        let closed_now = was_active && !self.library_browser.is_active();
        let resync_task = if closed_now && !self.is_theater_mode {
            self.sidebar_feature.coordinator.resync_active_scroll().map(|m| {
                AppMessage::SidebarFeature(SidebarFeatureMessage::Content(m))
            })
        } else {
            iced::Task::none()
        };

        iced::Task::batch([task.map(AppMessage::LibraryBrowser), bridge_task, resync_task])
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, AppMessage> {
        let center_content: Element<'_, AppMessage> = if self.is_theater_mode {
            self.playback_feature.view_theater().map(AppMessage::PlaybackFeature)
        } else if self.library_browser.is_active() {
            self.library_browser.view().map(AppMessage::LibraryBrowser)
        } else {
            self.sidebar_feature.view_content().map(AppMessage::SidebarFeature)
        };

        let center_view = container(center_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(spacing::SP_20)
            .style(|_theme| container::Style {
                background: Some(Background::Color(theme().surface.panel)),
                border: Border {
                    radius: border::Radius::from(radii::R_18),
                    ..Default::default()
                },
                ..Default::default()
            });

        let queue_view = container(
            self.playback_feature.view_queue().map(AppMessage::PlaybackFeature)
        );

        let content_layer = row![
            self.sidebar_feature.view_sidebar().map(AppMessage::SidebarFeature),
            space().width(15),
            center_view,
            space().width(15),
            queue_view
        ]
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: spacing::SP_10,
                right: spacing::SP_15,
                bottom: spacing::SP_10,
                left: spacing::SP_0,
            });

        // 1. EXTRAEMOS los overlays de aquí. Este stack ahora es netamente estructural
        // para el cuerpo de la aplicación y ya no mezcla popups.
        let layout_stack = stack![content_layer];

        let is_current_liked = self.playback_feature.current_track_id()
            .and_then(|id| self.sidebar_feature.coordinator.catalog_store.track_by_id(id))
            .map(|t| t.liked)
            .unwrap_or(false);

        let playback_view  = self.playback_feature.view(self.is_theater_mode, is_current_liked).map(AppMessage::PlaybackFeature);
        let search_view    = self.search_feature.view().map(AppMessage::SearchFeature);
        let search_overlay = self.search_feature.view_dropdown(&self.view_thumbnails).map(AppMessage::SearchFeature);

        let top_bar = row![
            self.sidebar_feature.view_toggle().map(AppMessage::SidebarFeature),
            container(search_view).width(Length::Fill),
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
        let mut absolute_root_layers: Vec<Element<'_, AppMessage>> = vec![
            app_root.into(),
            search_overlay,
        ];

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

        stack(absolute_root_layers).into()
    }
    pub fn subscription(&self) -> iced::Subscription<AppMessage> {
        let search_sub   = self.search_feature.subscription().map(AppMessage::SearchFeature);
        let playback_sub = self.playback_feature.subscription(self.is_theater_mode).map(AppMessage::PlaybackFeature);
        let sidebar_sub  = self.sidebar_feature.subscription().map(AppMessage::SidebarFeature);

        let close_sub = window::events()
            .filter_map(|(id, event)| match event {
                window::Event::CloseRequested => Some(AppMessage::CloseRequested(id)),
                _ => None,
            });

        let nav_back_sub = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Back)) => {
                Some(AppMessage::NavigateBack)
            }
            _ => None,
        });

        let library_menu_mouse_sub = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => Some(AppMessage::LibraryBrowser(
                LibraryBrowserMessage::ContextMenuEvent(ContextMenuEvent::MouseMoved(position)),
            )),
            _ => None,
        });

        let escape_sub = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) => Some(AppMessage::EscapePressed),
            _ => None,
        });

        let autosave_sub = iced::time::every(std::time::Duration::from_secs(2))
            .map(|_| AppMessage::AutosaveTick);

        iced::Subscription::batch(vec![
            search_sub,
            playback_sub,
            sidebar_sub,
            close_sub,
            nav_back_sub,
            library_menu_mouse_sub,
            escape_sub,
            autosave_sub,
        ])
    }

    pub fn theme(&self, _window: window::Id) -> Theme {
        ATELIER_THEME.clone()
    }
}

/// Tema de iced derivado de los tokens: alimenta todo widget sin `.style()`.
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

    iced::daemon(App::init, App::update, App::view)
        .subscription(App::subscription)
        .font(include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf"))
        .font(include_bytes!("../assets/fonts/SF-Pro-Display-Regular.otf"))
        .theme(App::theme)
        .run()
}