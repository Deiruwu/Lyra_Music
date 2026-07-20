mod microservices;
mod model;
mod audio;
mod ui;
pub mod tray;
pub mod db;

use std::sync::{Arc, OnceLock};
use std::sync::atomic::Ordering;
use iced::{border, window, Background, Border, Color, Element, Font, Length, Padding, Theme};
use iced::widget::{column, container, row, space, stack};

use crate::audio::discord::DiscordPresence;
use crate::audio::download_daemon::DownloadWorker;
use crate::audio::engine::AudioEngine;
use audio::manager::manager::TrackManager;
use crate::audio::mpris::MprisServer;
use crate::audio::radio_daemon::RadioWorker;
use crate::db::db::init_db;
use crate::db::playlist_manager::PlaylistManager;
use crate::microservices::client::MicroserviceClient;
use crate::tray::TrayFlags;

use crate::ui::playback_feature::playback_feature::{PlaybackFeature, PlaybackFeatureMessage, PlaybackOutMessage};
use crate::ui::search_feature::search_feature::{SearchFeature, SearchFeatureMessage, SearchFeatureOutMessage};
use crate::ui::sidebar_feature::sidebar_feature::{
    SidebarFeature, SidebarFeatureMessage
};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;

const JETBRAINS_MONO: Font = Font::with_name("JetBrainsMono Nerd Font");

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
}

struct App {
    _engine: AudioEngine,
    manager: Arc<TrackManager>,
    search_feature: SearchFeature,
    playback_feature: PlaybackFeature,
    sidebar_feature: SidebarFeature,
    player_thumbnails: ThumbnailCache,
    view_thumbnails: ThumbnailCache,
    radio: Arc<RadioWorker>,
    tray_flags: Arc<TrayFlags>,
    main_window: Option<window::Id>,
    is_theater_mode: bool,
}

