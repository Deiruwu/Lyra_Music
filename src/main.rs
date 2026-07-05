mod microservices;
mod model;
mod audio;
mod ui;
pub mod tray;

use std::sync::{Arc, OnceLock};
use std::sync::atomic::Ordering;
use iced::{border, window, Background, Border, Color, Element, Font, Length, Padding, Theme};
use iced::widget::{column, container, row, space, stack};
use crate::audio::discord::DiscordPresence;
use crate::audio::download_daemon::DownloadWorker;
use crate::audio::engine::AudioEngine;
use audio::mananger::manager::TrackManager;
use crate::audio::mpris::MprisServer;
use crate::audio::radio_daemon::RadioWorker;
use crate::microservices::client::MicroserviceClient;
use crate::tray::TrayFlags;
use crate::ui::playback_feature::playback_feature::{PlaybackFeature, PlaybackFeatureMessage};
use crate::ui::search_feature::search_feature::{SearchFeature, SearchFeatureMessage, SearchFeatureOutMessage};
use crate::ui::utils::thumbnail_cache::ThumbnailCache;

const JETBRAINS_MONO: Font = Font::with_name("JetBrainsMono Nerd Font");

static TRAY_FLAGS: OnceLock<Arc<TrayFlags>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum AppMessage {
    SearchFeature(SearchFeatureMessage),
    PlaybackFeature(PlaybackFeatureMessage),
    WindowOpened(window::Id),
    CloseRequested(window::Id),
    ShowWindow,
    Quit,
}

struct App {
    _engine: AudioEngine,
    search_feature: SearchFeature,
    playback_feature: PlaybackFeature,
    thumbnails: ThumbnailCache,
    radio: Arc<RadioWorker>,
    tray_flags: Arc<TrayFlags>,
    main_window: Option<window::Id>,
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

        let radio = RadioWorker::new(Arc::clone(&manager), client).spawn();
        radio.set_enabled(true);
        radio.set_queue_target(15);

        let (window_id, open_task) = window::open(window::Settings {
            size: iced::Size::new(800.0, 600.0),
            exit_on_close_request: false,
            ..Default::default()
        });

        let app = Self {
            _engine: engine,
            search_feature: SearchFeature::new(),
            playback_feature: PlaybackFeature::new(Arc::clone(&manager)),
            thumbnails: ThumbnailCache::new(250, 50),
            radio,
            tray_flags,
            main_window: Some(window_id),
        };

        (app, open_task.map(AppMessage::WindowOpened))
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
                self.playback_feature
                    .update(PlaybackFeatureMessage::Tick, &mut self.thumbnails)
                    .map(AppMessage::PlaybackFeature)
            }

            AppMessage::PlaybackFeature(msg) => {
                self.playback_feature.update(msg, &mut self.thumbnails).map(AppMessage::PlaybackFeature)
            }

            AppMessage::SearchFeature(msg) => {
                let (search_task, out_msg) = self.search_feature.update(msg, &mut self.thumbnails);

                let mut feature_task = iced::Task::none();

                if let SearchFeatureOutMessage::TrackReadyToPlay(playable) = out_msg {
                    feature_task = self.playback_feature.update(
                        PlaybackFeatureMessage::Play(playable),
                        &mut self.thumbnails,
                    );
                }

                iced::Task::batch(vec![
                    search_task.map(AppMessage::SearchFeature),
                    feature_task.map(AppMessage::PlaybackFeature),
                ])
            }
        }
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, AppMessage> {
        let center_view = container(
            self.playback_feature.view_lyrics().map(AppMessage::PlaybackFeature)
        )
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
            self.playback_feature.view_queue(&self.thumbnails).map(AppMessage::PlaybackFeature)
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
            space().width(15),
            center_view,
            space().width(15),
        ]
            .width(Length::Fill)
            .height(Length::Fill);

        let layout_stack = stack![content_layer, queue_layer];

        let playback_view  = self.playback_feature.view(&self.thumbnails).map(AppMessage::PlaybackFeature);
        let search_view    = self.search_feature.view().map(AppMessage::SearchFeature);
        let search_overlay = self.search_feature.view_dropdown(&self.thumbnails).map(AppMessage::SearchFeature);

        stack![
            column![search_view, layout_stack, playback_view].height(Length::Fill),
            search_overlay,
        ]
            .into()
    }

    pub fn subscription(&self) -> iced::Subscription<AppMessage> {
        let search_sub  = self.search_feature.subscription().map(AppMessage::SearchFeature);
        let feature_sub = self.playback_feature.subscription().map(AppMessage::PlaybackFeature);
        let close_sub = window::events()
            .filter_map(|(id, event)| match event {
                window::Event::CloseRequested => Some(AppMessage::CloseRequested(id)),
                _ => None,
            });
        iced::Subscription::batch(vec![search_sub, feature_sub, close_sub])
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
        .theme(App::theme)
        .run()
}