impl App {
    pub fn init() -> (Self, iced::Task<AppMessage>) {
        let (manager, engine) = TrackManager::new()
            .expect("Fallo fatal al inicializar el hardware de audio");

        let manager    = Arc::new(manager);
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

        let radio = RadioWorker::new(Arc::clone(&manager), client).spawn();
        radio.set_enabled(true);
        radio.set_queue_target(15);

        let (window_id, open_task) = window::open(window::Settings {
            size: iced::Size::new(800.0, 600.0),
            exit_on_close_request: false,
            ..Default::default()
        });

        // `PlaylistManager` necesita el pool de SQLite ya conectado (init_db
        // corre las migraciones) antes de poder construirse (cachea el id
        // de la playlist SYSTEM con una query). Ambos pasos son async, pero
        // `App::init()` es síncrono.
        //
        // `iced` está compilado con el feature "tokio" (ver Cargo.toml), lo
        // que significa que ya arranca su propio runtime tokio multi-thread
        // para ejecutar `Task::perform` — es el mismo runtime que hace
        // funcionar el `tokio::spawn` de `CatalogStore::delete_track`. Por
        // eso `block_in_place` es seguro aquí: el runtime activo es
        // multi-thread (rt-multi-thread está en Cargo.toml). El fallback a
        // `Runtime::new()` queda solo como red de seguridad; en la práctica
        // nunca debería activarse dado este setup.
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://music_center.db".into());

        let init_playlist_manager = async {
            let pool = init_db(&database_url)
                .await
                .expect("Fallo fatal al inicializar la base de datos local");
            PlaylistManager::new(pool)
                .await
                .expect("Fallo fatal al inicializar PlaylistManager")
        };

        let playlist_manager = match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(init_playlist_manager)),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()
                    .expect("No se pudo crear runtime temporal para inicializar la BD");
                rt.block_on(init_playlist_manager)
            }
        };
        let playlist_manager = Arc::new(playlist_manager);

        let (sidebar_feature, sidebar_task) = SidebarFeature::new(
            sidebar_client,
            Arc::clone(&playlist_manager),
            Arc::clone(&manager),
        );

        let app = Self {
            _engine: engine,
            search_feature: SearchFeature::new(),
            playback_feature: PlaybackFeature::new(Arc::clone(&manager)),
            sidebar_feature,
            player_thumbnails: ThumbnailCache::new(250, 50),
            view_thumbnails: ThumbnailCache::new(100, 50),
            radio,
            tray_flags,
            main_window: Some(window_id),
            manager,
            is_theater_mode: false,
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
                iced::exit()
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

                let (tick_task, _out) = self.playback_feature
                    .update(PlaybackFeatureMessage::Tick, &mut self.player_thumbnails);

                iced::Task::batch(vec![
                    position_task.map(AppMessage::PlaybackFeature),
                    tick_task.map(AppMessage::PlaybackFeature),
                ])
            }

            AppMessage::PlaybackFeature(msg) => {
                let (task, out_msg) = self.playback_feature.update(msg, &mut self.player_thumbnails);

                let like_task = match out_msg {
                    PlaybackOutMessage::ToggleTheaterMode => {
                        self.is_theater_mode = !self.is_theater_mode;
                        iced::Task::none()
                    }
                    PlaybackOutMessage::RequestToggleLike(track_id) => {
                        // El estado de like vive en `CatalogStore`, dueño de
                        // `SidebarFeature`; `PlaybackFeature` no lo conoce,
                        // así que el toggle se resuelve aquí y el resultado
                        // (LikeToggled) se enruta como un CatalogStoreMessage
                        // normal hacia el sidebar.
                        self.sidebar_feature
                            .catalog_store
                            .toggle_like(&track_id)
                            .map(|catalog_msg| {
                                AppMessage::SidebarFeature(SidebarFeatureMessage::Catalog(catalog_msg))
                            })
                    }
                    PlaybackOutMessage::Idle => iced::Task::none(),
                };

                iced::Task::batch(vec![
                    task.map(AppMessage::PlaybackFeature),
                    like_task,
                ])
            }

            AppMessage::SidebarFeature(msg) => {
                // `SidebarFeature` resuelve internamente play/enqueue y el
                // flujo de creación/eliminación de playlists contra sus
                // propios distritos (Explorer/Playlists) y `CatalogStore`.
                // Main ya no necesita reaccionar a ningún out-message por
                // ahora.
                let (task, _out_msg) = self.sidebar_feature.update(msg, &mut self.view_thumbnails);

                task.map(AppMessage::SidebarFeature)
            }

            AppMessage::SearchFeature(msg) => {
                let (search_task, out_msg) = self.search_feature.update(msg, &mut self.view_thumbnails);
                let mut feature_task = iced::Task::none();

                if let SearchFeatureOutMessage::TrackReadyToPlay(playable) = out_msg {
                    let (t, _out) = self.playback_feature.update(
                        PlaybackFeatureMessage::Play(playable),
                        &mut self.player_thumbnails,
                    );
                    feature_task = t;
                }

                iced::Task::batch(vec![
                    search_task.map(AppMessage::SearchFeature),
                    feature_task.map(AppMessage::PlaybackFeature),
                ])
            }
        }
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, AppMessage> {
        let center_content: Element<'_, AppMessage> = if self.is_theater_mode {
            self.playback_feature.view_theater().map(AppMessage::PlaybackFeature)
        } else {
            self.sidebar_feature.view_content(&self.view_thumbnails).map(AppMessage::SidebarFeature)
        };

        let center_view = container(center_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .style(|_theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.15, 0.15, 0.20))),
                border: Border {
                    radius: border::Radius::from(18.0),
                    ..Default::default()
                },
                ..Default::default()
            });

        let queue_layer = container(
            self.playback_feature.view_queue(&self.player_thumbnails).map(AppMessage::PlaybackFeature)
        )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .padding(Padding {
                top: 10.0,
                right: 20.0,
                bottom: 10.0,
                left: 0.0,
            });

        let content_layer = row![
            self.sidebar_feature.view_sidebar().map(AppMessage::SidebarFeature),
            space().width(15),
            center_view,
            space().width(15),
        ]
            .width(Length::Fill)
            .height(Length::Fill);

        let layout_stack = stack![content_layer, queue_layer];

        let is_current_liked = self.playback_feature.current_track_id()
            .and_then(|id| self.sidebar_feature.catalog_store.track_by_id(id))
            .map(|t| t.liked)
            .unwrap_or(false);

        let playback_view  = self.playback_feature.view(&self.player_thumbnails, self.is_theater_mode, is_current_liked).map(AppMessage::PlaybackFeature);
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
                background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
                ..Default::default()
            });

        stack![
            app_root,
            search_overlay,
        ]
            .into()
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

        iced::Subscription::batch(vec![search_sub, playback_sub, sidebar_sub, close_sub])
    }

    pub fn theme(&self, _window: window::Id) -> Theme {
        Theme::Dracula
    }
}

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